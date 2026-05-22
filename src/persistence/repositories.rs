use sqlx::AnyPool;

use crate::persistence::models::{
    AuctionClosureJobRecord, BidRecord, ListingAuctionSessionRecord, NewBidRecord,
    NewListingAuctionSessionRecord, NewOutboxEventRecord, OutboxEventRecord, ProxyBidRecord,
};

#[derive(Debug, Clone)]
pub struct ListingAuctionSessionRepository {
    pub pool: AnyPool,
}

impl ListingAuctionSessionRepository {
    pub fn new(pool: AnyPool) -> Self {
        Self { pool }
    }

    pub async fn insert(
        &self,
        auction: &NewListingAuctionSessionRecord,
    ) -> Result<ListingAuctionSessionRecord, sqlx::Error> {
        sqlx::query_as::<_, ListingAuctionSessionRecord>(
            "INSERT INTO listings (id, listing_id, seller_id, auction_type, starting_price_cents, reserve_price_cents, \
             current_highest_bid_cents, minimum_increment_cents, lifecycle_state, start_time, end_time, created_at, updated_at) \
             VALUES ($1, $2, $3, 'ENGLISH', $4, $5, $6, $7, $8, $9, $10, $11, $12) \
             RETURNING id, listing_id, seller_id, auction_type, starting_price_cents, reserve_price_cents, current_highest_bid_cents, \
             minimum_increment_cents, lifecycle_state AS status, start_time, end_time, created_at, updated_at",
        )
        .bind(&auction.id)
        .bind(&auction.listing_id)
        .bind(&auction.seller_id)
        .bind(auction.starting_price_cents)
        .bind(auction.reserve_price_cents)
        .bind(auction.current_highest_bid_cents)
        .bind(auction.minimum_increment_cents)
        .bind(&auction.status)
        .bind(auction.start_time)
        .bind(auction.end_time)
        .bind(auction.created_at)
        .bind(auction.updated_at)
        .fetch_one(&self.pool)
        .await
    }

    pub async fn find_by_id(
        &self,
        id: &str,
    ) -> Result<Option<ListingAuctionSessionRecord>, sqlx::Error> {
        sqlx::query_as::<_, ListingAuctionSessionRecord>(
            "SELECT id, listing_id, seller_id, auction_type, starting_price_cents, reserve_price_cents, \
             current_highest_bid_cents, minimum_increment_cents, lifecycle_state AS status, start_time, end_time, created_at, updated_at \
             FROM listings WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn find_by_listing_id(
        &self,
        listing_id: &str,
    ) -> Result<Option<ListingAuctionSessionRecord>, sqlx::Error> {
        sqlx::query_as::<_, ListingAuctionSessionRecord>(
            "SELECT id, listing_id, seller_id, auction_type, starting_price_cents, reserve_price_cents, \
             current_highest_bid_cents, minimum_increment_cents, lifecycle_state AS status, start_time, end_time, created_at, updated_at \
             FROM listings WHERE listing_id = $1 OR id = $1 \
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(listing_id)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn list_all(&self) -> Result<Vec<ListingAuctionSessionRecord>, sqlx::Error> {
        sqlx::query_as::<_, ListingAuctionSessionRecord>(
            "SELECT id, listing_id, seller_id, auction_type, starting_price_cents, reserve_price_cents, \
             current_highest_bid_cents, minimum_increment_cents, lifecycle_state AS status, start_time, end_time, created_at, updated_at \
             FROM listings ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
    }

    pub async fn list_pending_closure(
        &self,
        now: i64,
    ) -> Result<Vec<ListingAuctionSessionRecord>, sqlx::Error> {
        sqlx::query_as::<_, ListingAuctionSessionRecord>(
            "SELECT id, listing_id, seller_id, auction_type, starting_price_cents, reserve_price_cents, \
             current_highest_bid_cents, minimum_increment_cents, lifecycle_state AS status, start_time, end_time, created_at, updated_at \
             FROM listings \
             WHERE end_time <= $1 AND lifecycle_state NOT IN ('WON', 'UNSOLD', 'CLOSED', 'CANCELLED') \
             ORDER BY end_time ASC",
        )
        .bind(now)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn pop_pending_closure(
        &self,
        now: i64,
        tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    ) -> Result<Option<ListingAuctionSessionRecord>, sqlx::Error> {
        let use_locking = tx.backend_name() != "SQLite";
        let query = if use_locking {
            "SELECT id, listing_id, seller_id, auction_type, starting_price_cents, reserve_price_cents, \
             current_highest_bid_cents, minimum_increment_cents, lifecycle_state AS status, start_time, end_time, created_at, updated_at \
             FROM listings \
             WHERE end_time <= $1 AND lifecycle_state NOT IN ('WON', 'UNSOLD', 'CLOSED', 'CANCELLED') \
             ORDER BY end_time ASC \
             LIMIT 1 \
             FOR UPDATE SKIP LOCKED"
        } else {
            "SELECT id, listing_id, seller_id, auction_type, starting_price_cents, reserve_price_cents, \
             current_highest_bid_cents, minimum_increment_cents, lifecycle_state AS status, start_time, end_time, created_at, updated_at \
             FROM listings \
             WHERE end_time <= $1 AND lifecycle_state NOT IN ('WON', 'UNSOLD', 'CLOSED', 'CANCELLED') \
             ORDER BY end_time ASC \
             LIMIT 1"
        };

        sqlx::query_as::<_, ListingAuctionSessionRecord>(query)
            .bind(now)
            .fetch_optional(&mut **tx)
            .await
    }

    pub async fn find_by_id_for_update(
        &self,
        id: &str,
        tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    ) -> Result<Option<ListingAuctionSessionRecord>, sqlx::Error> {
        let use_locking = tx.backend_name() != "SQLite";
        let query = if use_locking {
            "SELECT id, listing_id, seller_id, auction_type, starting_price_cents, reserve_price_cents, \
             current_highest_bid_cents, minimum_increment_cents, lifecycle_state AS status, start_time, end_time, created_at, updated_at \
             FROM listings WHERE id = $1 OR listing_id = $1 FOR UPDATE"
        } else {
            "SELECT id, listing_id, seller_id, auction_type, starting_price_cents, reserve_price_cents, \
             current_highest_bid_cents, minimum_increment_cents, lifecycle_state AS status, start_time, end_time, created_at, updated_at \
             FROM listings WHERE id = $1 OR listing_id = $1"
        };

        sqlx::query_as::<_, ListingAuctionSessionRecord>(query)
            .bind(id)
            .fetch_optional(&mut **tx)
            .await
    }

    pub async fn update_lifecycle_status(
        &self,
        auction_id: &str,
        status: &str,
        current_highest_bid_cents: Option<i64>,
        updated_at: i64,
    ) -> Result<ListingAuctionSessionRecord, sqlx::Error> {
        sqlx::query_as::<_, ListingAuctionSessionRecord>(
            "UPDATE listings \
             SET lifecycle_state = $1, current_highest_bid_cents = $2, updated_at = $3 \
             WHERE id = $4 \
             RETURNING id, listing_id, seller_id, auction_type, starting_price_cents, reserve_price_cents, \
             current_highest_bid_cents, minimum_increment_cents, lifecycle_state AS status, start_time, end_time, created_at, updated_at",
        )
        .bind(status)
        .bind(current_highest_bid_cents)
        .bind(updated_at)
        .bind(auction_id)
        .fetch_one(&self.pool)
        .await
    }

    pub async fn update_lifecycle_status_with_tx(
        &self,
        auction_id: &str,
        status: &str,
        current_highest_bid_cents: Option<i64>,
        updated_at: i64,
        tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    ) -> Result<ListingAuctionSessionRecord, sqlx::Error> {
        sqlx::query_as::<_, ListingAuctionSessionRecord>(
            "UPDATE listings \
             SET lifecycle_state = $1, current_highest_bid_cents = $2, updated_at = $3 \
             WHERE id = $4 \
             RETURNING id, listing_id, seller_id, auction_type, starting_price_cents, reserve_price_cents, \
             current_highest_bid_cents, minimum_increment_cents, lifecycle_state AS status, start_time, end_time, created_at, updated_at",
        )
        .bind(status)
        .bind(current_highest_bid_cents)
        .bind(updated_at)
        .bind(auction_id)
        .fetch_one(&mut **tx)
        .await
    }
}

#[derive(Debug, Clone)]
pub struct BidRepository {
    pool: AnyPool,
}

#[derive(Debug, Clone)]
pub struct ProxyBidRepository {
    pool: AnyPool,
}

impl ProxyBidRepository {
    pub fn new(pool: AnyPool) -> Self {
        Self { pool }
    }

    pub async fn upsert_max(
        &self,
        auction_id: &str,
        bidder_id: &str,
        max_bid_amount_cents: i64,
        now: i64,
    ) -> Result<ProxyBidRecord, sqlx::Error> {
        sqlx::query_as::<_, ProxyBidRecord>(
            "INSERT INTO proxy_bids (listing_id, bidder_id, max_bid_amount_cents, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (listing_id, bidder_id) DO UPDATE \
             SET max_bid_amount_cents = excluded.max_bid_amount_cents, \
                 updated_at = excluded.updated_at \
             RETURNING listing_id AS auction_id, bidder_id, max_bid_amount_cents, created_at, updated_at",
        )
        .bind(auction_id)
        .bind(bidder_id)
        .bind(max_bid_amount_cents)
        .bind(now)
        .bind(now)
        .fetch_one(&self.pool)
        .await
    }

    pub async fn list_by_auction(
        &self,
        auction_id: &str,
    ) -> Result<Vec<ProxyBidRecord>, sqlx::Error> {
        sqlx::query_as::<_, ProxyBidRecord>(
            "SELECT listing_id AS auction_id, bidder_id, max_bid_amount_cents, created_at, updated_at \
             FROM proxy_bids WHERE listing_id = $1 \
             ORDER BY max_bid_amount_cents DESC, created_at ASC, bidder_id ASC",
        )
        .bind(auction_id)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn find_by_bidder(
        &self,
        auction_id: &str,
        bidder_id: &str,
    ) -> Result<Option<ProxyBidRecord>, sqlx::Error> {
        sqlx::query_as::<_, ProxyBidRecord>(
            "SELECT listing_id AS auction_id, bidder_id, max_bid_amount_cents, created_at, updated_at \
             FROM proxy_bids WHERE listing_id = $1 AND bidder_id = $2",
        )
        .bind(auction_id)
        .bind(bidder_id)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn delete_for_bidder(
        &self,
        auction_id: &str,
        bidder_id: &str,
    ) -> Result<u64, sqlx::Error> {
        let result = sqlx::query("DELETE FROM proxy_bids WHERE listing_id = $1 AND bidder_id = $2")
            .bind(auction_id)
            .bind(bidder_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }
}

impl BidRepository {
    pub fn new(pool: AnyPool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, bid: &NewBidRecord) -> Result<BidRecord, sqlx::Error> {
        self.insert_with_wallet_hold(bid, None).await
    }

    pub async fn insert_with_wallet_hold(
        &self,
        bid: &NewBidRecord,
        wallet_hold_id: Option<&str>,
    ) -> Result<BidRecord, sqlx::Error> {
        sqlx::query_as::<_, BidRecord>(
            "INSERT INTO bids (id, listing_id, bidder_id, bid_amount_cents, bid_time, wallet_hold_id) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             RETURNING id, listing_id AS auction_id, bidder_id, bid_amount_cents, bid_time, wallet_hold_id",
        )
        .bind(&bid.id)
        .bind(&bid.auction_id)
        .bind(&bid.bidder_id)
        .bind(bid.bid_amount_cents)
        .bind(bid.bid_time)
        .bind(wallet_hold_id)
        .fetch_one(&self.pool)
        .await
    }

    pub async fn list_by_auction_id_desc(
        &self,
        auction_id: &str,
    ) -> Result<Vec<BidRecord>, sqlx::Error> {
        sqlx::query_as::<_, BidRecord>(
            "SELECT id, listing_id AS auction_id, bidder_id, bid_amount_cents, bid_time, wallet_hold_id \
             FROM bids WHERE listing_id = $1 \
             ORDER BY bid_amount_cents DESC, bid_time ASC, id ASC",
        )
        .bind(auction_id)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn list_by_auction_id_desc_with_tx(
        &self,
        auction_id: &str,
        tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    ) -> Result<Vec<BidRecord>, sqlx::Error> {
        sqlx::query_as::<_, BidRecord>(
            "SELECT id, listing_id AS auction_id, bidder_id, bid_amount_cents, bid_time, wallet_hold_id \
             FROM bids WHERE listing_id = $1 \
             ORDER BY bid_amount_cents DESC, bid_time ASC, id ASC",
        )
        .bind(auction_id)
        .fetch_all(&mut **tx)
        .await
    }

    pub async fn list_by_auction_cursor(
        &self,
        auction_id: &str,
        cursor: Option<(i64, i64, String)>,
        limit: i64,
    ) -> Result<Vec<BidRecord>, sqlx::Error> {
        match cursor {
            Some((amount, bid_time, id)) => {
                sqlx::query_as::<_, BidRecord>(
                    "SELECT id, listing_id AS auction_id, bidder_id, bid_amount_cents, bid_time, wallet_hold_id \
                     FROM bids \
                     WHERE listing_id = $1 \
                       AND (bid_amount_cents < $2 \
                         OR (bid_amount_cents = $2 AND bid_time > $3) \
                         OR (bid_amount_cents = $2 AND bid_time = $3 AND id > $4)) \
                     ORDER BY bid_amount_cents DESC, bid_time ASC, id ASC \
                     LIMIT $5",
                )
                .bind(auction_id)
                .bind(amount)
                .bind(bid_time)
                .bind(id)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
            }
            None => {
                sqlx::query_as::<_, BidRecord>(
                    "SELECT id, listing_id AS auction_id, bidder_id, bid_amount_cents, bid_time, wallet_hold_id \
                     FROM bids \
                     WHERE listing_id = $1 \
                     ORDER BY bid_amount_cents DESC, bid_time ASC, id ASC \
                     LIMIT $2",
                )
                .bind(auction_id)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
            }
        }
    }

    pub async fn find_winning_bid(
        &self,
        auction_id: &str,
    ) -> Result<Option<BidRecord>, sqlx::Error> {
        sqlx::query_as::<_, BidRecord>(
            "SELECT id, listing_id AS auction_id, bidder_id, bid_amount_cents, bid_time, wallet_hold_id \
             FROM bids WHERE listing_id = $1 \
             ORDER BY bid_amount_cents DESC, bid_time ASC, id ASC \
             LIMIT 1",
        )
        .bind(auction_id)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn find_matching_bid(
        &self,
        auction_id: &str,
        bidder_id: &str,
        bid_amount_cents: i64,
        bid_time: i64,
    ) -> Result<Option<BidRecord>, sqlx::Error> {
        sqlx::query_as::<_, BidRecord>(
            "SELECT id, listing_id AS auction_id, bidder_id, bid_amount_cents, bid_time, wallet_hold_id \
             FROM bids \
             WHERE listing_id = $1 AND bidder_id = $2 AND bid_amount_cents = $3 AND bid_time = $4 \
             ORDER BY id ASC \
             LIMIT 1",
        )
        .bind(auction_id)
        .bind(bidder_id)
        .bind(bid_amount_cents)
        .bind(bid_time)
        .fetch_optional(&self.pool)
        .await
    }
}

#[derive(Debug, Clone)]
pub struct OutboxRepository {
    pool: AnyPool,
}

impl OutboxRepository {
    pub fn new(pool: AnyPool) -> Self {
        Self { pool }
    }

    pub async fn insert(
        &self,
        event: &NewOutboxEventRecord,
    ) -> Result<OutboxEventRecord, sqlx::Error> {
        sqlx::query_as::<_, OutboxEventRecord>(
            "INSERT INTO outbox_events (id, aggregate_id, event_type, payload, published, created_at, updated_at, published_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             RETURNING id, aggregate_id, event_type, payload, CASE WHEN published THEN 1 ELSE 0 END AS published, \
             published_at, created_at, updated_at",
        )
        .bind(&event.id)
        .bind(&event.aggregate_id)
        .bind(&event.event_type)
        .bind(&event.payload)
        .bind(event.published)
        .bind(event.created_at)
        .bind(event.updated_at)
        .bind(event.published_at)
        .fetch_one(&self.pool)
        .await
    }

    pub async fn insert_with_tx(
        &self,
        event: &NewOutboxEventRecord,
        tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    ) -> Result<OutboxEventRecord, sqlx::Error> {
        sqlx::query_as::<_, OutboxEventRecord>(
            "INSERT INTO outbox_events (id, aggregate_id, event_type, payload, published, created_at, updated_at, published_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             RETURNING id, aggregate_id, event_type, payload, CASE WHEN published THEN 1 ELSE 0 END AS published, \
             published_at, created_at, updated_at",
        )
        .bind(&event.id)
        .bind(&event.aggregate_id)
        .bind(&event.event_type)
        .bind(&event.payload)
        .bind(event.published)
        .bind(event.created_at)
        .bind(event.updated_at)
        .bind(event.published_at)
        .fetch_one(&mut **tx)
        .await
    }

    pub async fn list_pending(&self, limit: i64) -> Result<Vec<OutboxEventRecord>, sqlx::Error> {
        sqlx::query_as::<_, OutboxEventRecord>(
            "SELECT id, aggregate_id, event_type, payload, CASE WHEN published THEN 1 ELSE 0 END AS published, \
             published_at, created_at, updated_at \
             FROM outbox_events WHERE published = $1 \
             ORDER BY created_at ASC \
             LIMIT $2",
        )
        .bind(false)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn claim_pending(
        &self,
        limit: i64,
        now: i64,
        lock_until: i64,
        worker_id: &str,
    ) -> Result<Vec<OutboxEventRecord>, sqlx::Error> {
        let use_postgres_claim = {
            let conn = self.pool.acquire().await?;
            conn.backend_name() != "SQLite"
        };

        if use_postgres_claim {
            return sqlx::query_as::<_, OutboxEventRecord>(
                "WITH candidate AS ( \
                    SELECT id \
                    FROM outbox_events \
                    WHERE published = $1 \
                      AND next_attempt_at <= $2 \
                      AND (locked_until IS NULL OR locked_until <= $2) \
                    ORDER BY created_at ASC \
                    LIMIT $3 \
                    FOR UPDATE SKIP LOCKED \
                 ) \
                 UPDATE outbox_events \
                 SET locked_until = $4, locked_by = $5, updated_at = $2 \
                 WHERE id IN (SELECT id FROM candidate) \
                 RETURNING id, aggregate_id, event_type, payload, CASE WHEN published THEN 1 ELSE 0 END AS published, \
                 published_at, created_at, updated_at",
            )
            .bind(false)
            .bind(now)
            .bind(limit)
            .bind(lock_until)
            .bind(worker_id)
            .fetch_all(&self.pool)
            .await;
        }

        let candidates = sqlx::query_as::<_, OutboxEventRecord>(
            "SELECT id, aggregate_id, event_type, payload, CASE WHEN published THEN 1 ELSE 0 END AS published, \
             published_at, created_at, updated_at \
             FROM outbox_events \
             WHERE published = $1 \
               AND next_attempt_at <= $2 \
               AND (locked_until IS NULL OR locked_until <= $2) \
             ORDER BY created_at ASC \
             LIMIT $3",
        )
        .bind(false)
        .bind(now)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut claimed = Vec::with_capacity(candidates.len());
        for event in candidates {
            let result = sqlx::query(
                "UPDATE outbox_events \
                 SET locked_until = $1, locked_by = $2, updated_at = $3 \
                 WHERE id = $4 \
                   AND published = $5 \
                   AND (locked_until IS NULL OR locked_until <= $3)",
            )
            .bind(lock_until)
            .bind(worker_id)
            .bind(now)
            .bind(&event.id)
            .bind(false)
            .execute(&self.pool)
            .await?;

            if result.rows_affected() == 1 {
                claimed.push(event);
            }
        }

        Ok(claimed)
    }

    pub async fn mark_published(
        &self,
        event_id: &str,
        published_at: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE outbox_events \
             SET published = $1, published_at = $2, updated_at = $3, locked_until = NULL, locked_by = NULL, last_error = NULL \
             WHERE id = $4",
        )
        .bind(true)
        .bind(published_at)
        .bind(published_at)
        .bind(event_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn mark_publish_failed(
        &self,
        event_id: &str,
        now: i64,
        next_attempt_at: i64,
        error: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE outbox_events \
             SET attempts = attempts + 1, next_attempt_at = $1, locked_until = NULL, locked_by = NULL, last_error = $2, updated_at = $3 \
             WHERE id = $4",
        )
        .bind(next_attempt_at)
        .bind(error)
        .bind(now)
        .bind(event_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct AuctionClosureJobRepository {
    pool: AnyPool,
}

impl AuctionClosureJobRepository {
    pub fn new(pool: AnyPool) -> Self {
        Self { pool }
    }

    pub async fn upsert_pending(
        &self,
        auction_id: &str,
        due_at: i64,
        now: i64,
    ) -> Result<AuctionClosureJobRecord, sqlx::Error> {
        sqlx::query_as::<_, AuctionClosureJobRecord>(
            "INSERT INTO auction_closure_jobs \
             (auction_id, due_at, status, attempts, locked_until, locked_by, last_error, created_at, updated_at) \
             VALUES ($1, $2, 'PENDING', 0, NULL, NULL, NULL, $3, $3) \
             ON CONFLICT (auction_id) DO UPDATE \
             SET due_at = excluded.due_at, \
                 status = CASE WHEN auction_closure_jobs.status IN ('DONE', 'SETTLING') THEN auction_closure_jobs.status ELSE 'PENDING' END, \
                 locked_until = NULL, locked_by = NULL, updated_at = excluded.updated_at \
             RETURNING auction_id, due_at, status, attempts, locked_until, locked_by, last_error, created_at, updated_at",
        )
        .bind(auction_id)
        .bind(due_at)
        .bind(now)
        .fetch_one(&self.pool)
        .await
    }

    pub async fn claim_due(
        &self,
        now: i64,
        lock_until: i64,
        worker_id: &str,
    ) -> Result<Option<AuctionClosureJobRecord>, sqlx::Error> {
        let use_postgres_claim = {
            let conn = self.pool.acquire().await?;
            conn.backend_name() != "SQLite"
        };

        if use_postgres_claim {
            return sqlx::query_as::<_, AuctionClosureJobRecord>(
                "WITH candidate AS ( \
                    SELECT auction_id \
                    FROM auction_closure_jobs \
                    WHERE status IN ('PENDING', 'SETTLING') \
                      AND due_at <= $1 \
                      AND (locked_until IS NULL OR locked_until <= $1) \
                    ORDER BY due_at ASC \
                    LIMIT 1 \
                    FOR UPDATE SKIP LOCKED \
                 ) \
                 UPDATE auction_closure_jobs \
                 SET status = 'PROCESSING', locked_until = $2, locked_by = $3, updated_at = $1 \
                 WHERE auction_id IN (SELECT auction_id FROM candidate) \
                 RETURNING auction_id, due_at, status, attempts, locked_until, locked_by, last_error, created_at, updated_at",
            )
            .bind(now)
            .bind(lock_until)
            .bind(worker_id)
            .fetch_optional(&self.pool)
            .await;
        }

        let candidate = sqlx::query_as::<_, AuctionClosureJobRecord>(
            "SELECT auction_id, due_at, status, attempts, locked_until, locked_by, last_error, created_at, updated_at \
             FROM auction_closure_jobs \
             WHERE status IN ('PENDING', 'SETTLING') \
               AND due_at <= $1 \
               AND (locked_until IS NULL OR locked_until <= $1) \
             ORDER BY due_at ASC \
             LIMIT 1",
        )
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

        let Some(job) = candidate else {
            return Ok(None);
        };

        let result = sqlx::query(
            "UPDATE auction_closure_jobs \
             SET status = 'PROCESSING', locked_until = $1, locked_by = $2, updated_at = $3 \
             WHERE auction_id = $4 \
               AND status IN ('PENDING', 'SETTLING') \
               AND (locked_until IS NULL OR locked_until <= $3)",
        )
        .bind(lock_until)
        .bind(worker_id)
        .bind(now)
        .bind(&job.auction_id)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 1 {
            Ok(Some(job))
        } else {
            Ok(None)
        }
    }

    pub async fn mark_done(&self, auction_id: &str, now: i64) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE auction_closure_jobs \
             SET status = 'DONE', locked_until = NULL, locked_by = NULL, last_error = NULL, updated_at = $1 \
             WHERE auction_id = $2",
        )
        .bind(now)
        .bind(auction_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn mark_settling(
        &self,
        auction_id: &str,
        due_at: i64,
        now: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO auction_closure_jobs \
             (auction_id, due_at, status, attempts, locked_until, locked_by, last_error, created_at, updated_at) \
             VALUES ($1, $2, 'SETTLING', 0, NULL, NULL, NULL, $3, $3) \
             ON CONFLICT (auction_id) DO UPDATE \
             SET status = 'SETTLING', due_at = excluded.due_at, locked_until = NULL, locked_by = NULL, last_error = NULL, updated_at = excluded.updated_at",
        )
        .bind(auction_id)
        .bind(due_at)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn mark_failed(
        &self,
        auction_id: &str,
        now: i64,
        next_attempt_at: i64,
        error: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE auction_closure_jobs \
             SET status = 'PENDING', attempts = attempts + 1, due_at = $1, locked_until = NULL, locked_by = NULL, last_error = $2, updated_at = $3 \
             WHERE auction_id = $4",
        )
        .bind(next_attempt_at)
        .bind(error)
        .bind(now)
        .bind(auction_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn mark_settlement_failed(
        &self,
        auction_id: &str,
        now: i64,
        next_attempt_at: i64,
        error: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE auction_closure_jobs \
             SET status = 'SETTLING', attempts = attempts + 1, due_at = $1, locked_until = NULL, locked_by = NULL, last_error = $2, updated_at = $3 \
             WHERE auction_id = $4",
        )
        .bind(next_attempt_at)
        .bind(error)
        .bind(now)
        .bind(auction_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn reconcile_missing_pending_jobs(&self, now: i64) -> Result<u64, sqlx::Error> {
        let result = sqlx::query(
            "INSERT INTO auction_closure_jobs \
             (auction_id, due_at, status, attempts, locked_until, locked_by, last_error, created_at, updated_at) \
             SELECT id, end_time, 'PENDING', 0, NULL, NULL, NULL, $1, $1 \
             FROM listings \
             WHERE end_time <= $1 \
               AND lifecycle_state NOT IN ('WON', 'UNSOLD', 'CLOSED', 'CANCELLED') \
               AND id NOT IN (SELECT auction_id FROM auction_closure_jobs)",
        )
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }
}
