use crate::persistence::models::{BidRecord, ListingAuctionSessionRecord};
use crate::service::auction_commands::CloseAuctionHandler;
use crate::service::auction_core::AuctionService;
use crate::service::auction_types::CloseListingAuctionSessionError;
use crate::service::closure_workflow::AuctionClosureWorkflowFactory;

impl AuctionService {
    /// Optimized for background queue processing using Pessimistic Lock.
    pub async fn process_one_pending_closure(
        &self,
    ) -> Result<Option<ListingAuctionSessionRecord>, CloseListingAuctionSessionError> {
        CloseAuctionHandler::new(self).process_one_pending().await
    }

    pub(crate) async fn process_one_pending_closure_core(
        &self,
    ) -> Result<Option<ListingAuctionSessionRecord>, CloseListingAuctionSessionError> {
        let now = chrono::Utc::now().timestamp();
        self.closure_job_repo
            .reconcile_missing_pending_jobs(now)
            .await
            .map_err(|error| CloseListingAuctionSessionError::DatabaseError(error.to_string()))?;

        let worker_id = format!("auction-closure-{}", std::process::id());
        let job = self
            .closure_job_repo
            .claim_due(now, now + 60, &worker_id)
            .await
            .map_err(|error| CloseListingAuctionSessionError::DatabaseError(error.to_string()))?;

        let Some(job) = job else {
            return Ok(None);
        };

        let mut tx = self
            .listing_auction_session_repo
            .pool
            .begin()
            .await
            .map_err(|error| CloseListingAuctionSessionError::DatabaseError(error.to_string()))?;

        let pending_auction = self
            .listing_auction_session_repo
            .find_by_id_for_update(&job.auction_id, &mut tx)
            .await
            .map_err(|error| CloseListingAuctionSessionError::DatabaseError(error.to_string()))?;

        let Some(auction) = pending_auction else {
            tx.rollback()
                .await
                .map_err(|e| CloseListingAuctionSessionError::DatabaseError(e.to_string()))?;
            self.closure_job_repo
                .mark_done(&job.auction_id, now)
                .await
                .map_err(|e| CloseListingAuctionSessionError::DatabaseError(e.to_string()))?;
            return Ok(None);
        };

        if job.status == "SETTLING" {
            let bids = self
                .bid_repo
                .list_by_auction_id_desc_with_tx(&auction.id, &mut tx)
                .await
                .map_err(|error| {
                    CloseListingAuctionSessionError::DatabaseError(error.to_string())
                })?;
            tx.commit()
                .await
                .map_err(|e| CloseListingAuctionSessionError::DatabaseError(e.to_string()))?;

            if let Err(error) = self
                .execute_post_closure_side_effects(&auction, &bids)
                .await
            {
                let retry_at = now + 30;
                self.closure_job_repo
                    .mark_settlement_failed(&auction.id, now, retry_at, &error.to_string())
                    .await
                    .map_err(|e| CloseListingAuctionSessionError::DatabaseError(e.to_string()))?;
                return Err(error);
            }

            self.closure_job_repo
                .mark_done(&auction.id, now)
                .await
                .map_err(|e| CloseListingAuctionSessionError::DatabaseError(e.to_string()))?;

            return Ok(Some(auction));
        }

        if auction.status == "WON" || auction.status == "UNSOLD" || auction.status == "CANCELLED" {
            tx.rollback()
                .await
                .map_err(|e| CloseListingAuctionSessionError::DatabaseError(e.to_string()))?;
            self.closure_job_repo
                .mark_done(&auction.id, now)
                .await
                .map_err(|e| CloseListingAuctionSessionError::DatabaseError(e.to_string()))?;
            return Ok(None);
        }

        if auction.end_time > now {
            tx.rollback()
                .await
                .map_err(|e| CloseListingAuctionSessionError::DatabaseError(e.to_string()))?;
            self.closure_job_repo
                .upsert_pending(&auction.id, auction.end_time, now)
                .await
                .map_err(|e| CloseListingAuctionSessionError::DatabaseError(e.to_string()))?;
            return Ok(None);
        }

        let (updated, bids) = match self.execute_auction_closure(auction, &mut tx, now).await {
            Ok(result) => result,
            Err(error) => {
                let _ = tx.rollback().await;
                let retry_at = now + 30;
                self.closure_job_repo
                    .mark_failed(&job.auction_id, now, retry_at, &error.to_string())
                    .await
                    .map_err(|e| CloseListingAuctionSessionError::DatabaseError(e.to_string()))?;
                return Err(error);
            }
        };

        tx.commit()
            .await
            .map_err(|e| CloseListingAuctionSessionError::DatabaseError(e.to_string()))?;

        self.closure_job_repo
            .mark_settling(&updated.id, now, now)
            .await
            .map_err(|e| CloseListingAuctionSessionError::DatabaseError(e.to_string()))?;

        if let Err(error) = self
            .execute_post_closure_side_effects(&updated, &bids)
            .await
        {
            let retry_at = now + 30;
            self.closure_job_repo
                .mark_settlement_failed(&updated.id, now, retry_at, &error.to_string())
                .await
                .map_err(|e| CloseListingAuctionSessionError::DatabaseError(e.to_string()))?;
            return Err(error);
        }

        self.closure_job_repo
            .mark_done(&updated.id, now)
            .await
            .map_err(|e| CloseListingAuctionSessionError::DatabaseError(e.to_string()))?;

        Ok(Some(updated))
    }

    /// Manual close endpoint secured with row-level transaction.
    pub async fn close_auction(
        &self,
        auction_id: &str,
    ) -> Result<ListingAuctionSessionRecord, CloseListingAuctionSessionError> {
        CloseAuctionHandler::new(self).close(auction_id).await
    }

    pub(crate) async fn close_auction_core(
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
            tx.rollback()
                .await
                .map_err(|e| CloseListingAuctionSessionError::DatabaseError(e.to_string()))?;
            return Err(CloseListingAuctionSessionError::AuctionNotEnded);
        }

        if auction.status == "WON" || auction.status == "UNSOLD" {
            tx.rollback()
                .await
                .map_err(|e| CloseListingAuctionSessionError::DatabaseError(e.to_string()))?;
            return Ok(auction);
        }

        let (updated, bids) = self.execute_auction_closure(auction, &mut tx, now).await?;

        tx.commit()
            .await
            .map_err(|e| CloseListingAuctionSessionError::DatabaseError(e.to_string()))?;

        self.closure_job_repo
            .mark_settling(&updated.id, now, now)
            .await
            .map_err(|e| CloseListingAuctionSessionError::DatabaseError(e.to_string()))?;

        if let Err(error) = self
            .execute_post_closure_side_effects(&updated, &bids)
            .await
        {
            let retry_at = now + 30;
            self.closure_job_repo
                .mark_settlement_failed(&updated.id, now, retry_at, &error.to_string())
                .await
                .map_err(|e| CloseListingAuctionSessionError::DatabaseError(e.to_string()))?;
            return Err(error);
        }
        self.closure_job_repo
            .mark_done(&updated.id, now)
            .await
            .map_err(|e| CloseListingAuctionSessionError::DatabaseError(e.to_string()))?;

        Ok(updated)
    }

    async fn execute_auction_closure(
        &self,
        auction: ListingAuctionSessionRecord,
        tx: &mut sqlx::Transaction<'_, sqlx::Any>,
        now: i64,
    ) -> Result<(ListingAuctionSessionRecord, Vec<BidRecord>), CloseListingAuctionSessionError>
    {
        let bids = self
            .bid_repo
            .list_by_auction_id_desc_with_tx(&auction.id, tx)
            .await
            .map_err(|error| CloseListingAuctionSessionError::DatabaseError(error.to_string()))?;

        let winning_bid = bids.first();
        let decision =
            AuctionClosureWorkflowFactory::for_auction(&auction).determine_outcome(&auction, &bids);

        let updated = self
            .listing_auction_session_repo
            .update_lifecycle_status_with_tx(
                &auction.id,
                decision.status,
                decision.highest_bid_cents,
                now,
                tx,
            )
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
                        .map_err(|error| {
                            CloseListingAuctionSessionError::WalletError(error.to_string())
                        })?;
                }
            } else {
                for bid in bids {
                    if let Some(hold_id) = &bid.wallet_hold_id {
                        wallet_client.release_hold(hold_id).await.map_err(|error| {
                            CloseListingAuctionSessionError::WalletError(error.to_string())
                        })?;
                    }
                }
            }
        }
        Ok(())
    }
}
