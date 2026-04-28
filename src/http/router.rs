use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};

use crate::http::dto::{AuctionResponse, CreateAuctionRequest, ErrorResponse};
use crate::service::auction_service::{AuctionService, CreateAuctionError};

#[derive(Debug, Clone)]
pub struct AppState {
    auction_service: AuctionService,
}

pub fn create_router(auction_service: AuctionService) -> Router {
    Router::new()
        .route("/auctions", post(create_auction))
        .with_state(AppState { auction_service })
}

async fn create_auction(
    State(state): State<AppState>,
    Json(request): Json<CreateAuctionRequest>,
) -> Result<(StatusCode, Json<AuctionResponse>), ApiError> {
    let auction = state.auction_service.create_auction(request.into()).await?;
    Ok((StatusCode::CREATED, Json(auction.into())))
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
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
