use std::env;

use crate::server::default_database_url;

pub fn resolve_database_url() -> String {
    env::var("DATABASE_URL").unwrap_or_else(|_| default_database_url())
}

pub fn resolve_bind_address() -> String {
    env::var("BIND_ADDRESS")
        .or_else(|_| env::var("PORT").map(|port| format!("0.0.0.0:{port}")))
        .unwrap_or_else(|_| "0.0.0.0:3000".to_string())
}

pub fn resolve_auction_closure_interval_ms() -> u64 {
    env::var("AUCTION_CLOSURE_SCHEDULER_INTERVAL_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000)
}

pub fn resolve_auction_closure_batch_size() -> i64 {
    env::var("AUCTION_CLOSURE_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(50)
}

pub fn resolve_outbox_interval_ms() -> u64 {
    env::var("OUTBOX_SCHEDULER_INTERVAL_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000)
}

pub fn resolve_outbox_batch_size() -> i64 {
    env::var("OUTBOX_BATCH_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(50)
}

pub fn resolve_scheduler_jitter_ms() -> u64 {
    env::var("SCHEDULER_JITTER_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

pub fn resolve_rabbitmq_url() -> String {
    env::var("RABBITMQ_URL").unwrap_or_else(|_| "amqp://guest:guest@localhost:5672/%2f".to_string())
}

pub fn resolve_events_exchange() -> String {
    env::var("BIDMART_EVENTS_EXCHANGE").unwrap_or_else(|_| "bidmart.events".to_string())
}

/// Grace period after auction end before bid holds expire (default 72 hours).
pub fn bid_hold_grace_seconds() -> i64 {
    env::var("BID_HOLD_GRACE_SECONDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(72 * 60 * 60)
}
