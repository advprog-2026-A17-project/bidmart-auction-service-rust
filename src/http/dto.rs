use serde::{Deserialize, Serialize};

use crate::persistence::models::AuctionRecord;
use crate::service::auction_service::CreateAuctionCommand;

#[derive(Debug, Clone, Deserialize)]
pub struct CreateAuctionRequest {
    pub listing_id: String,
    pub seller_id: String,
    pub starting_price_cents: i64,
    pub reserve_price_cents: i64,
    pub minimum_increment_cents: i64,
    pub start_time: i64,
    pub end_time: i64,
}

impl From<CreateAuctionRequest> for CreateAuctionCommand {
    fn from(request: CreateAuctionRequest) -> Self {
        Self {
            listing_id: request.listing_id,
            seller_id: request.seller_id,
            starting_price_cents: request.starting_price_cents,
            reserve_price_cents: request.reserve_price_cents,
            minimum_increment_cents: request.minimum_increment_cents,
            start_time: request.start_time,
            end_time: request.end_time,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AuctionResponse {
    pub id: String,
    pub listing_id: String,
    pub seller_id: String,
    pub starting_price_cents: i64,
    pub reserve_price_cents: i64,
    pub current_highest_bid_cents: Option<i64>,
    pub minimum_increment_cents: i64,
    pub status: String,
    pub start_time: i64,
    pub end_time: i64,
}

impl From<AuctionRecord> for AuctionResponse {
    fn from(record: AuctionRecord) -> Self {
        Self {
            id: record.id,
            listing_id: record.listing_id,
            seller_id: record.seller_id,
            starting_price_cents: record.starting_price_cents,
            reserve_price_cents: record.reserve_price_cents,
            current_highest_bid_cents: record.current_highest_bid_cents,
            minimum_increment_cents: record.minimum_increment_cents,
            status: record.status,
            start_time: record.start_time,
            end_time: record.end_time,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorResponse {
    pub message: String,
}
