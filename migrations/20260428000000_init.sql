-- Listings table: now owns auction lifecycle + bidding state
CREATE TABLE IF NOT EXISTS listings (
    id TEXT PRIMARY KEY,
    listing_id TEXT NOT NULL UNIQUE,
    seller_id TEXT NOT NULL,
    auction_type TEXT NOT NULL DEFAULT 'ENGLISH',
    starting_price_cents INTEGER NOT NULL,
    reserve_price_cents INTEGER NOT NULL,
    current_highest_bid_cents INTEGER,
    minimum_increment_cents INTEGER NOT NULL,
    lifecycle_state TEXT NOT NULL DEFAULT 'DRAFT',
    start_time INTEGER NOT NULL,
    end_time INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS bids (
    id TEXT PRIMARY KEY,
    listing_id TEXT NOT NULL,
    bidder_id TEXT NOT NULL,
    bid_amount_cents INTEGER NOT NULL,
    bid_time INTEGER NOT NULL,
    wallet_hold_id TEXT,
    FOREIGN KEY (listing_id) REFERENCES listings(id)
);

CREATE INDEX IF NOT EXISTS bids_listing_id_idx ON bids(listing_id);
CREATE INDEX IF NOT EXISTS bids_listing_bid_time_idx ON bids(listing_id, bid_time DESC);
CREATE INDEX IF NOT EXISTS bids_listing_amount_idx ON bids(listing_id, bid_amount_cents DESC, bid_time ASC);

CREATE TABLE IF NOT EXISTS proxy_bids (
    listing_id TEXT NOT NULL,
    bidder_id TEXT NOT NULL,
    max_bid_amount_cents INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (listing_id, bidder_id),
    FOREIGN KEY (listing_id) REFERENCES listings(id)
);

CREATE INDEX IF NOT EXISTS proxy_bids_listing_max_idx
    ON proxy_bids(listing_id, max_bid_amount_cents DESC, created_at ASC, bidder_id ASC);

CREATE TABLE IF NOT EXISTS outbox_events (
    id TEXT PRIMARY KEY,
    aggregate_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    payload TEXT NOT NULL,
    published BOOLEAN NOT NULL,
    published_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS outbox_events_published_idx ON outbox_events(published, created_at);
