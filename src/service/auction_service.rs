use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::auction::{Auction, BidError, Money, UnixSeconds, UserId};
use crate::client::{CatalogClient, HoldFundsRequest, ListingSummary, WalletClient};
use crate::persistence::models::{
    AuctionRecord, BidRecord, NewAuctionRecord, NewBidRecord, NewOutboxEventRecord,
};
use crate::persistence::repositories::{AuctionRepository, BidRepository, OutboxRepository};
use thiserror::Error;
use tokio::sync::OwnedMutexGuard;

#[derive(Clone)]
pub struct AuctionService {
    auction_repo: AuctionRepository,
    bid_repo: BidRepository,
    outbox_repo: OutboxRepository,
    wallet_client: Option<Arc<dyn WalletClient>>,
    catalog_client: Option<Arc<dyn CatalogClient>>,
    bid_locks: Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
}

impl std::fmt::Debug for AuctionService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuctionService")
            .field("auction_repo", &"<AuctionRepository>")
            .field("bid_repo", &"<BidRepository>")
            .field("outbox_repo", &"<OutboxRepository>")
            .field("wallet_client", &"<WalletClient>")
            .field("catalog_client", &"<CatalogClient>")
            .field("bid_locks", &"<BidLocks>")
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
            bid_locks: Arc::new(Mutex::new(HashMap::new())),
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
        let Some(listing) = self
            .require_active_listing(&command.listing_id)
            .await
            .map_err(CreateAuctionError::InvalidInput)?
        else {
            return Ok(());
        };

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

    pub async fn list_pending_closure(
        &self,
    ) -> Result<Vec<AuctionRecord>, ListPendingClosureError> {
        let now = chrono::Utc::now().timestamp();
        self.auction_repo
            .list_pending_closure(now)
            .await
            .map_err(|error| ListPendingClosureError::DatabaseError(error.to_string()))
    }

    pub async fn close_auction(&self, auction_id: &str) -> Result<AuctionRecord, CloseAuctionError> {
        let auction = self
            .auction_repo
            .find_by_id(auction_id)
            .await
            .map_err(|error| CloseAuctionError::DatabaseError(error.to_string()))?
            .ok_or(CloseAuctionError::AuctionNotFound)?;

        let now = chrono::Utc::now().timestamp();
        if auction.end_time > now {
            return Err(CloseAuctionError::AuctionNotEnded);
        }

        if auction.status == "WON" || auction.status == "UNSOLD" {
            return Ok(auction);
        }

        let bids = self
            .bid_repo
            .list_by_auction_id_desc(auction_id)
            .await
            .map_err(|error| CloseAuctionError::DatabaseError(error.to_string()))?;
        let winning_bid = bids.first();
        let highest_bid_cents = winning_bid
            .map(|bid| bid.bid_amount_cents)
            .or(auction.current_highest_bid_cents);
        let status = if highest_bid_cents
            .map(|amount| amount >= auction.reserve_price_cents)
            .unwrap_or(false)
        {
            "WON"
        } else {
            "UNSOLD"
        };

        let updated = self
            .auction_repo
            .update_lifecycle_status(auction_id, status, highest_bid_cents, now)
            .await
            .map_err(|error| CloseAuctionError::DatabaseError(error.to_string()))?;

        if let Some(wallet_client) = &self.wallet_client {
            if status == "WON" {
                if let Some(hold_id) = winning_bid.and_then(|bid| bid.wallet_hold_id.as_deref()) {
                    wallet_client
                        .convert_hold_to_payment(hold_id)
                        .await
                        .map_err(|error| CloseAuctionError::WalletError(error.to_string()))?;
                }
            } else {
                for bid in &bids {
                    if let Some(hold_id) = &bid.wallet_hold_id {
                        wallet_client
                            .release_hold(hold_id)
                            .await
                            .map_err(|error| CloseAuctionError::WalletError(error.to_string()))?;
                    }
                }
            }
        }

        self.publish_auction_ended_event(&updated, winning_bid)
            .await
            .map_err(|error| CloseAuctionError::DatabaseError(error.to_string()))?;

        Ok(updated)
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
        let _bid_guard = self.auction_bid_guard(auction_id).await;

        if let Some(existing_bid) = self
            .bid_repo
            .find_matching_bid(auction_id, bidder_id, bid_amount_cents, bid_time)
            .await
            .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?
        {
            return Ok(existing_bid);
        }

        // Fetch auction
        let auction_record = self
            .auction_repo
            .find_by_id(auction_id)
            .await
            .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?
            .ok_or(PlaceBidError::AuctionNotFound)?;
        self.validate_listing_for_bid(&auction_record.listing_id)
            .await?;

        let previous_winning_bid = self
            .bid_repo
            .find_winning_bid(auction_id)
            .await
            .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?;

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
        let hold_id = if let Some(wallet_client) = &self.wallet_client {
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
            .insert_with_wallet_hold(&bid_record, hold_id.as_deref())
            .await
            .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?;

        if let (Some(wallet_client), Some(previous_hold_id)) = (
            &self.wallet_client,
            previous_winning_bid.and_then(|bid| bid.wallet_hold_id),
        ) {
            wallet_client
                .release_hold(&previous_hold_id)
                .await
                .map_err(|error| PlaceBidError::WalletError(error.to_string()))?;
        }

        // Update auction if it was extended or if current highest bid changed
        let new_highest_cents = bid_result.new_highest.amount.cents() as i64;
        let mut updated_record = auction_record.clone();
        updated_record.current_highest_bid_cents = Some(new_highest_cents);
        updated_record.end_time = bid_result.new_end_at.value() as i64;
        updated_record.status = self.status_to_string(auction.status());
        updated_record.updated_at = bid_time;

        let persisted_update = sqlx::query_as::<_, AuctionRecord>(
            "UPDATE auctions \
             SET current_highest_bid_cents = ?, end_time = ?, status = ?, updated_at = ? \
             WHERE id = ? AND (current_highest_bid_cents IS NULL OR current_highest_bid_cents < ?) \
             RETURNING id, listing_id, seller_id, starting_price_cents, reserve_price_cents, \
             current_highest_bid_cents, minimum_increment_cents, status, start_time, end_time, created_at, updated_at",
        )
        .bind(updated_record.current_highest_bid_cents)
        .bind(updated_record.end_time)
        .bind(&updated_record.status)
        .bind(updated_record.updated_at)
        .bind(auction_id)
        .bind(new_highest_cents)
        .fetch_optional(&self.auction_repo.pool)
        .await
        .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?;
        updated_record = match persisted_update {
            Some(record) => record,
            None => self
                .auction_repo
                .find_by_id(auction_id)
                .await
                .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?
                .unwrap_or(updated_record),
        };

        // Publish event via outbox
        self.publish_bid_placed_event(&updated_record, &inserted_bid)
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

    async fn validate_listing_for_bid(&self, listing_id: &str) -> Result<(), PlaceBidError> {
        self.require_active_listing(listing_id)
            .await
            .map_err(PlaceBidError::CatalogError)?;

        Ok(())
    }

    async fn require_active_listing(
        &self,
        listing_id: &str,
    ) -> Result<Option<ListingSummary>, String> {
        let Some(catalog_client) = &self.catalog_client else {
            return Ok(None);
        };

        let listing = catalog_client
            .get_listing_summary(listing_id)
            .await
            .map_err(|error| error.to_string())?;

        if !listing.status.eq_ignore_ascii_case("ACTIVE") {
            return Err("Listing is not active".to_string());
        }

        Ok(Some(listing))
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
        auction: &AuctionRecord,
        bid: &BidRecord,
    ) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().timestamp();
        let payload = serde_json::json!({
            "auction_id": auction.id,
            "listing_id": auction.listing_id,
            "bid_id": bid.id,
            "bidder_id": bid.bidder_id,
            "amount_cents": bid.bid_amount_cents,
            "current_price_cents": bid.bid_amount_cents,
            "bid_time": bid.bid_time,
            "placed_at": now
        })
        .to_string();
        let event = NewOutboxEventRecord {
            id: uuid::Uuid::new_v4().to_string(),
            aggregate_id: auction.id.clone(),
            event_type: "BidPlaced".to_string(),
            payload,
            published: false,
            published_at: None,
            created_at: now,
            updated_at: now,
        };
        self.outbox_repo.insert(&event).await?;
        Ok(())
    }

    async fn publish_auction_ended_event(
        &self,
        auction: &AuctionRecord,
        winning_bid: Option<&BidRecord>,
    ) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().timestamp();
        let payload = serde_json::json!({
            "auction_id": auction.id,
            "listing_id": auction.listing_id,
            "seller_id": auction.seller_id,
            "status": auction.status,
            "winner_bidder_id": winning_bid.map(|bid| bid.bidder_id.as_str()),
            "winning_bid_cents": winning_bid.map(|bid| bid.bid_amount_cents),
            "ended_at": now
        })
        .to_string();
        let event = NewOutboxEventRecord {
            id: uuid::Uuid::new_v4().to_string(),
            aggregate_id: auction.id.clone(),
            event_type: "AuctionEnded".to_string(),
            payload,
            published: false,
            published_at: None,
            created_at: now,
            updated_at: now,
        };
        self.outbox_repo.insert(&event).await?;
        Ok(())
    }

    async fn auction_bid_guard(&self, auction_id: &str) -> OwnedMutexGuard<()> {
        let lock = {
            let mut bid_locks = self.bid_locks.lock().expect("bid lock map poisoned");
            bid_locks
                .entry(auction_id.to_string())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };

        lock.lock_owned().await
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
pub enum ListPendingClosureError {
    #[error("Database error: {0}")]
    DatabaseError(String),
}

#[derive(Debug, Error)]
pub enum CloseAuctionError {
    #[error("Auction not found")]
    AuctionNotFound,
    #[error("Auction has not reached its end time")]
    AuctionNotEnded,
    #[error("Wallet error: {0}")]
    WalletError(String),
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
    CatalogError(String),
    WalletError(String),
    DatabaseError(String),
}

impl std::fmt::Display for PlaceBidError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlaceBidError::AuctionNotFound => write!(f, "Auction not found"),
            PlaceBidError::BidError(e) => write!(f, "{:?}", e),
            PlaceBidError::CatalogError(e) => write!(f, "Catalog error: {}", e),
            PlaceBidError::WalletError(e) => write!(f, "Wallet error: {}", e),
            PlaceBidError::DatabaseError(e) => write!(f, "Database error: {}", e),
        }
    }
}

impl std::error::Error for PlaceBidError {}
