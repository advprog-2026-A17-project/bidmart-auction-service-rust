#[test]
fn production_readiness_document_covers_remaining_auction_decisions() {
    let document = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("docs/auction-production-readiness.md"),
    )
    .expect("read production readiness document");

    assert!(document.contains("Database-Backed Concurrency Control"));
    assert!(document.contains("Equal Bid Fairness"));
    assert!(document.contains("HTTP Outbox Relay"));
    assert!(document.contains("Idempotency"));
    assert!(document.contains("Production Migration Strategy"));
    assert!(document.contains("Load Testing"));
    assert!(document.contains("Domain Reconstruction"));
    assert!(document.contains("Auction Type Strategy"));
}
