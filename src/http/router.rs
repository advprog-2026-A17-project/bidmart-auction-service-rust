use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::http::dto::{AuctionResponse, CreateAuctionRequest, ErrorResponse};
use crate::service::auction_service::{AuctionService, CreateAuctionError, GetAuctionError};

#[derive(Debug, Clone)]
pub struct AppState {
    auction_service: AuctionService,
}

pub fn create_router(auction_service: AuctionService) -> Router {
    Router::new()
        .route("/auctions", post(create_auction))
        .route("/auctions/:auction_id", get(get_auction_by_id))
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
