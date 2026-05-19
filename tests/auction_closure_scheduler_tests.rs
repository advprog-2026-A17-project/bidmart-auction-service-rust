use sqlx::AnyPool;

use bidmart_auction_service_rust::persistence::models::NewListingAuctionSessionRecord;
use bidmart_auction_service_rust::persistence::repositories::{
    ListingAuctionSessionRepository, BidRepository, OutboxRepository,
};
use bidmart_auction_service_rust::scheduler::auction_closure_scheduler::AuctionClosureScheduler;
use bidmart_auction_service_rust::service::auction_service::AuctionService;

async fn setup_test_db() -> AnyPool {
    let pool = bidmart_auction_service_rust::server::connect_pool("sqlite::memory:")
        .await
        .expect("connect to in-memory db");

    let sql = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("migrations/20260428000000_init.sql"),
    )
    .expect("read migration");

    for statement in sql.split(';') {
        let trimmed = statement.trim();
        if !trimmed.is_empty() {
            sqlx::query(trimmed)
                .execute(&pool)
                .await
                .expect("execute migration");
        }
    }

    pool
}

#[tokio::test]
async fn closure_scheduler_closes_expired_unprocessed_auctions() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = AuctionService::new(listing_auction_session_repo.clone(), bid_repo, outbox_repo);
    let scheduler = AuctionClosureScheduler::new(service);

    let now = chrono::Utc::now().timestamp();
    listing_auction_session_repo
        .insert(&NewListingAuctionSessionRecord {
            id: "expired-unsold".to_string(),
            listing_id: "listing-expired".to_string(),
            seller_id: "seller-expired".to_string(),
            starting_price_cents: 1000,
            reserve_price_cents: 5000,
            current_highest_bid_cents: None,
            minimum_increment_cents: 200,
            status: "ACTIVE".to_string(),
            start_time: now - 600,
            end_time: now - 1,
            created_at: now - 600,
            updated_at: now - 1,
        })
        .await
        .expect("insert expired auction");

    let report = scheduler.close_pending().await.expect("close pending");

    assert_eq!(report.attempted, 1);
    assert_eq!(report.closed, 1);
    assert_eq!(report.failed, 0);
    let auction = listing_auction_session_repo
        .find_by_id("expired-unsold")
        .await
        .expect("find auction")
        .expect("auction exists");
    assert_eq!(auction.status, "UNSOLD");
}
