//! Tests for config module — covering env var resolution fallback paths.

use bidmart_auction_service_rust::config;
use std::sync::{Mutex, OnceLock};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn with_env_var<T>(key: &str, value: Option<&str>, test: impl FnOnce() -> T) -> T {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let previous = std::env::var(key).ok();
    unsafe {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    let result = test();

    unsafe {
        match previous {
            Some(previous) => std::env::set_var(key, previous),
            None => std::env::remove_var(key),
        }
    }
    result
}

#[test]
fn resolve_bind_address_default() {
    // Without env vars set, should return default
    let addr = config::resolve_bind_address();
    assert!(!addr.is_empty());
    // Should contain a port
    assert!(addr.contains(':'));
}

#[test]
fn resolve_auction_closure_interval_default() {
    let ms = config::resolve_auction_closure_interval_ms();
    assert!(ms > 0);
}

#[test]
fn resolve_outbox_interval_default() {
    let ms = config::resolve_outbox_interval_ms();
    assert!(ms > 0);
}

#[test]
fn resolve_rabbitmq_url_default() {
    let url = config::resolve_rabbitmq_url();
    assert!(url.contains("amqp"));
}

#[test]
fn resolve_events_exchange_default() {
    let exchange = config::resolve_events_exchange();
    assert!(exchange.contains("bidmart"));
}

#[test]
fn resolve_database_url_default() {
    let url = config::resolve_database_url();
    assert!(!url.is_empty());
}

#[test]
fn resolve_bind_address_prefers_bind_address() {
    with_env_var("BIND_ADDRESS", Some("127.0.0.1:4001"), || {
        assert_eq!(config::resolve_bind_address(), "127.0.0.1:4001");
    });
}

#[test]
fn resolve_bind_address_uses_port_when_bind_address_missing() {
    let _guard = env_lock().lock().expect("env lock poisoned");
    let previous_bind = std::env::var("BIND_ADDRESS").ok();
    let previous_port = std::env::var("PORT").ok();
    unsafe {
        std::env::remove_var("BIND_ADDRESS");
        std::env::set_var("PORT", "4010");
    }

    assert_eq!(config::resolve_bind_address(), "0.0.0.0:4010");

    unsafe {
        match previous_bind {
            Some(value) => std::env::set_var("BIND_ADDRESS", value),
            None => std::env::remove_var("BIND_ADDRESS"),
        }
        match previous_port {
            Some(value) => std::env::set_var("PORT", value),
            None => std::env::remove_var("PORT"),
        }
    }
}

#[test]
fn scheduler_config_accepts_valid_values_and_rejects_invalid_batch_sizes() {
    with_env_var(
        "AUCTION_CLOSURE_SCHEDULER_INTERVAL_MS",
        Some("2500"),
        || {
            assert_eq!(config::resolve_auction_closure_interval_ms(), 2500);
        },
    );
    with_env_var("OUTBOX_SCHEDULER_INTERVAL_MS", Some("3000"), || {
        assert_eq!(config::resolve_outbox_interval_ms(), 3000);
    });
    with_env_var("SCHEDULER_JITTER_MS", Some("125"), || {
        assert_eq!(config::resolve_scheduler_jitter_ms(), 125);
    });
    with_env_var("AUCTION_CLOSURE_BATCH_SIZE", Some("10"), || {
        assert_eq!(config::resolve_auction_closure_batch_size(), 10);
    });
    with_env_var("AUCTION_CLOSURE_BATCH_SIZE", Some("0"), || {
        assert_eq!(config::resolve_auction_closure_batch_size(), 50);
    });
    with_env_var("OUTBOX_BATCH_SIZE", Some("12"), || {
        assert_eq!(config::resolve_outbox_batch_size(), 12);
    });
    with_env_var("OUTBOX_BATCH_SIZE", Some("-1"), || {
        assert_eq!(config::resolve_outbox_batch_size(), 50);
    });
}

#[test]
fn message_and_hold_config_accept_env_overrides() {
    with_env_var("RABBITMQ_URL", Some("amqp://user:pass@example/%2f"), || {
        assert_eq!(
            config::resolve_rabbitmq_url(),
            "amqp://user:pass@example/%2f"
        );
    });
    with_env_var("BIDMART_EVENTS_EXCHANGE", Some("custom.events"), || {
        assert_eq!(config::resolve_events_exchange(), "custom.events");
    });
    with_env_var("BID_HOLD_GRACE_SECONDS", Some("3600"), || {
        assert_eq!(config::bid_hold_grace_seconds(), 3600);
    });
    with_env_var("BID_HOLD_GRACE_SECONDS", Some("invalid"), || {
        assert_eq!(config::bid_hold_grace_seconds(), 72 * 60 * 60);
    });
}
