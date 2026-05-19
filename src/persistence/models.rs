use sqlx::{FromRow, Row, any::AnyRow};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListingAuctionSessionRecord {
    pub id: String,
    pub listing_id: String,
    pub seller_id: String,
    pub auction_type: String,
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
pub struct NewListingAuctionSessionRecord {
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

#[derive(Debug, Clone, PartialEq, Eq)]
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

fn optional_i64(row: &AnyRow, column: &str) -> Result<Option<i64>, sqlx::Error> {
    match row.try_get(column) {
        Ok(value) => Ok(Some(value)),
        Err(sqlx::Error::ColumnDecode { source, .. })
            if source.to_string().contains("SQL type `NULL`") =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn optional_string(row: &AnyRow, column: &str) -> Result<Option<String>, sqlx::Error> {
    match row.try_get(column) {
        Ok(value) => Ok(Some(value)),
        Err(sqlx::Error::ColumnDecode { source, .. })
            if source.to_string().contains("SQL type `NULL`") =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn bool_from_row(row: &AnyRow, column: &str) -> Result<bool, sqlx::Error> {
    match row.try_get(column) {
        Ok(value) => Ok(value),
        Err(_) => row.try_get::<i64, _>(column).map(|value| value != 0),
    }
}

impl<'r> FromRow<'r, AnyRow> for ListingAuctionSessionRecord {
    fn from_row(row: &'r AnyRow) -> Result<Self, sqlx::Error> {
        let id: String = row.try_get("id")?;
        let listing_id = row
            .try_get::<String, _>("listing_id")
            .unwrap_or_else(|_| id.clone());
        let status = row
            .try_get::<String, _>("status")
            .or_else(|_| row.try_get("lifecycle_state"))?;
        Ok(Self {
            id,
            listing_id,
            seller_id: row.try_get("seller_id")?,
            auction_type: row.try_get("auction_type")?,
            starting_price_cents: row.try_get("starting_price_cents")?,
            reserve_price_cents: row.try_get("reserve_price_cents")?,
            current_highest_bid_cents: optional_i64(row, "current_highest_bid_cents")?,
            minimum_increment_cents: row.try_get("minimum_increment_cents")?,
            status,
            start_time: row.try_get("start_time")?,
            end_time: row.try_get("end_time")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

impl<'r> FromRow<'r, AnyRow> for BidRecord {
    fn from_row(row: &'r AnyRow) -> Result<Self, sqlx::Error> {
        let auction_id = row
            .try_get::<String, _>("auction_id")
            .or_else(|_| row.try_get("listing_id"))?;
        Ok(Self {
            id: row.try_get("id")?,
            auction_id,
            bidder_id: row.try_get("bidder_id")?,
            bid_amount_cents: row.try_get("bid_amount_cents")?,
            bid_time: row.try_get("bid_time")?,
            wallet_hold_id: optional_string(row, "wallet_hold_id")?,
        })
    }
}

impl<'r> FromRow<'r, AnyRow> for OutboxEventRecord {
    fn from_row(row: &'r AnyRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            aggregate_id: row.try_get("aggregate_id")?,
            event_type: row.try_get("event_type")?,
            payload: row.try_get("payload")?,
            published: bool_from_row(row, "published")?,
            published_at: optional_i64(row, "published_at")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}
