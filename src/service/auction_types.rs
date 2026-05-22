use crate::listing_auction_session::BidError;
use crate::persistence::models::BidRecord;
use thiserror::Error;

pub(super) const CREATE_AUCTION_START_TIME_CLOCK_SKEW_SECONDS: i64 = 120;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateAuctionCommand {
    pub listing_id: String,
    pub seller_id: String,
    pub auction_type: String,
    pub starting_price_cents: i64,
    pub reserve_price_cents: i64,
    pub minimum_increment_cents: i64,
    pub start_time: i64,
    pub end_time: i64,
}

impl CreateAuctionCommand {
    pub(super) fn validate(&self, now: i64) -> Result<(), CreateAuctionError> {
        if self.listing_id.trim().is_empty() {
            return Err(CreateAuctionError::InvalidInput(
                "listing_id is required".to_string(),
            ));
        }

        if self.seller_id.trim().is_empty() {
            return Err(CreateAuctionError::InvalidInput(
                "seller_id is required".to_string(),
            ));
        }

        if self.auction_type.trim().is_empty() {
            return Err(CreateAuctionError::InvalidInput(
                "auction_type is required".to_string(),
            ));
        }

        if self.starting_price_cents <= 0 {
            return Err(CreateAuctionError::InvalidInput(
                "starting_price_cents must be greater than zero".to_string(),
            ));
        }

        if self.minimum_increment_cents <= 0 {
            return Err(CreateAuctionError::InvalidInput(
                "minimum_increment_cents must be greater than zero".to_string(),
            ));
        }

        if self.reserve_price_cents < self.starting_price_cents {
            return Err(CreateAuctionError::InvalidInput(
                "reserve_price_cents must be greater than or equal to starting_price_cents"
                    .to_string(),
            ));
        }

        if self.end_time <= self.start_time {
            return Err(CreateAuctionError::InvalidInput(
                "end_time must be after start_time".to_string(),
            ));
        }

        if self.start_time + CREATE_AUCTION_START_TIME_CLOCK_SKEW_SECONDS < now {
            return Err(CreateAuctionError::InvalidInput(
                "start_time must be greater than or equal to current time".to_string(),
            ));
        }

        if self.end_time <= now {
            return Err(CreateAuctionError::InvalidInput(
                "end_time must be in the future".to_string(),
            ));
        }

        Ok(())
    }
}

pub(super) fn initial_status(start_time: i64, now: i64) -> String {
    if start_time > now {
        "DRAFT".to_string()
    } else {
        "ACTIVE".to_string()
    }
}

pub(super) fn is_catalog_listing_biddable(status: &str) -> bool {
    status.eq_ignore_ascii_case("ACTIVE") || status.eq_ignore_ascii_case("EXTENDED")
}

pub(super) fn hold_expiration_rfc3339(end_time_unix: i64) -> String {
    let grace = crate::config::bid_hold_grace_seconds();
    let expires_at = end_time_unix.saturating_add(grace);
    chrono::DateTime::<chrono::Utc>::from_timestamp(expires_at, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| "2099-12-31T23:59:59Z".to_string())
}

#[derive(Debug, Clone)]
pub struct BidCursorPage {
    pub items: Vec<BidRecord>,
    pub next_cursor: Option<String>,
    pub size: i64,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum BidPlacementMode {
    Standard { amount_cents: i64 },
    Proxy { max_amount_cents: i64 },
}

#[derive(Debug, Clone)]
pub(super) struct BidCursor {
    pub(super) amount_cents: i64,
    pub(super) bid_time: i64,
    pub(super) id: String,
}

pub(super) fn parse_bid_cursor(value: &str) -> Result<BidCursor, String> {
    let mut parts = value.splitn(3, ':');
    let amount = parts
        .next()
        .ok_or_else(|| "invalid cursor format".to_string())?
        .parse::<i64>()
        .map_err(|_| "invalid cursor amount".to_string())?;
    let bid_time = parts
        .next()
        .ok_or_else(|| "invalid cursor format".to_string())?
        .parse::<i64>()
        .map_err(|_| "invalid cursor bid time".to_string())?;
    let id = parts
        .next()
        .ok_or_else(|| "invalid cursor format".to_string())?;

    if id.trim().is_empty() {
        return Err("invalid cursor id".to_string());
    }

    Ok(BidCursor {
        amount_cents: amount,
        bid_time,
        id: id.to_string(),
    })
}

pub(super) fn bid_cursor_from_bid(bid: &BidRecord) -> BidCursor {
    BidCursor {
        amount_cents: bid.bid_amount_cents,
        bid_time: bid.bid_time,
        id: bid.id.clone(),
    }
}

pub(super) fn cents_to_rupiah(amount_cents: i64) -> u64 {
    if amount_cents <= 0 {
        return 0;
    }
    (amount_cents as u64).div_ceil(100)
}

impl std::fmt::Display for BidCursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}", self.amount_cents, self.bid_time, self.id)
    }
}

#[derive(Debug, Error)]
pub enum CreateAuctionError {
    #[error("{0}")]
    InvalidInput(String),
    #[error("Database error: {0}")]
    DatabaseError(String),
}

#[derive(Debug, Error)]
pub enum GetListingAuctionSessionError {
    #[error("Database error: {0}")]
    DatabaseError(String),
}

#[derive(Debug, Error)]
pub enum ListListingAuctionSessionsError {
    #[error("Database error: {0}")]
    DatabaseError(String),
}

#[derive(Debug, Error)]
pub enum ListPendingClosureError {
    #[error("Database error: {0}")]
    DatabaseError(String),
}

#[derive(Debug, Error)]
pub enum CloseListingAuctionSessionError {
    #[error("ListingAuctionSession not found")]
    AuctionNotFound,
    #[error("ListingAuctionSession has not reached its end time")]
    AuctionNotEnded,
    #[error("Wallet error: {0}")]
    WalletError(String),
    #[error("Database error: {0}")]
    DatabaseError(String),
}

#[derive(Debug, Error)]
pub enum ListBidsError {
    #[error("{0}")]
    InvalidInput(String),
    #[error("Database error: {0}")]
    DatabaseError(String),
}

#[derive(Debug)]
pub enum PlaceBidError {
    AuctionNotFound,
    BidError(BidError),
    CatalogError(String),
    WalletError(String),
    DatabaseError(String),
}

impl std::fmt::Display for PlaceBidError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlaceBidError::AuctionNotFound => write!(f, "ListingAuctionSession not found"),
            PlaceBidError::BidError(e) => write!(f, "{:?}", e),
            PlaceBidError::CatalogError(e) => write!(f, "Catalog error: {}", e),
            PlaceBidError::WalletError(e) => write!(f, "Wallet error: {}", e),
            PlaceBidError::DatabaseError(e) => write!(f, "Database error: {}", e),
        }
    }
}

impl std::error::Error for PlaceBidError {}
