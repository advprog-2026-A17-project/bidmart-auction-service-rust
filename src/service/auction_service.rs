use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::listing_auction_session::{ListingAuctionSession, BidError, Money, UnixSeconds, UserId};
use crate::client::{CatalogClient, HoldFundsRequest, ListingSummary, WalletClient};
use crate::persistence::models::{
    ListingAuctionSessionRecord, BidRecord, NewListingAuctionSessionRecord, NewBidRecord, NewOutboxEventRecord,
};
use crate::persistence::repositories::{ListingAuctionSessionRepository, BidRepository, OutboxRepository, ProxyBidRepository};
use crate::service::auction_strategy::{AuctionType, resolve_strategy};
use crate::service::bid_policies::{
    AmountBidPolicy, IdentityBidPolicy, TimeBidPolicy, WalletBidPolicy,
};
use thiserror::Error;
use tokio::sync::OwnedMutexGuard;

const CREATE_AUCTION_START_TIME_CLOCK_SKEW_SECONDS: i64 = 120;

#[derive(Clone)]
pub struct AuctionService {
    listing_auction_session_repo: ListingAuctionSessionRepository,
    bid_repo: BidRepository,
    outbox_repo: OutboxRepository,
    proxy_bid_repo: ProxyBidRepository,
    wallet_client: Option<Arc<dyn WalletClient>>,
    catalog_client: Option<Arc<dyn CatalogClient>>,
    bid_locks: Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
}

impl std::fmt::Debug for AuctionService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuctionService")
            .field("listing_auction_session_repo", &"<ListingAuctionSessionRepository>")
            .field("bid_repo", &"<BidRepository>")
            .field("outbox_repo", &"<OutboxRepository>")
            .field("proxy_bid_repo", &"<ProxyBidRepository>")
            .field("wallet_client", &"<WalletClient>")
            .field("catalog_client", &"<CatalogClient>")
            .field("bid_locks", &"<BidLocks>")
            .finish()
    }
}

impl AuctionService {
    pub fn new(
        listing_auction_session_repo: ListingAuctionSessionRepository,
        bid_repo: BidRepository,
        outbox_repo: OutboxRepository,
    ) -> Self {
        Self::new_with_clients(listing_auction_session_repo, bid_repo, outbox_repo, None, None)
    }

    pub fn new_with_wallet(
        listing_auction_session_repo: ListingAuctionSessionRepository,
        bid_repo: BidRepository,
        outbox_repo: OutboxRepository,
        wallet_client: Arc<dyn WalletClient>,
    ) -> Self {
        Self::new_with_clients(
            listing_auction_session_repo,
            bid_repo,
            outbox_repo,
            Some(wallet_client),
            None,
        )
    }

    pub fn new_with_catalog(
        listing_auction_session_repo: ListingAuctionSessionRepository,
        bid_repo: BidRepository,
        outbox_repo: OutboxRepository,
        catalog_client: Arc<dyn CatalogClient>,
    ) -> Self {
        Self::new_with_clients(
            listing_auction_session_repo,
            bid_repo,
            outbox_repo,
            None,
            Some(catalog_client),
        )
    }

    pub fn new_with_clients(
        listing_auction_session_repo: ListingAuctionSessionRepository,
        bid_repo: BidRepository,
        outbox_repo: OutboxRepository,
        wallet_client: Option<Arc<dyn WalletClient>>,
        catalog_client: Option<Arc<dyn CatalogClient>>,
    ) -> Self {
        Self {
            proxy_bid_repo: ProxyBidRepository::new(listing_auction_session_repo.pool.clone()),
            listing_auction_session_repo,
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
    ) -> Result<ListingAuctionSessionRecord, CreateAuctionError> {
        let now = chrono::Utc::now().timestamp();
        command.validate(now)?;
        let auction_type = AuctionType::from_input(Some(&command.auction_type))
            .map_err(CreateAuctionError::InvalidInput)?;
        resolve_strategy(auction_type).validate_create_request()?;
        self.validate_listing_for_auction(&command).await?;

        if let Some(existing) = self
            .listing_auction_session_repo
            .find_by_listing_id(&command.listing_id)
            .await
            .map_err(|error| CreateAuctionError::DatabaseError(error.to_string()))?
        {
            return Ok(existing);
        }

        let listing_id = command.listing_id.clone();
        let auction = NewListingAuctionSessionRecord {
            id: listing_id.clone(),
            listing_id,
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

        let inserted = self
            .listing_auction_session_repo
            .insert(&auction)
            .await
            .map_err(|error| CreateAuctionError::DatabaseError(error.to_string()))?;
        self.publish_auction_created_event(&inserted)
            .await
            .map_err(|error| CreateAuctionError::DatabaseError(error.to_string()))?;
        Ok(inserted)
    }

    async fn validate_listing_for_auction(
        &self,
        command: &CreateAuctionCommand,
    ) -> Result<(), CreateAuctionError> {
        let listing = self
            .require_active_listing(&command.listing_id)
            .await
            .map_err(CreateAuctionError::InvalidInput)?;

        if !listing.seller_id.is_empty() && listing.seller_id != command.seller_id {
            return Err(CreateAuctionError::InvalidInput(
                "Listing seller does not match auction seller".to_string(),
            ));
        }

        Ok(())
    }

    pub async fn get_auction_by_id(
        &self,
        auction_id: &str,
    ) -> Result<Option<ListingAuctionSessionRecord>, GetListingAuctionSessionError> {
        let found = self
            .listing_auction_session_repo
            .find_by_id(auction_id)
            .await
            .map_err(|error| GetListingAuctionSessionError::DatabaseError(error.to_string()))?;
        if found.is_some() {
            return Ok(found);
        }
        self.listing_auction_session_repo
            .find_by_listing_id(auction_id)
            .await
            .map_err(|error| GetListingAuctionSessionError::DatabaseError(error.to_string()))
    }

    pub async fn list_auctions(&self) -> Result<Vec<ListingAuctionSessionRecord>, ListListingAuctionSessionsError> {
        self.listing_auction_session_repo
            .list_all()
            .await
            .map_err(|error| ListListingAuctionSessionsError::DatabaseError(error.to_string()))
    }

    pub async fn list_pending_closure(
        &self,
    ) -> Result<Vec<ListingAuctionSessionRecord>, ListPendingClosureError> {
        let now = chrono::Utc::now().timestamp();
        self.listing_auction_session_repo
            .list_pending_closure(now)
            .await
            .map_err(|error| ListPendingClosureError::DatabaseError(error.to_string()))
    }

    /// Optimized for background queue processing using Pessimistic Lock
    pub async fn process_one_pending_closure(
        &self,
    ) -> Result<Option<ListingAuctionSessionRecord>, CloseListingAuctionSessionError> {
        let now = chrono::Utc::now().timestamp();
        let mut tx = self
            .listing_auction_session_repo
            .pool
            .begin()
            .await
            .map_err(|error| CloseListingAuctionSessionError::DatabaseError(error.to_string()))?;

        let pending_auction = self
            .listing_auction_session_repo
            .pop_pending_closure(now, &mut tx)
            .await
            .map_err(|error| CloseListingAuctionSessionError::DatabaseError(error.to_string()))?;

        let Some(auction) = pending_auction else {
            tx.rollback().await.map_err(|e| CloseListingAuctionSessionError::DatabaseError(e.to_string()))?;
            return Ok(None);
        };

        let (updated, bids) = self.execute_auction_closure(auction, &mut tx, now).await?;
        
        tx.commit().await.map_err(|e| CloseListingAuctionSessionError::DatabaseError(e.to_string()))?;
        
        self.execute_post_closure_side_effects(&updated, &bids).await?;

        Ok(Some(updated))
    }

    /// Manual close endpoint secured with row-level transaction
    pub async fn close_auction(
        &self,
        auction_id: &str,
    ) -> Result<ListingAuctionSessionRecord, CloseListingAuctionSessionError> {
        let now = chrono::Utc::now().timestamp();
        let mut tx = self
            .listing_auction_session_repo
            .pool
            .begin()
            .await
            .map_err(|error| CloseListingAuctionSessionError::DatabaseError(error.to_string()))?;

        let auction = self
            .listing_auction_session_repo
            .find_by_id_for_update(auction_id, &mut tx)
            .await
            .map_err(|error| CloseListingAuctionSessionError::DatabaseError(error.to_string()))?
            .ok_or(CloseListingAuctionSessionError::AuctionNotFound)?;

        if auction.end_time > now {
            tx.rollback().await.map_err(|e| CloseListingAuctionSessionError::DatabaseError(e.to_string()))?;
            return Err(CloseListingAuctionSessionError::AuctionNotEnded);
        }

        if auction.status == "WON" || auction.status == "UNSOLD" {
            tx.rollback().await.map_err(|e| CloseListingAuctionSessionError::DatabaseError(e.to_string()))?;
            return Ok(auction);
        }

        let (updated, bids) = self.execute_auction_closure(auction, &mut tx, now).await?;
        
        tx.commit().await.map_err(|e| CloseListingAuctionSessionError::DatabaseError(e.to_string()))?;

        self.execute_post_closure_side_effects(&updated, &bids).await?;

        Ok(updated)
    }

    async fn execute_auction_closure(
        &self,
        auction: ListingAuctionSessionRecord,
        tx: &mut sqlx::Transaction<'_, sqlx::Any>,
        now: i64,
    ) -> Result<(ListingAuctionSessionRecord, Vec<BidRecord>), CloseListingAuctionSessionError> {
        let bids = self
            .bid_repo
            .list_by_auction_id_desc_with_tx(&auction.id, tx)
            .await
            .map_err(|error| CloseListingAuctionSessionError::DatabaseError(error.to_string()))?;
        
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
            .listing_auction_session_repo
            .update_lifecycle_status_with_tx(&auction.id, status, highest_bid_cents, now, tx)
            .await
            .map_err(|error| CloseListingAuctionSessionError::DatabaseError(error.to_string()))?;

        self.publish_auction_ended_event_with_tx(&updated, winning_bid, tx)
            .await
            .map_err(|error| CloseListingAuctionSessionError::DatabaseError(error.to_string()))?;

        Ok((updated, bids))
    }

    async fn execute_post_closure_side_effects(
        &self,
        auction: &ListingAuctionSessionRecord,
        bids: &[BidRecord],
    ) -> Result<(), CloseListingAuctionSessionError> {
        let winning_bid = bids.first();

        if let Some(wallet_client) = &self.wallet_client {
            if auction.status == "WON" {
                let winner = winning_bid.ok_or_else(|| {
                    CloseListingAuctionSessionError::WalletError(
                        "WON auction missing winning bid".to_string(),
                    )
                })?;
                if let Some(hold_id) = winner.wallet_hold_id.as_deref() {
                    wallet_client
                        .convert_hold_to_payment(hold_id)
                        .await
                        .map_err(|error| CloseListingAuctionSessionError::WalletError(error.to_string()))?;
                }
            } else {
                for bid in bids {
                    if let Some(hold_id) = &bid.wallet_hold_id {
                        wallet_client
                            .release_hold(hold_id)
                            .await
                            .map_err(|error| CloseListingAuctionSessionError::WalletError(error.to_string()))?;
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn list_bids(&self, auction_id: &str) -> Result<Vec<BidRecord>, ListBidsError> {
        let canonical_auction_id = self
            .listing_auction_session_repo
            .find_by_id(auction_id)
            .await
            .map_err(|error| ListBidsError::DatabaseError(error.to_string()))?
            .or(
                self.listing_auction_session_repo
                    .find_by_listing_id(auction_id)
                    .await
                    .map_err(|error| ListBidsError::DatabaseError(error.to_string()))?,
            )
            .map(|record| record.id)
            .unwrap_or_else(|| auction_id.to_string());
        self.bid_repo
            .list_by_auction_id_desc(&canonical_auction_id)
            .await
            .map_err(|error| ListBidsError::DatabaseError(error.to_string()))
    }

    pub async fn list_bids_with_cursor(
        &self,
        auction_id: &str,
        cursor: Option<&str>,
        limit: Option<i64>,
    ) -> Result<BidCursorPage, ListBidsError> {
        let canonical_auction_id = self
            .listing_auction_session_repo
            .find_by_id(auction_id)
            .await
            .map_err(|error| ListBidsError::DatabaseError(error.to_string()))?
            .or(
                self.listing_auction_session_repo
                    .find_by_listing_id(auction_id)
                    .await
                    .map_err(|error| ListBidsError::DatabaseError(error.to_string()))?,
            )
            .map(|record| record.id)
            .unwrap_or_else(|| auction_id.to_string());
        let sanitized_limit = limit.unwrap_or(20).clamp(1, 100);
        let parsed_cursor = match cursor {
            Some(value) => Some(parse_bid_cursor(value).map_err(ListBidsError::InvalidInput)?),
            None => None,
        };

        let mut bids = self
            .bid_repo
            .list_by_auction_cursor(
                &canonical_auction_id,
                parsed_cursor.map(|cursor| (cursor.amount_cents, cursor.bid_time, cursor.id)),
                sanitized_limit + 1,
            )
            .await
            .map_err(|error| ListBidsError::DatabaseError(error.to_string()))?;

        let has_more = bids.len() as i64 > sanitized_limit;
        if has_more {
            bids.truncate(sanitized_limit as usize);
        }

        let next_cursor = if has_more {
            bids.last().map(|bid| bid_cursor_from_bid(bid).to_string())
        } else {
            None
        };

        Ok(BidCursorPage {
            items: bids,
            next_cursor,
            size: sanitized_limit,
        })
    }

    pub async fn place_bid_and_persist(
        &self,
        auction_id: &str,
        bidder_id: &str,
        bid_amount_cents: i64,
        bid_time: i64,
    ) -> Result<BidRecord, PlaceBidError> {
        self.place_bid_internal(
            auction_id,
            bidder_id,
            bid_time,
            BidPlacementMode::Standard {
                amount_cents: bid_amount_cents,
            },
        )
        .await
    }

    pub async fn place_proxy_bid_and_persist(
        &self,
        auction_id: &str,
        bidder_id: &str,
        max_bid_amount_cents: i64,
        bid_time: i64,
    ) -> Result<BidRecord, PlaceBidError> {
        self.place_bid_internal(
            auction_id,
            bidder_id,
            bid_time,
            BidPlacementMode::Proxy {
                max_amount_cents: max_bid_amount_cents,
            },
        )
        .await
    }

    async fn place_bid_internal(
        &self,
        auction_id: &str,
        bidder_id: &str,
        bid_time: i64,
        mode: BidPlacementMode,
    ) -> Result<BidRecord, PlaceBidError> {
        let resolved_record = self
            .listing_auction_session_repo
            .find_by_id(auction_id)
            .await
            .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?
            .or(
                self.listing_auction_session_repo
                    .find_by_listing_id(auction_id)
                    .await
                    .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?,
            )
            .ok_or(PlaceBidError::AuctionNotFound)?;
        let canonical_auction_id = resolved_record.id.clone();
        let _bid_guard = self.auction_bid_guard(&canonical_auction_id).await;

        if let BidPlacementMode::Standard { amount_cents } = mode {
            if let Some(existing_bid) = self
                .bid_repo
                .find_matching_bid(&canonical_auction_id, bidder_id, amount_cents, bid_time)
                .await
                .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?
            {
                return Ok(existing_bid);
            }
        }

        let auction_record = self
            .listing_auction_session_repo
            .find_by_id(&canonical_auction_id)
            .await
            .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?
            .ok_or(PlaceBidError::AuctionNotFound)?;
        self.validate_listing_for_bid(&auction_record.listing_id)
            .await?;

        let previous_winning_bid = self
            .bid_repo
            .find_winning_bid(&canonical_auction_id)
            .await
            .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?;

        TimeBidPolicy::validate(&auction_record, bid_time).map_err(PlaceBidError::BidError)?;
        IdentityBidPolicy::validate(&auction_record, bidder_id, previous_winning_bid.as_ref())
            .map_err(PlaceBidError::BidError)?;

        let mut auction = self
            .record_to_domain_with_bid(&auction_record)
            .await
            .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?;

        let bid_result = match mode {
            BidPlacementMode::Standard { amount_cents } => {
                AmountBidPolicy::validate(
                    &auction_record,
                    amount_cents,
                    previous_winning_bid.as_ref(),
                )
                .map_err(PlaceBidError::BidError)?;
                auction
                    .place_bid(
                        UserId::new(bidder_id),
                        Money::from_cents(amount_cents as u64),
                        UnixSeconds::new(bid_time as u64),
                    )
                    .map_err(PlaceBidError::BidError)?
            }
            BidPlacementMode::Proxy { max_amount_cents } => auction
                .place_proxy_bid(
                    UserId::new(bidder_id),
                    Money::from_cents(max_amount_cents as u64),
                    UnixSeconds::new(bid_time as u64),
                )
                .map_err(PlaceBidError::BidError)?,
        };
        let accepted_bid_amount_cents = bid_result.new_highest.amount.cents() as i64;
        WalletBidPolicy::validate(accepted_bid_amount_cents).map_err(PlaceBidError::BidError)?;

        if let BidPlacementMode::Proxy { max_amount_cents } = mode {
            self.proxy_bid_repo
                .upsert_max(&canonical_auction_id, bidder_id, max_amount_cents, bid_time)
                .await
                .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?;
        }

        let bid_id = uuid::Uuid::new_v4().to_string();

        if let Some(previous_bid) = previous_winning_bid.as_ref() {
            if previous_bid.bidder_id != bidder_id {
                if let (Some(wallet_client), Some(previous_hold_id)) = (
                    &self.wallet_client,
                    previous_bid.wallet_hold_id.as_deref(),
                ) {
                    wallet_client
                        .release_hold(previous_hold_id)
                        .await
                        .map_err(|error| PlaceBidError::WalletError(error.to_string()))?;
                }
            }
        }

        let hold_id = if let Some(wallet_client) = &self.wallet_client {
            let hold_request = HoldFundsRequest {
                user_id: bidder_id.to_string(),
                role: Some("BUYER".to_string()),
                hold_id: uuid::Uuid::new_v4().to_string(),
                auction_id: canonical_auction_id.clone(),
                bid_id: bid_id.clone(),
                amount: cents_to_rupiah(accepted_bid_amount_cents),
                expires_at: hold_expiration_rfc3339(bid_result.new_end_at.value() as i64),
            };

            let hold_response = wallet_client
                .hold_funds(hold_request)
                .await
                .map_err(|e| PlaceBidError::WalletError(e.to_string()))?;

            Some(hold_response.id)
        } else {
            None
        };

        let bid_record = NewBidRecord {
            id: bid_id,
            auction_id: canonical_auction_id.clone(),
            bidder_id: bidder_id.to_string(),
            bid_amount_cents: accepted_bid_amount_cents,
            bid_time,
        };
        let inserted_bid = match self
            .bid_repo
            .insert_with_wallet_hold(&bid_record, hold_id.as_deref())
            .await
        {
            Ok(inserted_bid) => inserted_bid,
            Err(error) => {
                if let (Some(wallet_client), Some(hold_id)) =
                    (&self.wallet_client, hold_id.as_deref())
                {
                    let _ = wallet_client.release_hold(hold_id).await;
                }
                return Err(PlaceBidError::DatabaseError(error.to_string()));
            }
        };

        if let Some(previous_bid) = previous_winning_bid.as_ref() {
            if previous_bid.bidder_id != bidder_id {
                self.publish_outbid_event(
                    &auction_record,
                    previous_bid,
                    accepted_bid_amount_cents,
                )
                .await
                .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?;
            }
        }

        let new_highest_cents = bid_result.new_highest.amount.cents() as i64;
        let mut updated_record = auction_record.clone();
        updated_record.current_highest_bid_cents = Some(new_highest_cents);
        updated_record.end_time = bid_result.new_end_at.value() as i64;
        updated_record.status = self.status_to_string(auction.status());
        updated_record.updated_at = bid_time;

        let persisted_update = sqlx::query_as::<_, ListingAuctionSessionRecord>(
            "UPDATE listings \
             SET current_highest_bid_cents = $1, end_time = $2, lifecycle_state = $3, updated_at = $4 \
             WHERE id = $5 AND (current_highest_bid_cents IS NULL OR current_highest_bid_cents < $6) \
             RETURNING id, listing_id, seller_id, auction_type, starting_price_cents, reserve_price_cents, \
             current_highest_bid_cents, minimum_increment_cents, lifecycle_state AS status, start_time, end_time, created_at, updated_at",
        )
        .bind(updated_record.current_highest_bid_cents)
        .bind(updated_record.end_time)
        .bind(&updated_record.status)
        .bind(updated_record.updated_at)
        .bind(&canonical_auction_id)
        .bind(new_highest_cents)
        .fetch_optional(&self.listing_auction_session_repo.pool)
        .await
        .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?;
        updated_record = match persisted_update {
            Some(record) => record,
            None => self
                .listing_auction_session_repo
                .find_by_id(&canonical_auction_id)
                .await
                .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?
                .unwrap_or(updated_record),
        };

        self.resolve_proxy_auto_bid(
                &canonical_auction_id,
                &updated_record,
                bid_time,
                previous_winning_bid.as_ref(),
            )
            .await?;

        self.publish_bid_placed_event(&updated_record, &inserted_bid)
            .await
            .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?;

        Ok(inserted_bid)
    }

    async fn resolve_proxy_auto_bid(
        &self,
        auction_id: &str,
        current_listing: &ListingAuctionSessionRecord,
        bid_time: i64,
        _prior_winner: Option<&BidRecord>,
    ) -> Result<(), PlaceBidError> {
        let proxy_rows = self
            .proxy_bid_repo
            .list_by_auction(auction_id)
            .await
            .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?;
        if proxy_rows.is_empty() {
            return Ok(());
        }

        let current_winning = self
            .bid_repo
            .find_winning_bid(auction_id)
            .await
            .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?;
        let Some(current_winning) = current_winning else {
            return Ok(());
        };

        let top_proxy = &proxy_rows[0];
        let increment = current_listing.minimum_increment_cents;
        let competitor_cap = proxy_rows
            .iter()
            .filter(|p| p.bidder_id != top_proxy.bidder_id)
            .map(|p| p.max_bid_amount_cents)
            .max()
            .unwrap_or(current_winning.bid_amount_cents);
        let target_amount =
            std::cmp::min(top_proxy.max_bid_amount_cents, competitor_cap + increment);

        if target_amount <= current_winning.bid_amount_cents {
            return Ok(());
        }

        let minimum_to_beat = current_winning.bid_amount_cents + increment;
        if top_proxy.max_bid_amount_cents < minimum_to_beat {
            return Ok(());
        }

        let auto_bid_time = bid_time.saturating_add(1);
        let (updated_status, updated_end_time) = if top_proxy.bidder_id == current_winning.bidder_id {
            let mut end_time = current_listing.end_time;
            if end_time.saturating_sub(auto_bid_time) <= 120 {
                end_time = auto_bid_time.saturating_add(120);
            }
            let status = if end_time > current_listing.end_time {
                "EXTENDED".to_string()
            } else {
                current_listing.status.clone()
            };
            (status, end_time)
        } else {
            let mut auction = self
                .record_to_domain_with_bid(current_listing)
                .await
                .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?;
            let accepted = auction
                .place_bid(
                    UserId::new(&top_proxy.bidder_id),
                    Money::from_cents(target_amount as u64),
                    UnixSeconds::new(auto_bid_time as u64),
                )
                .map_err(PlaceBidError::BidError)?;
            (
                self.status_to_string(auction.status()),
                accepted.new_end_at.value() as i64,
            )
        };

        let bid_id = uuid::Uuid::new_v4().to_string();
        let hold_id = if let Some(wallet_client) = &self.wallet_client {
            let hold_request = HoldFundsRequest {
                user_id: top_proxy.bidder_id.clone(),
                role: Some("BUYER".to_string()),
                hold_id: uuid::Uuid::new_v4().to_string(),
                auction_id: auction_id.to_string(),
                bid_id: bid_id.clone(),
                amount: cents_to_rupiah(target_amount),
                expires_at: hold_expiration_rfc3339(updated_end_time),
            };
            let hold_response = wallet_client
                .hold_funds(hold_request)
                .await
                .map_err(|e| PlaceBidError::WalletError(e.to_string()))?;
            Some(hold_response.id)
        } else {
            None
        };

        let bid_record = NewBidRecord {
            id: bid_id,
            auction_id: auction_id.to_string(),
            bidder_id: top_proxy.bidder_id.clone(),
            bid_amount_cents: target_amount,
            bid_time: auto_bid_time,
        };
        let inserted = match self
            .bid_repo
            .insert_with_wallet_hold(&bid_record, hold_id.as_deref())
            .await
        {
            Ok(inserted) => inserted,
            Err(error) => {
                if let (Some(wallet_client), Some(hold_id)) =
                    (&self.wallet_client, hold_id.as_deref())
                {
                    let _ = wallet_client.release_hold(hold_id).await;
                }
                return Err(PlaceBidError::DatabaseError(error.to_string()));
            }
        };

        if let (Some(wallet_client), Some(previous_hold_id)) =
            (&self.wallet_client, current_winning.wallet_hold_id.as_deref())
        {
            wallet_client
                .release_hold(previous_hold_id)
                .await
                .map_err(|error| PlaceBidError::WalletError(error.to_string()))?;
        }

        if current_winning.bidder_id != top_proxy.bidder_id {
            self.publish_outbid_event(current_listing, &current_winning, target_amount)
                .await
                .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?;
        }

        let updated_record = sqlx::query_as::<_, ListingAuctionSessionRecord>(
            "UPDATE listings \
             SET current_highest_bid_cents = $1, end_time = $2, lifecycle_state = $3, updated_at = $4 \
             WHERE id = $5 \
             RETURNING id, listing_id, seller_id, auction_type, starting_price_cents, reserve_price_cents, \
             current_highest_bid_cents, minimum_increment_cents, lifecycle_state AS status, start_time, end_time, created_at, updated_at",
        )
        .bind(Some(target_amount))
        .bind(updated_end_time)
        .bind(&updated_status)
        .bind(auto_bid_time)
        .bind(auction_id)
        .fetch_one(&self.listing_auction_session_repo.pool)
        .await
        .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?;

        self.publish_bid_placed_event(&updated_record, &inserted)
            .await
            .map_err(|e| PlaceBidError::DatabaseError(e.to_string()))?;

        let _ = inserted;
        Ok(())
    }

    async fn record_to_domain_with_bid(
        &self,
        record: &ListingAuctionSessionRecord,
    ) -> Result<ListingAuctionSession, sqlx::Error> {
        let status = match record.status.as_str() {
            "DRAFT" | "SCHEDULED" => crate::listing_auction_session::ListingAuctionSessionStatus::Draft,
            "ACTIVE" => crate::listing_auction_session::ListingAuctionSessionStatus::Active,
            "EXTENDED" => crate::listing_auction_session::ListingAuctionSessionStatus::Extended,
            "CLOSED" | "ENDED" => crate::listing_auction_session::ListingAuctionSessionStatus::Closed,
            "WON" => crate::listing_auction_session::ListingAuctionSessionStatus::Won,
            "UNSOLD" => crate::listing_auction_session::ListingAuctionSessionStatus::Unsold,
            "CANCELLED" => crate::listing_auction_session::ListingAuctionSessionStatus::Cancelled,
            _ => crate::listing_auction_session::ListingAuctionSessionStatus::Draft,
        };

        let current_highest = self
            .bid_repo
            .find_winning_bid(&record.id)
            .await?
            .map(|bid_record| crate::listing_auction_session::Bid {
                bidder_id: UserId::new(bid_record.bidder_id),
                amount: Money::from_cents(bid_record.bid_amount_cents as u64),
                placed_at: UnixSeconds::new(bid_record.bid_time as u64),
            });

        Ok(ListingAuctionSession::with_status(
            &record.id,
            &record.listing_id,
            &record.seller_id,
            Money::from_cents(record.starting_price_cents as u64),
            Money::from_cents(record.minimum_increment_cents as u64),
            Money::from_cents(record.reserve_price_cents as u64),
            UnixSeconds::new(record.start_time as u64),
            UnixSeconds::new(record.end_time as u64),
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
    ) -> Result<ListingSummary, String> {
        let catalog_client = self.catalog_client.as_ref().ok_or_else(|| {
            "Catalogue service is not configured".to_string()
        })?;

        let listing = catalog_client
            .get_listing_summary(listing_id)
            .await
            .map_err(|error| error.to_string())?;

        if !is_catalog_listing_biddable(&listing.status) {
            return Err("Listing is not active".to_string());
        }

        Ok(listing)
    }

    fn status_to_string(&self, status: crate::listing_auction_session::ListingAuctionSessionStatus) -> String {
        match status {
            crate::listing_auction_session::ListingAuctionSessionStatus::Draft => "DRAFT".to_string(),
            crate::listing_auction_session::ListingAuctionSessionStatus::Active => "ACTIVE".to_string(),
            crate::listing_auction_session::ListingAuctionSessionStatus::Extended => "EXTENDED".to_string(),
            crate::listing_auction_session::ListingAuctionSessionStatus::Closed => "CLOSED".to_string(),
            crate::listing_auction_session::ListingAuctionSessionStatus::Won => "WON".to_string(),
            crate::listing_auction_session::ListingAuctionSessionStatus::Unsold => "UNSOLD".to_string(),
            crate::listing_auction_session::ListingAuctionSessionStatus::Cancelled => "CANCELLED".to_string(),
        }
    }

    pub async fn get_auction_with_bids(
        &self,
        auction_id: &str,
    ) -> Result<Option<(String, Vec<String>)>, sqlx::Error> {
        match self
            .listing_auction_session_repo
            .find_by_id(auction_id)
            .await?
            .or(self.listing_auction_session_repo.find_by_listing_id(auction_id).await?)
        {
            Some(auction) => {
                let bids = self.bid_repo.list_by_auction_id_desc(&auction.id).await?;
                let bid_ids: Vec<String> = bids.iter().map(|b| b.id.clone()).collect();
                Ok(Some((auction.id, bid_ids)))
            }
            None => Ok(None),
        }
    }

    async fn publish_bid_placed_event(
        &self,
        auction: &ListingAuctionSessionRecord,
        bid: &BidRecord,
    ) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().timestamp();
        let payload = serde_json::json!({
            "auctionId": auction.id,
            "listingId": auction.listing_id,
            "sellerId": auction.seller_id,
            "bidId": bid.id,
            "bidderId": bid.bidder_id,
            "amountCents": bid.bid_amount_cents,
            "currentPrice": bid.bid_amount_cents,
            "status": auction.status,
            "endTime": auction.end_time,
            "bidTime": bid.bid_time,
            "placedAt": now
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

    async fn publish_outbid_event(
        &self,
        auction: &ListingAuctionSessionRecord,
        previous_bid: &BidRecord,
        new_amount_cents: i64,
    ) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().timestamp();
        let payload = serde_json::json!({
            "auctionId": auction.id,
            "listingId": auction.listing_id,
            "sellerId": auction.seller_id,
            "previousBidderId": previous_bid.bidder_id,
            "amountCents": new_amount_cents,
            "currentPrice": new_amount_cents,
            "outbidAt": now
        })
        .to_string();
        let event = NewOutboxEventRecord {
            id: uuid::Uuid::new_v4().to_string(),
            aggregate_id: auction.id.clone(),
            event_type: "Outbid".to_string(),
            payload,
            published: false,
            published_at: None,
            created_at: now,
            updated_at: now,
        };
        self.outbox_repo.insert(&event).await?;
        Ok(())
    }

    async fn publish_auction_created_event(
        &self,
        auction: &ListingAuctionSessionRecord,
    ) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().timestamp();
        let payload = serde_json::json!({
            "auctionId": auction.id,
            "listingId": auction.listing_id,
            "sellerId": auction.seller_id,
            "startingPrice": auction.starting_price_cents,
            "reservePrice": auction.reserve_price_cents,
            "minimumIncrement": auction.minimum_increment_cents,
            "status": auction.status,
            "startTime": auction.start_time,
            "endTime": auction.end_time,
            "createdAt": now
        })
        .to_string();
        let event = NewOutboxEventRecord {
            id: uuid::Uuid::new_v4().to_string(),
            aggregate_id: auction.id.clone(),
            event_type: "AuctionCreated".to_string(),
            payload,
            published: false,
            published_at: None,
            created_at: now,
            updated_at: now,
        };
        self.outbox_repo.insert(&event).await?;
        Ok(())
    }

    async fn publish_auction_ended_event_with_tx(
        &self,
        auction: &ListingAuctionSessionRecord,
        winning_bid: Option<&BidRecord>,
        tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    ) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().timestamp();
        let reserve_met = auction.status == "WON";
        let payload = if reserve_met {
            serde_json::json!({
                "auctionId": auction.id,
                "listingId": auction.listing_id,
                "sellerId": auction.seller_id,
                "status": auction.status,
                "reserveMet": true,
                "winnerId": winning_bid.map(|bid| bid.bidder_id.as_str()),
                "finalPrice": winning_bid.map(|bid| bid.bid_amount_cents),
                "endedAt": now
            })
        } else {
            serde_json::json!({
                "auctionId": auction.id,
                "listingId": auction.listing_id,
                "sellerId": auction.seller_id,
                "status": auction.status,
                "reserveMet": false,
                "endedAt": now
            })
        }
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
        self.outbox_repo.insert_with_tx(&event, tx).await?;
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
    pub auction_type: String,
    pub starting_price_cents: i64,
    pub reserve_price_cents: i64,
    pub minimum_increment_cents: i64,
    pub start_time: i64,
    pub end_time: i64,
}

impl CreateAuctionCommand {
    fn validate(&self, now: i64) -> Result<(), CreateAuctionError> {
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

fn initial_status(start_time: i64, now: i64) -> String {
    if start_time > now {
        "DRAFT".to_string()
    } else {
        "ACTIVE".to_string()
    }
}

fn is_catalog_listing_biddable(status: &str) -> bool {
    status.eq_ignore_ascii_case("ACTIVE") || status.eq_ignore_ascii_case("EXTENDED")
}

fn hold_expiration_rfc3339(end_time_unix: i64) -> String {
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
enum BidPlacementMode {
    Standard { amount_cents: i64 },
    Proxy { max_amount_cents: i64 },
}

#[derive(Debug, Clone)]
struct BidCursor {
    amount_cents: i64,
    bid_time: i64,
    id: String,
}

fn parse_bid_cursor(value: &str) -> Result<BidCursor, String> {
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

fn bid_cursor_from_bid(bid: &BidRecord) -> BidCursor {
    BidCursor {
        amount_cents: bid.bid_amount_cents,
        bid_time: bid.bid_time,
        id: bid.id.clone(),
    }
}

fn cents_to_rupiah(amount_cents: i64) -> u64 {
    if amount_cents <= 0 {
        return 0;
    }
    ((amount_cents as u64) + 99) / 100
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

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_command() -> CreateAuctionCommand {
        CreateAuctionCommand {
            listing_id: "listing-1".to_string(),
            seller_id: "seller-1".to_string(),
            auction_type: "ENGLISH".to_string(),
            starting_price_cents: 1000,
            reserve_price_cents: 2000,
            minimum_increment_cents: 100,
            start_time: 100,
            end_time: 200,
        }
    }

    #[test] fn validate_ok() { assert!(valid_command().validate(100).is_ok()); }
    #[test] fn validate_empty_listing() { let mut c = valid_command(); c.listing_id = "  ".into(); assert!(c.validate(100).is_err()); }
    #[test] fn validate_empty_seller() { let mut c = valid_command(); c.seller_id = "".into(); assert!(c.validate(100).is_err()); }
    #[test] fn validate_empty_type() { let mut c = valid_command(); c.auction_type = " ".into(); assert!(c.validate(100).is_err()); }
    #[test] fn validate_zero_price() { let mut c = valid_command(); c.starting_price_cents = 0; assert!(c.validate(100).is_err()); }
    #[test] fn validate_neg_price() { let mut c = valid_command(); c.starting_price_cents = -5; assert!(c.validate(100).is_err()); }
    #[test] fn validate_zero_incr() { let mut c = valid_command(); c.minimum_increment_cents = 0; assert!(c.validate(100).is_err()); }
    #[test] fn validate_reserve_below() { let mut c = valid_command(); c.reserve_price_cents = 500; assert!(c.validate(100).is_err()); }
    #[test] fn validate_reserve_eq() { let mut c = valid_command(); c.reserve_price_cents = 1000; assert!(c.validate(100).is_ok()); }
    #[test] fn validate_end_before() { let mut c = valid_command(); c.end_time = 50; assert!(c.validate(100).is_err()); }
    #[test] fn validate_end_eq() { let mut c = valid_command(); c.end_time = 100; assert!(c.validate(100).is_err()); }
    #[test] fn validate_start_in_past() { let mut c = valid_command(); c.start_time = 100 - CREATE_AUCTION_START_TIME_CLOCK_SKEW_SECONDS - 1; assert!(c.validate(100).is_err()); }
    #[test] fn validate_start_allows_publish_clock_skew() { let mut c = valid_command(); c.start_time = 99; assert!(c.validate(100).is_ok()); }
    #[test] fn validate_end_not_future() { let mut c = valid_command(); c.start_time = 100; c.end_time = 100; assert!(c.validate(100).is_err()); }

    #[test] fn status_draft() { assert_eq!(initial_status(200, 100), "DRAFT"); }
    #[test] fn status_active() { assert_eq!(initial_status(100, 200), "ACTIVE"); }
    #[test] fn status_active_eq() { assert_eq!(initial_status(100, 100), "ACTIVE"); }

    #[test] fn biddable_active() { assert!(is_catalog_listing_biddable("ACTIVE")); }
    #[test] fn biddable_ext() { assert!(is_catalog_listing_biddable("EXTENDED")); }
    #[test] fn biddable_ci() { assert!(is_catalog_listing_biddable("active")); assert!(is_catalog_listing_biddable("Extended")); }
    #[test] fn not_biddable_draft() { assert!(!is_catalog_listing_biddable("DRAFT")); }
    #[test] fn not_biddable_closed() { assert!(!is_catalog_listing_biddable("CLOSED")); }
    #[test] fn not_biddable_empty() { assert!(!is_catalog_listing_biddable("")); }

    #[test] fn cursor_ok() { let c = parse_bid_cursor("5000:170:bid-a").unwrap(); assert_eq!(c.amount_cents, 5000); assert_eq!(c.id, "bid-a"); }
    #[test] fn cursor_bad_amt() { assert!(parse_bid_cursor("x:1:b").is_err()); }
    #[test] fn cursor_bad_time() { assert!(parse_bid_cursor("1:x:b").is_err()); }
    #[test] fn cursor_missing_id() { assert!(parse_bid_cursor("1:2").is_err()); }
    #[test] fn cursor_empty_id() { assert!(parse_bid_cursor("1:2: ").is_err()); }
    #[test] fn cursor_colons_in_id() { let c = parse_bid_cursor("1:2:a:b:c").unwrap(); assert_eq!(c.id, "a:b:c"); }

    #[test]
    fn cursor_roundtrip() {
        let bid = BidRecord { id: "b1".into(), auction_id: "a1".into(), bidder_id: "u1".into(), bid_amount_cents: 5000, bid_time: 170, wallet_hold_id: None };
        let d = bid_cursor_from_bid(&bid).to_string();
        assert_eq!(d, "5000:170:b1");
        let p = parse_bid_cursor(&d).unwrap();
        assert_eq!(p.amount_cents, 5000);
    }

    #[test]
    fn error_display() {
        assert!(format!("{}", PlaceBidError::AuctionNotFound).contains("not found"));
        assert!(format!("{}", PlaceBidError::CatalogError("f".into())).contains("Catalog"));
        assert!(format!("{}", PlaceBidError::WalletError("f".into())).contains("Wallet"));
        assert!(format!("{}", PlaceBidError::DatabaseError("f".into())).contains("Database"));
        assert!(!format!("{}", PlaceBidError::BidError(BidError::BidTooLow { minimum: Money::from_cents(1) })).is_empty());
        assert_eq!(format!("{}", CreateAuctionError::InvalidInput("x".into())), "x");
        assert!(format!("{}", CreateAuctionError::DatabaseError("x".into())).contains("Database"));
        assert!(format!("{}", CloseListingAuctionSessionError::AuctionNotFound).contains("not found"));
        assert!(format!("{}", CloseListingAuctionSessionError::AuctionNotEnded).contains("end time"));
        assert!(format!("{}", CloseListingAuctionSessionError::WalletError("x".into())).contains("Wallet"));
        assert!(format!("{}", CloseListingAuctionSessionError::DatabaseError("x".into())).contains("Database"));
        assert_eq!(format!("{}", ListBidsError::InvalidInput("x".into())), "x");
        assert!(format!("{}", ListBidsError::DatabaseError("x".into())).contains("Database"));
        assert!(format!("{}", GetListingAuctionSessionError::DatabaseError("x".into())).contains("Database"));
        assert!(format!("{}", ListListingAuctionSessionsError::DatabaseError("x".into())).contains("Database"));
        assert!(format!("{}", ListPendingClosureError::DatabaseError("x".into())).contains("Database"));
    }

    #[test]
    fn bid_cursor_page_fields() {
        let p = BidCursorPage { items: vec![], next_cursor: None, size: 20 };
        assert_eq!(p.items.len(), 0);
        assert!(p.next_cursor.is_none());
    }
}
