//! Tests for OutboxScheduler types and HttpOutboxPublisher error paths.

use bidmart_auction_service_rust::scheduler::outbox_scheduler::*;
use bidmart_auction_service_rust::scheduler::auction_closure_scheduler::*;

// ============================
// OutboxPublishError
// ============================

#[test]
fn outbox_publish_error_display() {
    let e = OutboxPublishError::new("connection refused");
    assert!(format!("{e}").contains("connection refused"));
}

#[test]
fn outbox_publish_error_debug() {
    let e = OutboxPublishError::new("timeout");
    assert!(format!("{e:?}").contains("timeout"));
}

// ============================
// OutboxSchedulerError
// ============================

#[test]
fn outbox_scheduler_error_display() {
    let e = OutboxSchedulerError::DatabaseError("db failed".into());
    assert!(format!("{e}").contains("Database"));
}

// ============================
// OutboxPublishReport
// ============================

#[test]
fn outbox_publish_report_struct() {
    let r = OutboxPublishReport {
        attempted: 5,
        published: 3,
        failed: 2,
    };
    assert_eq!(r.attempted, 5);
    assert_eq!(r.published, 3);
    assert_eq!(r.failed, 2);
}

// ============================
// HttpOutboxPublisher construction
// ============================

#[test]
fn http_outbox_publisher_invalid_url() {
    let result = HttpOutboxPublisher::new("not-a-url", "/events");
    assert!(result.is_err());
}

#[test]
fn http_outbox_publisher_valid_url() {
    let result = HttpOutboxPublisher::new("http://localhost:8080", "/events");
    assert!(result.is_ok());
}

// ============================
// AuctionClosureReport
// ============================

#[test]
fn auction_closure_report_struct() {
    let r = AuctionClosureReport {
        attempted: 10,
        closed: 8,
        failed: 2,
    };
    assert_eq!(r.attempted, 10);
    assert_eq!(r.closed, 8);
    assert_eq!(r.failed, 2);
}

// ============================
// AuctionClosureSchedulerError
// ============================

#[test]
fn closure_scheduler_error_display() {
    let inner = bidmart_auction_service_rust::service::auction_service::CloseListingAuctionSessionError::AuctionNotFound;
    let e = AuctionClosureSchedulerError::CloseAuction(inner);
    assert!(format!("{e}").contains("Close auction"));
}
