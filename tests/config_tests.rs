//! Tests for config module — covering env var resolution fallback paths.

use bidmart_auction_service_rust::config;

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
