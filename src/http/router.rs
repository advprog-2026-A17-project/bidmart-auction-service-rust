use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};

use crate::http::dto::{
    AuctionPageResponse, AuctionResponse, BidResponse, CreateAuctionRequest, ErrorResponse,
    PlaceBidRequest,
};
use crate::service::auction_service::{
    AuctionService, CloseAuctionError, CreateAuctionError, GetAuctionError, ListAuctionsError,
    ListBidsError, ListPendingClosureError, PlaceBidError,
};

#[derive(Debug, Clone)]
pub struct AppState {
    auction_service: Arc<AuctionService>,
}

pub fn create_router(auction_service: AuctionService) -> Router {
    let state = AppState {
        auction_service: Arc::new(auction_service),
    };
    Router::new()
        .route("/auctions", get(list_auctions).post(create_auction))
        .route("/auctions/:auction_id", get(get_auction_by_id))
        .route("/auctions/:auction_id/bids", get(list_bids).post(place_bid))
        .route("/api/v1/auctions", get(list_auctions).post(create_auction))
        .route(
            "/api/v1/auctions/pending-closure",
            get(list_pending_closure),
        )
        .route("/api/v1/auctions/:auction_id", get(get_auction_by_id))
        .route(
            "/api/v1/auctions/:auction_id/close",
            axum::routing::post(close_auction),
        )
        .route(
            "/api/v1/auctions/:auction_id/bids",
            get(list_bids).post(place_bid),
        )
        .with_state(state)
}

async fn create_auction(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(mut request): Json<CreateAuctionRequest>,
) -> Result<(StatusCode, Json<AuctionResponse>), ApiError> {
    apply_trusted_user_id(&headers, &mut request.seller_id, "seller_id")?;
    let command = request.try_into_command().map_err(ApiError::bad_request)?;
    let auction = state.auction_service.create_auction(command).await?;
    Ok((StatusCode::CREATED, Json(auction.into())))
}

async fn list_auctions(
    State(state): State<AppState>,
) -> Result<Json<AuctionPageResponse>, ApiError> {
    let auctions = state.auction_service.list_auctions().await?;
    let items: Vec<AuctionResponse> = auctions.into_iter().map(AuctionResponse::from).collect();
    let size = items.len() as i64;

    Ok(Json(AuctionPageResponse {
        items,
        page: 0,
        size,
        total_items: size,
        total_pages: if size == 0 { 0 } else { 1 },
    }))
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

async fn list_pending_closure(
    State(state): State<AppState>,
) -> Result<Json<Vec<AuctionResponse>>, ApiError> {
    let auctions = state.auction_service.list_pending_closure().await?;
    let response = auctions.into_iter().map(AuctionResponse::from).collect();
    Ok(Json(response))
}

async fn close_auction(
    State(state): State<AppState>,
    Path(auction_id): Path<String>,
) -> Result<Json<AuctionResponse>, ApiError> {
    let auction = state.auction_service.close_auction(&auction_id).await?;
    Ok(Json(auction.into()))
}

async fn place_bid(
    State(state): State<AppState>,
    Path(auction_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<PlaceBidRequest>,
) -> Result<(StatusCode, Json<BidResponse>), ApiError> {
    let bidder_id = resolve_trusted_user_id(&headers, request.bidder_id(), "bidder_id")?;
    let bid_amount_cents = request
        .bid_amount_cents()
        .ok_or_else(|| ApiError::bad_request("bid_amount is required"))?;

    let bid = state
        .auction_service
        .place_bid_and_persist(
            &auction_id,
            &bidder_id,
            bid_amount_cents,
            request.bid_time(),
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

    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
        }
    }
}

fn apply_trusted_user_id(
    headers: &HeaderMap,
    request_user_id: &mut Option<String>,
    field_name: &str,
) -> Result<(), ApiError> {
    let Some(trusted_user_id) = trusted_user_id(headers) else {
        return Ok(());
    };

    if let Some(body_user_id) = request_user_id.as_deref().filter(|value| !value.is_empty()) {
        if body_user_id != trusted_user_id {
            return Err(ApiError::forbidden(format!(
                "{field_name} does not match authenticated user"
            )));
        }
    }

    *request_user_id = Some(trusted_user_id);
    Ok(())
}

fn resolve_trusted_user_id(
    headers: &HeaderMap,
    request_user_id: Option<&str>,
    field_name: &str,
) -> Result<String, ApiError> {
    let mut resolved = request_user_id.map(str::to_string);
    apply_trusted_user_id(headers, &mut resolved, field_name)?;
    resolved.ok_or_else(|| ApiError::bad_request(format!("{field_name} is required")))
}

fn trusted_user_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-user-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
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

impl From<ListAuctionsError> for ApiError {
    fn from(error: ListAuctionsError) -> Self {
        match error {
            ListAuctionsError::DatabaseError(message) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message,
            },
        }
    }
}

impl From<ListPendingClosureError> for ApiError {
    fn from(error: ListPendingClosureError) -> Self {
        match error {
            ListPendingClosureError::DatabaseError(message) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message,
            },
        }
    }
}

impl From<CloseAuctionError> for ApiError {
    fn from(error: CloseAuctionError) -> Self {
        match error {
            CloseAuctionError::AuctionNotFound => Self {
                status: StatusCode::NOT_FOUND,
                message: "auction not found".to_string(),
            },
            CloseAuctionError::AuctionNotEnded => Self {
                status: StatusCode::BAD_REQUEST,
                message: "auction has not reached its end time".to_string(),
            },
            CloseAuctionError::WalletError(message) => Self {
                status: StatusCode::PAYMENT_REQUIRED,
                message,
            },
            CloseAuctionError::DatabaseError(message) => Self {
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
            PlaceBidError::CatalogError(message) => Self {
                status: StatusCode::BAD_REQUEST,
                message,
            },
            PlaceBidError::WalletError(message) => Self {
                status: StatusCode::PAYMENT_REQUIRED,
                message,
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
