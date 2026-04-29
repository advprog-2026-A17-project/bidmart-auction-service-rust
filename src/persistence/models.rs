use sqlx::FromRow;

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct AuctionRecord {
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
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAuctionRecord {
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
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct BidRecord {
    pub id: String,
    pub auction_id: String,
    pub bidder_id: String,
    pub bid_amount_cents: i64,
    pub bid_time: i64,
    pub wallet_hold_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewBidRecord {
    pub id: String,
    pub auction_id: String,
    pub bidder_id: String,
    pub bid_amount_cents: i64,
    pub bid_time: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct OutboxEventRecord {
    pub id: String,
    pub aggregate_id: String,
    pub event_type: String,
    pub payload: String,
    pub published: bool,
    pub published_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOutboxEventRecord {
    pub id: String,
    pub aggregate_id: String,
    pub event_type: String,
    pub payload: String,
    pub published: bool,
    pub published_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}
