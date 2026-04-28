use serde::{Deserialize, Serialize};

use crate::persistence::models::{AuctionRecord, BidRecord};
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
    #[serde(rename = "listingId")]
    pub listing_id_api: String,
    #[serde(rename = "sellerId")]
    pub seller_id_api: String,
    #[serde(rename = "startingPrice")]
    pub starting_price: f64,
    #[serde(rename = "reservePrice")]
    pub reserve_price: f64,
    #[serde(rename = "currentHighestBid")]
    pub current_highest_bid: Option<f64>,
    #[serde(rename = "minimumIncrement")]
    pub minimum_increment: f64,
    #[serde(rename = "startTime")]
    pub start_time_api: String,
    #[serde(rename = "endTime")]
    pub end_time_api: String,
}

impl From<AuctionRecord> for AuctionResponse {
    fn from(record: AuctionRecord) -> Self {
        let listing_id = record.listing_id;
        let seller_id = record.seller_id;
        let starting_price_cents = record.starting_price_cents;
        let reserve_price_cents = record.reserve_price_cents;
        let current_highest_bid_cents = record.current_highest_bid_cents;
        let minimum_increment_cents = record.minimum_increment_cents;
        let start_time = record.start_time;
        let end_time = record.end_time;

        Self {
            id: record.id,
            listing_id: listing_id.clone(),
            seller_id: seller_id.clone(),
            starting_price_cents,
            reserve_price_cents,
            current_highest_bid_cents,
            minimum_increment_cents,
            status: record.status,
            start_time,
            end_time,
            listing_id_api: listing_id,
            seller_id_api: seller_id,
            starting_price: cents_to_decimal(starting_price_cents),
            reserve_price: cents_to_decimal(reserve_price_cents),
            current_highest_bid: current_highest_bid_cents.map(cents_to_decimal),
            minimum_increment: cents_to_decimal(minimum_increment_cents),
            start_time_api: unix_seconds_to_rfc3339(start_time),
            end_time_api: unix_seconds_to_rfc3339(end_time),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AuctionPageResponse {
    pub items: Vec<AuctionResponse>,
    pub page: i64,
    pub size: i64,
    #[serde(rename = "totalItems")]
    pub total_items: i64,
    #[serde(rename = "totalPages")]
    pub total_pages: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorResponse {
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlaceBidRequest {
    #[serde(default, alias = "bidderId")]
    pub bidder_id: Option<String>,
    #[serde(default)]
    pub bid_amount_cents: Option<i64>,
    #[serde(default, rename = "bidAmount")]
    pub bid_amount: Option<f64>,
    #[serde(default)]
    pub bid_time: Option<i64>,
}

impl PlaceBidRequest {
    pub fn bidder_id(&self) -> Option<&str> {
        self.bidder_id.as_deref()
    }

    pub fn bid_amount_cents(&self) -> Option<i64> {
        self.bid_amount_cents
            .or_else(|| self.bid_amount.map(decimal_to_cents))
    }

    pub fn bid_time(&self) -> i64 {
        self.bid_time
            .unwrap_or_else(|| chrono::Utc::now().timestamp())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BidResponse {
    pub id: String,
    pub auction_id: String,
    pub bidder_id: String,
    pub bid_amount_cents: i64,
    pub bid_time: i64,
    #[serde(rename = "auctionId")]
    pub auction_id_api: String,
    #[serde(rename = "bidderId")]
    pub bidder_id_api: String,
    #[serde(rename = "bidAmount")]
    pub bid_amount: f64,
    #[serde(rename = "bidTime")]
    pub bid_time_api: String,
}

impl From<BidRecord> for BidResponse {
    fn from(record: BidRecord) -> Self {
        let auction_id = record.auction_id;
        let bidder_id = record.bidder_id;
        let bid_amount_cents = record.bid_amount_cents;
        let bid_time = record.bid_time;

        Self {
            id: record.id,
            auction_id: auction_id.clone(),
            bidder_id: bidder_id.clone(),
            bid_amount_cents,
            bid_time,
            auction_id_api: auction_id,
            bidder_id_api: bidder_id,
            bid_amount: cents_to_decimal(bid_amount_cents),
            bid_time_api: unix_seconds_to_rfc3339(bid_time),
        }
    }
}

fn cents_to_decimal(cents: i64) -> f64 {
    cents as f64 / 100.0
}

fn decimal_to_cents(amount: f64) -> i64 {
    (amount * 100.0).round() as i64
}

fn unix_seconds_to_rfc3339(seconds: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(seconds, 0)
        .map(|datetime| datetime.to_rfc3339())
        .unwrap_or_else(|| seconds.to_string())
}
