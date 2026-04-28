use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::http::dto::{
    AuctionResponse, BidResponse, CreateAuctionRequest, ErrorResponse, PlaceBidRequest,
};
use crate::service::auction_service::{
    AuctionService, CreateAuctionError, GetAuctionError, ListBidsError, PlaceBidError,
};

#[derive(Debug, Clone)]
pub struct AppState {
    auction_service: AuctionService,
}

pub fn create_router(auction_service: AuctionService) -> Router {
    Router::new()
        .route("/auctions", post(create_auction))
        .route("/auctions/:auction_id", get(get_auction_by_id))
        .route("/auctions/:auction_id/bids", get(list_bids).post(place_bid))
        .with_state(AppState { auction_service })
}

async fn create_auction(
    State(state): State<AppState>,
    Json(request): Json<CreateAuctionRequest>,
) -> Result<(StatusCode, Json<AuctionResponse>), ApiError> {
    let auction = state.auction_service.create_auction(request.into()).await?;
    Ok((StatusCode::CREATED, Json(auction.into())))
}

async fn get_auction_by_id(
    State(state): State<AppState>,
    Path(auction_id): Path<String>,
) -> Result<Json<AuctionResponse>, ApiError> {
    let auction = state
        .auction_service
        .get_auction_by_id(&auction_id)
        .await?
        .ok_or_else(|| ApiError::not_found("auction not found"))?;

    Ok(Json(auction.into()))
}

async fn place_bid(
    State(state): State<AppState>,
    Path(auction_id): Path<String>,
    Json(request): Json<PlaceBidRequest>,
) -> Result<(StatusCode, Json<BidResponse>), ApiError> {
    let bid = state
        .auction_service
        .place_bid_and_persist(
            &auction_id,
            &request.bidder_id,
            request.bid_amount_cents,
            request.bid_time,
        )
        .await?;

    Ok((StatusCode::CREATED, Json(bid.into())))
}

async fn list_bids(
    State(state): State<AppState>,
    Path(auction_id): Path<String>,
) -> Result<Json<Vec<BidResponse>>, ApiError> {
    let bids = state.auction_service.list_bids(&auction_id).await?;
    let response = bids.into_iter().map(BidResponse::from).collect();

    Ok(Json(response))
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }
}

impl From<CreateAuctionError> for ApiError {
    fn from(error: CreateAuctionError) -> Self {
        match error {
            CreateAuctionError::InvalidInput(message) => Self {
                status: StatusCode::BAD_REQUEST,
                message,
            },
            CreateAuctionError::DatabaseError(message) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message,
            },
        }
    }
}

impl From<GetAuctionError> for ApiError {
    fn from(error: GetAuctionError) -> Self {
        match error {
            GetAuctionError::DatabaseError(message) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message,
            },
        }
    }
}

impl From<PlaceBidError> for ApiError {
    fn from(error: PlaceBidError) -> Self {
        match error {
            PlaceBidError::AuctionNotFound => Self {
                status: StatusCode::NOT_FOUND,
                message: "auction not found".to_string(),
            },
            PlaceBidError::BidError(error) => Self {
                status: StatusCode::BAD_REQUEST,
                message: error.to_string(),
            },
            PlaceBidError::DatabaseError(message) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message,
            },
        }
    }
}

impl From<ListBidsError> for ApiError {
    fn from(error: ListBidsError) -> Self {
        match error {
            ListBidsError::DatabaseError(message) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message,
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorResponse {
                message: self.message,
            }),
        )
            .into_response()
    }
}
