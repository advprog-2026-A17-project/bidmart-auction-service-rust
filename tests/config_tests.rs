use std::sync::Mutex;

use bidmart_auction_service_rust::config::{
    resolve_auction_closure_interval_ms, resolve_bind_address, resolve_database_url,
    resolve_events_exchange, resolve_outbox_interval_ms, resolve_rabbitmq_url,
};
use bidmart_auction_service_rust::server::default_database_url;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn with_env_lock<F: FnOnce()>(f: F) {
    let _guard = ENV_LOCK.lock().expect("env lock");
    f();
}

#[test]
fn resolve_database_url_uses_default_when_missing() {
    with_env_lock(|| {
        unsafe {
            std::env::remove_var("DATABASE_URL");
        }
        assert_eq!(resolve_database_url(), default_database_url());
    });
}

#[test]
fn resolve_bind_address_prefers_bind_address() {
    with_env_lock(|| {
        unsafe {
            std::env::set_var("BIND_ADDRESS", "127.0.0.1:4321");
            std::env::remove_var("PORT");
        }
        assert_eq!(resolve_bind_address(), "127.0.0.1:4321");
        unsafe {
            std::env::remove_var("BIND_ADDRESS");
        }
    });
}

#[test]
fn resolve_bind_address_falls_back_to_port() {
    with_env_lock(|| {
        unsafe {
            std::env::remove_var("BIND_ADDRESS");
            std::env::set_var("PORT", "4567");
        }
        assert_eq!(resolve_bind_address(), "0.0.0.0:4567");
        unsafe {
            std::env::remove_var("PORT");
        }
    });
}

#[test]
fn resolve_intervals_use_defaults() {
    with_env_lock(|| {
        unsafe {
            std::env::remove_var("AUCTION_CLOSURE_SCHEDULER_INTERVAL_MS");
            std::env::remove_var("OUTBOX_SCHEDULER_INTERVAL_MS");
        }
        assert_eq!(resolve_auction_closure_interval_ms(), 1000);
        assert_eq!(resolve_outbox_interval_ms(), 1000);
    });
}

#[test]
fn resolve_rabbitmq_defaults() {
    with_env_lock(|| {
        unsafe {
            std::env::remove_var("RABBITMQ_URL");
            std::env::remove_var("BIDMART_EVENTS_EXCHANGE");
        }
        assert_eq!(resolve_rabbitmq_url(), "amqp://guest:guest@localhost:5672/%2f");
        assert_eq!(resolve_events_exchange(), "bidmart.events");
    });
}
