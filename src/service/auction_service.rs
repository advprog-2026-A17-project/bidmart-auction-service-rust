use crate::auction::BidError;
use crate::persistence::models::{
    AuctionRecord, NewAuctionRecord, NewBidRecord, NewOutboxEventRecord,
};
use crate::persistence::repositories::{AuctionRepository, BidRepository, OutboxRepository};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct AuctionService {
    auction_repo: AuctionRepository,
    bid_repo: BidRepository,
    outbox_repo: OutboxRepository,
}

impl AuctionService {
    pub fn new(
        auction_repo: AuctionRepository,
        bid_repo: BidRepository,
        outbox_repo: OutboxRepository,
    ) -> Self {
        Self {
            auction_repo,
            bid_repo,
            outbox_repo,
        }
    }

    pub async fn create_auction(
        &self,
        command: CreateAuctionCommand,
    ) -> Result<AuctionRecord, CreateAuctionError> {
        command.validate()?;

        let now = chrono::Utc::now().timestamp();
        let auction = NewAuctionRecord {
            id: uuid::Uuid::new_v4().to_string(),
            listing_id: command.listing_id,
            seller_id: command.seller_id,
            starting_price_cents: command.starting_price_cents,
            reserve_price_cents: command.reserve_price_cents,
            current_highest_bid_cents: None,
            minimum_increment_cents: command.minimum_increment_cents,
            status: initial_status(command.start_time, now),
            start_time: command.start_time,
            end_time: command.end_time,
            created_at: now,
            updated_at: now,
        };

        self.auction_repo
            .insert(&auction)
            .await
            .map_err(|error| CreateAuctionError::DatabaseError(error.to_string()))
    }

    pub async fn get_auction_by_id(
        &self,
        auction_id: &str,
    ) -> Result<Option<AuctionRecord>, GetAuctionError> {
        self.auction_repo
            .find_by_id(auction_id)
            .await
            .map_err(|error| GetAuctionError::DatabaseError(error.to_string()))
    }

    /// Place a bid on an auction
    pub async fn place_bid_and_persist(
        &self,
        auction_id: &str,
        bidder_id: &str,
        bid_amount_cents: i64,
        bid_time: i64,
    ) -> Result<(), PlaceBidError> {
        // Fetch auction
        let _auction_record = self
            .auction_repo
            .find_by_id(auction_id)
            .await
            .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?
            .ok_or(PlaceBidError::AuctionNotFound)?;

        // In a real implementation, would validate against domain logic here
        // For now, just persist the bid

        // Persist the bid
        let bid_record = NewBidRecord {
            id: uuid::Uuid::new_v4().to_string(),
            auction_id: auction_id.to_string(),
            bidder_id: bidder_id.to_string(),
            bid_amount_cents,
            bid_time,
        };
        self.bid_repo
            .insert(&bid_record)
            .await
            .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?;

        // Publish event via outbox
        self.publish_bid_placed_event(auction_id, bidder_id, bid_amount_cents)
            .await
            .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    /// Get auction with bids
    pub async fn get_auction_with_bids(
        &self,
        auction_id: &str,
    ) -> Result<Option<(String, Vec<String>)>, sqlx::Error> {
        match self.auction_repo.find_by_id(auction_id).await? {
            Some(auction) => {
                let bids = self.bid_repo.list_by_auction_id_desc(auction_id).await?;
                let bid_ids: Vec<String> = bids.iter().map(|b| b.id.clone()).collect();
                Ok(Some((auction.id, bid_ids)))
            }
            None => Ok(None),
        }
    }

    async fn publish_bid_placed_event(
        &self,
        auction_id: &str,
        bidder_id: &str,
        bid_amount_cents: i64,
    ) -> Result<(), sqlx::Error> {
        let now = chrono::Local::now().timestamp() as u64;
        let event = NewOutboxEventRecord {
            id: uuid::Uuid::new_v4().to_string(),
            aggregate_id: auction_id.to_string(),
            event_type: "BidPlaced".to_string(),
            payload: format!(
                r#"{{"auction_id":"{}","bidder_id":"{}","amount_cents":{}}}"#,
                auction_id, bidder_id, bid_amount_cents
            ),
            published: false,
            published_at: None,
            created_at: now as i64,
            updated_at: now as i64,
        };
        self.outbox_repo.insert(&event).await?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateAuctionCommand {
    pub listing_id: String,
    pub seller_id: String,
    pub starting_price_cents: i64,
    pub reserve_price_cents: i64,
    pub minimum_increment_cents: i64,
    pub start_time: i64,
    pub end_time: i64,
}

impl CreateAuctionCommand {
    fn validate(&self) -> Result<(), CreateAuctionError> {
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

        Ok(())
    }
}

fn initial_status(start_time: i64, now: i64) -> String {
    if start_time > now {
        "SCHEDULED".to_string()
    } else {
        "ACTIVE".to_string()
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
pub enum GetAuctionError {
    #[error("Database error: {0}")]
    DatabaseError(String),
}

#[derive(Debug)]
pub enum PlaceBidError {
    AuctionNotFound,
    BidError(BidError),
    DatabaseError(String),
}

impl std::fmt::Display for PlaceBidError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlaceBidError::AuctionNotFound => write!(f, "Auction not found"),
            PlaceBidError::BidError(e) => write!(f, "{:?}", e),
            PlaceBidError::DatabaseError(e) => write!(f, "Database error: {}", e),
        }
    }
}

impl std::error::Error for PlaceBidError {}
