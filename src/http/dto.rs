use serde::{Deserialize, Serialize};

use crate::persistence::models::{ListingAuctionSessionRecord, BidRecord};
use crate::service::auction_service::CreateAuctionCommand;
use crate::service::auction_strategy::AuctionType;

#[derive(Debug, Clone, Deserialize)]
pub struct CreateAuctionRequest {
    #[serde(default, alias = "listingId")]
    pub listing_id: Option<String>,
    #[serde(default, alias = "sellerId")]
    pub seller_id: Option<String>,
    #[serde(default, alias = "auctionType")]
    pub auction_type: Option<String>,
    #[serde(default)]
    pub starting_price_cents: Option<i64>,
    #[serde(default, rename = "startingPrice")]
    pub starting_price: Option<f64>,
    #[serde(default)]
    pub reserve_price_cents: Option<i64>,
    #[serde(default, rename = "reservePrice")]
    pub reserve_price: Option<f64>,
    #[serde(default)]
    pub minimum_increment_cents: Option<i64>,
    #[serde(default, rename = "minimumIncrement")]
    pub minimum_increment: Option<f64>,
    #[serde(default, alias = "startTime")]
    pub start_time: Option<RequestTimestamp>,
    #[serde(default, alias = "endTime")]
    pub end_time: Option<RequestTimestamp>,
}

impl CreateAuctionRequest {
    pub fn try_into_command(self) -> Result<CreateAuctionCommand, String> {
        let auction_type = AuctionType::from_input(self.auction_type.as_deref())?;
        Ok(CreateAuctionCommand {
            listing_id: self
                .listing_id
                .ok_or_else(|| "listing_id is required".to_string())?,
            seller_id: self
                .seller_id
                .ok_or_else(|| "seller_id is required".to_string())?,
            starting_price_cents: self
                .starting_price_cents
                .or_else(|| self.starting_price.map(decimal_to_cents))
                .ok_or_else(|| "starting_price is required".to_string())?,
            reserve_price_cents: self
                .reserve_price_cents
                .or_else(|| self.reserve_price.map(decimal_to_cents))
                .ok_or_else(|| "reserve_price is required".to_string())?,
            minimum_increment_cents: self
                .minimum_increment_cents
                .or_else(|| self.minimum_increment.map(decimal_to_cents))
                .ok_or_else(|| "minimum_increment is required".to_string())?,
            start_time: resolve_unix_seconds(self.start_time, "start_time")?,
            end_time: resolve_unix_seconds(self.end_time, "end_time")?,
            auction_type: auction_type.as_storage_value().to_string(),
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum RequestTimestamp {
    UnixSeconds(i64),
    Rfc3339(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct AuctionResponse {
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
    #[serde(rename = "listingId")]
    pub listing_id_api: String,
    #[serde(rename = "sellerId")]
    pub seller_id_api: String,
    #[serde(rename = "auctionType")]
    pub auction_type_api: String,
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

impl From<ListingAuctionSessionRecord> for AuctionResponse {
    fn from(record: ListingAuctionSessionRecord) -> Self {
        let listing_id = record.listing_id;
        let seller_id = record.seller_id;
        let auction_type = record.auction_type;
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
            auction_type: auction_type.clone(),
            starting_price_cents,
            reserve_price_cents,
            current_highest_bid_cents,
            minimum_increment_cents,
            status: record.status,
            start_time,
            end_time,
            listing_id_api: listing_id,
            seller_id_api: seller_id,
            auction_type_api: auction_type,
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

#[derive(Debug, Clone, Deserialize)]
pub struct PlaceProxyBidRequest {
    #[serde(default, alias = "bidderId")]
    pub bidder_id: Option<String>,
    #[serde(default)]
    pub max_bid_amount_cents: Option<i64>,
    #[serde(default, rename = "maxBidAmount")]
    pub max_bid_amount: Option<f64>,
    #[serde(default)]
    pub bid_time: Option<i64>,
}

impl PlaceProxyBidRequest {
    pub fn bidder_id(&self) -> Option<&str> {
        self.bidder_id.as_deref()
    }

    pub fn max_bid_amount_cents(&self) -> Option<i64> {
        self.max_bid_amount_cents
            .or_else(|| self.max_bid_amount.map(decimal_to_cents))
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

#[derive(Debug, Clone, Serialize)]
pub struct BidCursorPageResponse {
    pub items: Vec<BidResponse>,
    #[serde(rename = "nextCursor")]
    pub next_cursor: Option<String>,
    pub size: i64,
}

fn cents_to_decimal(cents: i64) -> f64 {
    cents as f64 / 100.0
}

fn decimal_to_cents(amount: f64) -> i64 {
    (amount * 100.0).round() as i64
}

fn resolve_unix_seconds(value: Option<RequestTimestamp>, field: &str) -> Result<i64, String> {
    match value.ok_or_else(|| format!("{field} is required"))? {
        RequestTimestamp::UnixSeconds(seconds) => Ok(seconds),
        RequestTimestamp::Rfc3339(value) => chrono::DateTime::parse_from_rfc3339(&value)
            .map(|datetime| datetime.timestamp())
            .map_err(|_| format!("{field} must be unix seconds or RFC3339")),
    }
}

fn unix_seconds_to_rfc3339(seconds: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(seconds, 0)
        .map(|datetime| datetime.to_rfc3339())
        .unwrap_or_else(|| seconds.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn cents_to_dec() { assert!((cents_to_decimal(1050) - 10.5).abs() < f64::EPSILON); }
    #[test] fn dec_to_cents() { assert_eq!(decimal_to_cents(10.50), 1050); assert_eq!(decimal_to_cents(99.999), 10000); }

    #[test] fn resolve_int() { assert_eq!(resolve_unix_seconds(Some(RequestTimestamp::UnixSeconds(100)), "t").unwrap(), 100); }
    #[test] fn resolve_rfc() { assert!(resolve_unix_seconds(Some(RequestTimestamp::Rfc3339("2026-01-01T00:00:00Z".into())), "t").is_ok()); }
    #[test] fn resolve_bad_rfc() { assert!(resolve_unix_seconds(Some(RequestTimestamp::Rfc3339("bad".into())), "t").is_err()); }
    #[test] fn resolve_none() { assert!(resolve_unix_seconds(None, "t").is_err()); }
    #[test] fn rfc3339_valid() { assert!(unix_seconds_to_rfc3339(1700000000).contains("2023")); }

    #[test]
    fn try_into_full_cents() {
        let r = CreateAuctionRequest { listing_id: Some("l".into()), seller_id: Some("s".into()), auction_type: Some("ENGLISH".into()), starting_price_cents: Some(1000), starting_price: None, reserve_price_cents: Some(2000), reserve_price: None, minimum_increment_cents: Some(100), minimum_increment: None, start_time: Some(RequestTimestamp::UnixSeconds(100)), end_time: Some(RequestTimestamp::UnixSeconds(200)) };
        let c = r.try_into_command().unwrap();
        assert_eq!(c.starting_price_cents, 1000);
    }

    #[test]
    fn try_into_decimal_fallback() {
        let r = CreateAuctionRequest { listing_id: Some("l".into()), seller_id: Some("s".into()), auction_type: Some("ENGLISH".into()), starting_price_cents: None, starting_price: Some(10.0), reserve_price_cents: None, reserve_price: Some(20.0), minimum_increment_cents: None, minimum_increment: Some(1.0), start_time: Some(RequestTimestamp::UnixSeconds(100)), end_time: Some(RequestTimestamp::UnixSeconds(200)) };
        let c = r.try_into_command().unwrap();
        assert_eq!(c.starting_price_cents, 1000);
    }

    #[test]
    fn try_into_missing_listing() {
        let r = CreateAuctionRequest { listing_id: None, seller_id: Some("s".into()), auction_type: Some("ENGLISH".into()), starting_price_cents: Some(1000), starting_price: None, reserve_price_cents: Some(2000), reserve_price: None, minimum_increment_cents: Some(100), minimum_increment: None, start_time: Some(RequestTimestamp::UnixSeconds(100)), end_time: Some(RequestTimestamp::UnixSeconds(200)) };
        assert!(r.try_into_command().is_err());
    }

    #[test]
    fn try_into_missing_seller() {
        let r = CreateAuctionRequest { listing_id: Some("l".into()), seller_id: None, auction_type: Some("ENGLISH".into()), starting_price_cents: Some(1000), starting_price: None, reserve_price_cents: Some(2000), reserve_price: None, minimum_increment_cents: Some(100), minimum_increment: None, start_time: Some(RequestTimestamp::UnixSeconds(100)), end_time: Some(RequestTimestamp::UnixSeconds(200)) };
        assert!(r.try_into_command().is_err());
    }

    #[test]
    fn try_into_missing_starting() {
        let r = CreateAuctionRequest { listing_id: Some("l".into()), seller_id: Some("s".into()), auction_type: Some("ENGLISH".into()), starting_price_cents: None, starting_price: None, reserve_price_cents: Some(2000), reserve_price: None, minimum_increment_cents: Some(100), minimum_increment: None, start_time: Some(RequestTimestamp::UnixSeconds(100)), end_time: Some(RequestTimestamp::UnixSeconds(200)) };
        assert!(r.try_into_command().is_err());
    }

    #[test]
    fn try_into_missing_reserve() {
        let r = CreateAuctionRequest { listing_id: Some("l".into()), seller_id: Some("s".into()), auction_type: Some("ENGLISH".into()), starting_price_cents: Some(1000), starting_price: None, reserve_price_cents: None, reserve_price: None, minimum_increment_cents: Some(100), minimum_increment: None, start_time: Some(RequestTimestamp::UnixSeconds(100)), end_time: Some(RequestTimestamp::UnixSeconds(200)) };
        assert!(r.try_into_command().is_err());
    }

    #[test]
    fn try_into_missing_increment() {
        let r = CreateAuctionRequest { listing_id: Some("l".into()), seller_id: Some("s".into()), auction_type: Some("ENGLISH".into()), starting_price_cents: Some(1000), starting_price: None, reserve_price_cents: Some(2000), reserve_price: None, minimum_increment_cents: None, minimum_increment: None, start_time: Some(RequestTimestamp::UnixSeconds(100)), end_time: Some(RequestTimestamp::UnixSeconds(200)) };
        assert!(r.try_into_command().is_err());
    }

    #[test]
    fn place_bid_req_cents() {
        let r = PlaceBidRequest { bidder_id: Some("b".into()), bid_amount_cents: Some(5000), bid_amount: None, bid_time: Some(1000) };
        assert_eq!(r.bidder_id(), Some("b"));
        assert_eq!(r.bid_amount_cents(), Some(5000));
        assert_eq!(r.bid_time(), 1000);
    }

    #[test]
    fn place_bid_req_dec() {
        let r = PlaceBidRequest { bidder_id: None, bid_amount_cents: None, bid_amount: Some(50.0), bid_time: None };
        assert_eq!(r.bid_amount_cents(), Some(5000));
        assert!(r.bid_time() > 0);
    }

    #[test]
    fn proxy_bid_req_cents() {
        let r = PlaceProxyBidRequest { bidder_id: Some("b".into()), max_bid_amount_cents: Some(10000), max_bid_amount: None, bid_time: Some(1000) };
        assert_eq!(r.max_bid_amount_cents(), Some(10000));
    }

    #[test]
    fn proxy_bid_req_dec() {
        let r = PlaceProxyBidRequest { bidder_id: None, max_bid_amount_cents: None, max_bid_amount: Some(100.0), bid_time: None };
        assert_eq!(r.max_bid_amount_cents(), Some(10000));
    }

    #[test]
    fn auction_resp_from() {
        let rec = ListingAuctionSessionRecord { id: "a".into(), listing_id: "l".into(), seller_id: "s".into(), auction_type: "ENGLISH".into(), starting_price_cents: 1000, reserve_price_cents: 2000, current_highest_bid_cents: Some(1500), minimum_increment_cents: 100, status: "ACTIVE".into(), start_time: 1700000000, end_time: 1700000300, created_at: 1700000000, updated_at: 1700000000 };
        let resp: AuctionResponse = rec.into();
        assert_eq!(resp.id, "a");
        assert!((resp.starting_price - 10.0).abs() < f64::EPSILON);
        assert!((resp.current_highest_bid.unwrap() - 15.0).abs() < f64::EPSILON);
    }

    #[test]
    fn auction_resp_no_bid() {
        let rec = ListingAuctionSessionRecord { id: "a".into(), listing_id: "l".into(), seller_id: "s".into(), auction_type: "ENGLISH".into(), starting_price_cents: 500, reserve_price_cents: 1000, current_highest_bid_cents: None, minimum_increment_cents: 50, status: "DRAFT".into(), start_time: 1700000000, end_time: 1700000300, created_at: 1700000000, updated_at: 1700000000 };
        let resp: AuctionResponse = rec.into();
        assert!(resp.current_highest_bid.is_none());
    }

    #[test]
    fn bid_resp_from() {
        let rec = BidRecord { id: "b".into(), auction_id: "a".into(), bidder_id: "u".into(), bid_amount_cents: 5000, bid_time: 1700000010, wallet_hold_id: None };
        let resp: BidResponse = rec.into();
        assert_eq!(resp.id, "b");
        assert!((resp.bid_amount - 50.0).abs() < f64::EPSILON);
    }
}

