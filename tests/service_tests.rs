use sqlx::SqlitePool;
use uuid::Uuid;

use bidmart_auction_service_rust::persistence::models::NewAuctionRecord;
use bidmart_auction_service_rust::persistence::repositories::{AuctionRepository, BidRepository, OutboxRepository};
use bidmart_auction_service_rust::service::auction_service::AuctionService;

async fn setup_test_db() -> SqlitePool {
    let pool = SqlitePool::connect("sqlite::memory:")
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
async fn test_service_place_bid() {
    let pool = setup_test_db().await;
    let auction_repo = AuctionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);

    let service = AuctionService::new(auction_repo.clone(), bid_repo.clone(), outbox_repo.clone());

    // Create test auction
    let auction_id = Uuid::new_v4().to_string();
    let now = 1_700_000_000i64;

    let new_auction = NewAuctionRecord {
        id: auction_id.clone(),
        listing_id: "listing-1".to_string(),
        seller_id: "seller-1".to_string(),
        starting_price_cents: 1000,
        reserve_price_cents: 5000,
        current_highest_bid_cents: None,
        minimum_increment_cents: 200,
        status: "ACTIVE".to_string(),
        start_time: now,
        end_time: now + 300,
        created_at: now,
        updated_at: now,
    };
    auction_repo
        .insert(&new_auction)
        .await
        .expect("insert auction");

    // Place bid via service
    let result = service
        .place_bid_and_persist(
            &auction_id,
            "user-1",
            1500,
            now + 10,
        )
        .await;

    assert!(result.is_ok());

    // Verify bid was persisted
    let bids = bid_repo
        .list_by_auction_id_desc(&auction_id)
        .await
        .expect("list bids");
    assert_eq!(bids.len(), 1);
    assert_eq!(bids[0].bid_amount_cents, 1500);

    // Verify event was published to outbox
    let events = outbox_repo
        .list_pending(10)
        .await
        .expect("list outbox");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "BidPlaced");
}

#[tokio::test]
async fn test_service_place_bid_on_nonexistent_auction() {
    let pool = setup_test_db().await;
    let auction_repo = AuctionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);

    let service = AuctionService::new(auction_repo, bid_repo, outbox_repo);

    let result = service
        .place_bid_and_persist(
            "nonexistent-auction",
            "user-1",
            1500,
            1_700_000_010i64,
        )
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_service_get_auction_with_bids() {
    let pool = setup_test_db().await;
    let auction_repo = AuctionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);

    let service = AuctionService::new(auction_repo.clone(), bid_repo.clone(), outbox_repo);

    // Create test auction
    let auction_id = Uuid::new_v4().to_string();
    let now = 1_700_000_000i64;

    let new_auction = NewAuctionRecord {
        id: auction_id.clone(),
        listing_id: "listing-1".to_string(),
        seller_id: "seller-1".to_string(),
        starting_price_cents: 1000,
        reserve_price_cents: 5000,
        current_highest_bid_cents: None,
        minimum_increment_cents: 200,
        status: "ACTIVE".to_string(),
        start_time: now,
        end_time: now + 300,
        created_at: now,
        updated_at: now,
    };
    auction_repo
        .insert(&new_auction)
        .await
        .expect("insert auction");

    // Place bids
    service
        .place_bid_and_persist(&auction_id, "user-1", 1500, now + 10)
        .await
        .expect("place bid 1");

    service
        .place_bid_and_persist(&auction_id, "user-2", 2000, now + 20)
        .await
        .expect("place bid 2");

    // Get auction with bids
    let result = service
        .get_auction_with_bids(&auction_id)
        .await
        .expect("get auction");

    assert!(result.is_some());
    let (id, bid_ids) = result.unwrap();
    assert_eq!(id, auction_id);
    assert_eq!(bid_ids.len(), 2);
}
