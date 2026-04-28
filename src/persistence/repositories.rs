use sqlx::SqlitePool;

use crate::persistence::models::{
    AuctionRecord, BidRecord, NewAuctionRecord, NewBidRecord, NewOutboxEventRecord, OutboxEventRecord,
};

#[derive(Debug, Clone)]
pub struct AuctionRepository {
    pool: SqlitePool,
}

impl AuctionRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, auction: &NewAuctionRecord) -> Result<AuctionRecord, sqlx::Error> {
        sqlx::query_as::<_, AuctionRecord>(
            "INSERT INTO auctions (id, listing_id, seller_id, starting_price_cents, reserve_price_cents, \
             current_highest_bid_cents, minimum_increment_cents, status, start_time, end_time, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             RETURNING id, listing_id, seller_id, starting_price_cents, reserve_price_cents, current_highest_bid_cents, \
             minimum_increment_cents, status, start_time, end_time, created_at, updated_at"
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

    pub async fn find_by_id(&self, id: &str) -> Result<Option<AuctionRecord>, sqlx::Error> {
        sqlx::query_as::<_, AuctionRecord>(
            "SELECT id, listing_id, seller_id, starting_price_cents, reserve_price_cents, \
             current_highest_bid_cents, minimum_increment_cents, status, start_time, end_time, created_at, updated_at \
             FROM auctions WHERE id = ?"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn list_all(&self) -> Result<Vec<AuctionRecord>, sqlx::Error> {
        sqlx::query_as::<_, AuctionRecord>(
            "SELECT id, listing_id, seller_id, starting_price_cents, reserve_price_cents, \
             current_highest_bid_cents, minimum_increment_cents, status, start_time, end_time, created_at, updated_at \
             FROM auctions ORDER BY created_at DESC"
        )
        .fetch_all(&self.pool)
        .await
    }
}

#[derive(Debug, Clone)]
pub struct BidRepository {
    pool: SqlitePool,
}

impl BidRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, bid: &NewBidRecord) -> Result<BidRecord, sqlx::Error> {
        sqlx::query_as::<_, BidRecord>(
            "INSERT INTO bids (id, auction_id, bidder_id, bid_amount_cents, bid_time) \
             VALUES (?, ?, ?, ?, ?) \
             RETURNING id, auction_id, bidder_id, bid_amount_cents, bid_time"
        )
        .bind(&bid.id)
        .bind(&bid.auction_id)
        .bind(&bid.bidder_id)
        .bind(bid.bid_amount_cents)
        .bind(bid.bid_time)
        .fetch_one(&self.pool)
        .await
    }

    pub async fn list_by_auction_id_desc(
        &self,
        auction_id: &str,
    ) -> Result<Vec<BidRecord>, sqlx::Error> {
        sqlx::query_as::<_, BidRecord>(
            "SELECT id, auction_id, bidder_id, bid_amount_cents, bid_time \
             FROM bids WHERE auction_id = ? \
             ORDER BY bid_amount_cents DESC, bid_time ASC"
        )
        .bind(auction_id)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn find_winning_bid(
        &self,
        auction_id: &str,
    ) -> Result<Option<BidRecord>, sqlx::Error> {
        sqlx::query_as::<_, BidRecord>(
            "SELECT id, auction_id, bidder_id, bid_amount_cents, bid_time \
             FROM bids WHERE auction_id = ? \
             ORDER BY bid_amount_cents DESC, bid_time ASC \
             LIMIT 1"
        )
        .bind(auction_id)
        .fetch_optional(&self.pool)
        .await
    }
}

#[derive(Debug, Clone)]
pub struct OutboxRepository {
    pool: SqlitePool,
}

impl OutboxRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, event: &NewOutboxEventRecord) -> Result<OutboxEventRecord, sqlx::Error> {
        sqlx::query_as::<_, OutboxEventRecord>(
            "INSERT INTO outbox_events (id, aggregate_id, event_type, payload, published, created_at, updated_at, published_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
             RETURNING id, aggregate_id, event_type, payload, published, published_at, created_at, updated_at"
        )
        .bind(&event.id)
        .bind(&event.aggregate_id)
        .bind(&event.event_type)
        .bind(&event.payload)
        .bind(event.published as i32)
        .bind(event.created_at)
        .bind(event.updated_at)
        .bind(event.published_at)
        .fetch_one(&self.pool)
        .await
    }

    pub async fn list_pending(
        &self,
        limit: i64,
    ) -> Result<Vec<OutboxEventRecord>, sqlx::Error> {
        sqlx::query_as::<_, OutboxEventRecord>(
            "SELECT id, aggregate_id, event_type, payload, published, published_at, created_at, updated_at \
             FROM outbox_events WHERE published = 0 \
             ORDER BY created_at ASC \
             LIMIT ?"
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn mark_published(
        &self,
        event_id: &str,
        published_at: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE outbox_events SET published = 1, published_at = ?, updated_at = ? WHERE id = ?"
        )
        .bind(published_at)
        .bind(published_at)
        .bind(event_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
