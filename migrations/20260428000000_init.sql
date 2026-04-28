CREATE TABLE auctions (
    id TEXT PRIMARY KEY,
    listing_id TEXT NOT NULL,
    seller_id TEXT NOT NULL,
    starting_price_cents INTEGER NOT NULL,
    reserve_price_cents INTEGER NOT NULL,
    current_highest_bid_cents INTEGER,
    minimum_increment_cents INTEGER NOT NULL,
    status TEXT NOT NULL,
    start_time INTEGER NOT NULL,
    end_time INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE bids (
    id TEXT PRIMARY KEY,
    auction_id TEXT NOT NULL,
    bidder_id TEXT NOT NULL,
    bid_amount_cents INTEGER NOT NULL,
    bid_time INTEGER NOT NULL,
    FOREIGN KEY (auction_id) REFERENCES auctions(id)
);

CREATE INDEX bids_auction_id_idx ON bids(auction_id);
CREATE INDEX bids_auction_bid_time_idx ON bids(auction_id, bid_time DESC);
CREATE INDEX bids_auction_amount_idx ON bids(auction_id, bid_amount_cents DESC, bid_time ASC);

CREATE TABLE outbox_events (
    id TEXT PRIMARY KEY,
    aggregate_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    payload TEXT NOT NULL,
    published INTEGER NOT NULL,
    published_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX outbox_events_published_idx ON outbox_events(published, created_at);
