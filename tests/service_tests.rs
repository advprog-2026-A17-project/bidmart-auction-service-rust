use sqlx::AnyPool;
use uuid::Uuid;

use bidmart_auction_service_rust::persistence::models::{
    NewBidRecord, NewListingAuctionSessionRecord,
};
use bidmart_auction_service_rust::persistence::repositories::{
    BidRepository, ListingAuctionSessionRepository, OutboxRepository,
};
use bidmart_auction_service_rust::service::auction_service::AuctionService;

mod common;
use common::always_active_catalog;

fn test_auction_service(
    listing_auction_session_repo: ListingAuctionSessionRepository,
    bid_repo: BidRepository,
    outbox_repo: OutboxRepository,
) -> AuctionService {
    AuctionService::new_with_catalog(
        listing_auction_session_repo,
        bid_repo,
        outbox_repo,
        always_active_catalog(),
    )
}

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
async fn test_service_place_bid() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);

    let service = test_auction_service(
        listing_auction_session_repo.clone(),
        bid_repo.clone(),
        outbox_repo.clone(),
    );

    // Create test auction
    let auction_id = Uuid::new_v4().to_string();
    let now = 1_700_000_000i64;

    let new_auction = NewListingAuctionSessionRecord {
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
    listing_auction_session_repo
        .insert(&new_auction)
        .await
        .expect("insert auction");

    // Place bid via service
    let result = service
        .place_bid_and_persist(&auction_id, "user-1", 1500, now + 10)
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
    let events = outbox_repo.list_pending(10).await.expect("list outbox");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "BidPlaced");
    assert!(events[0].payload.contains("\"listingId\":\"listing-1\""));
    assert!(events[0].payload.contains("\"sellerId\":\"seller-1\""));
    assert!(events[0].payload.contains("\"amountCents\":1500"));
}

#[tokio::test]
async fn place_bid_publishes_outbid_event_when_previous_bidder_outbid() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = test_auction_service(
        listing_auction_session_repo.clone(),
        bid_repo.clone(),
        outbox_repo.clone(),
    );

    let auction_id = Uuid::new_v4().to_string();
    let now = 1_700_000_000i64;

    listing_auction_session_repo
        .insert(&NewListingAuctionSessionRecord {
            id: auction_id.clone(),
            listing_id: "listing-outbid".to_string(),
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
        })
        .await
        .expect("insert auction");

    service
        .place_bid_and_persist(&auction_id, "user-1", 1500, now + 10)
        .await
        .expect("first bid");
    service
        .place_bid_and_persist(&auction_id, "user-2", 1700, now + 20)
        .await
        .expect("outbid bid");

    let events = outbox_repo.list_pending(10).await.expect("list outbox");
    let outbid_events: Vec<_> = events
        .iter()
        .filter(|event| event.event_type == "Outbid")
        .collect();
    assert_eq!(outbid_events.len(), 1);
    assert!(
        outbid_events[0]
            .payload
            .contains("\"sellerId\":\"seller-1\"")
    );
    assert!(
        outbid_events[0]
            .payload
            .contains("\"previousBidderId\":\"user-1\"")
    );
    assert!(outbid_events[0].payload.contains("\"amountCents\":1700"));
}

#[tokio::test]
async fn place_bid_retry_with_same_bid_details_is_idempotent() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = test_auction_service(
        listing_auction_session_repo.clone(),
        bid_repo.clone(),
        outbox_repo.clone(),
    );

    let auction_id = Uuid::new_v4().to_string();
    let now = 1_700_000_000i64;

    listing_auction_session_repo
        .insert(&NewListingAuctionSessionRecord {
            id: auction_id.clone(),
            listing_id: "listing-idempotent".to_string(),
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
        })
        .await
        .expect("insert auction");

    let first = service
        .place_bid_and_persist(&auction_id, "user-1", 1500, now + 10)
        .await
        .expect("first bid");
    let retry = service
        .place_bid_and_persist(&auction_id, "user-1", 1500, now + 10)
        .await
        .expect("retry bid");

    assert_eq!(retry.id, first.id);
    let bids = bid_repo
        .list_by_auction_id_desc(&auction_id)
        .await
        .expect("list bids");
    assert_eq!(bids.len(), 1);
    let events = outbox_repo.list_pending(10).await.expect("list outbox");
    assert_eq!(events.len(), 1);
}

#[tokio::test]
async fn test_service_place_bid_on_nonexistent_auction() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);

    let service = test_auction_service(listing_auction_session_repo, bid_repo, outbox_repo);

    let result = service
        .place_bid_and_persist("nonexistent-auction", "user-1", 1500, 1_700_000_010i64)
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_service_get_auction_with_bids() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);

    let service = test_auction_service(
        listing_auction_session_repo.clone(),
        bid_repo.clone(),
        outbox_repo,
    );

    // Create test auction
    let auction_id = Uuid::new_v4().to_string();
    let now = 1_700_000_000i64;

    let new_auction = NewListingAuctionSessionRecord {
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
    listing_auction_session_repo
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

#[tokio::test]
async fn test_place_bid_rejects_bid_below_starting_price() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);

    let service = test_auction_service(
        listing_auction_session_repo.clone(),
        bid_repo.clone(),
        outbox_repo,
    );

    let auction_id = Uuid::new_v4().to_string();
    let now = 1_700_000_000i64;

    let new_auction = NewListingAuctionSessionRecord {
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
    listing_auction_session_repo
        .insert(&new_auction)
        .await
        .expect("insert");

    // Attempt to bid below starting price
    let result = service
        .place_bid_and_persist(&auction_id, "user-1", 500, now + 10)
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("minimum"));
}

#[tokio::test]
async fn test_place_bid_rejects_bid_below_previous_plus_increment() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);

    let service = test_auction_service(
        listing_auction_session_repo.clone(),
        bid_repo.clone(),
        outbox_repo,
    );

    let auction_id = Uuid::new_v4().to_string();
    let now = 1_700_000_000i64;

    let new_auction = NewListingAuctionSessionRecord {
        id: auction_id.clone(),
        listing_id: "listing-1".to_string(),
        seller_id: "seller-1".to_string(),
        starting_price_cents: 1000,
        reserve_price_cents: 5000,
        current_highest_bid_cents: Some(2000),
        minimum_increment_cents: 200,
        status: "ACTIVE".to_string(),
        start_time: now,
        end_time: now + 300,
        created_at: now,
        updated_at: now,
    };
    listing_auction_session_repo
        .insert(&new_auction)
        .await
        .expect("insert");

    // First bid (valid)
    service
        .place_bid_and_persist(&auction_id, "user-1", 2000, now + 10)
        .await
        .expect("first bid");

    // Second bid below required minimum (2000 + 200 = 2200)
    let result = service
        .place_bid_and_persist(&auction_id, "user-2", 2100, now + 20)
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("minimum"));
}

#[tokio::test]
async fn test_place_bid_rejects_seller_bidding() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);

    let service = test_auction_service(
        listing_auction_session_repo.clone(),
        bid_repo.clone(),
        outbox_repo,
    );

    let auction_id = Uuid::new_v4().to_string();
    let now = 1_700_000_000i64;

    let new_auction = NewListingAuctionSessionRecord {
        id: auction_id.clone(),
        listing_id: "listing-1".to_string(),
        seller_id: "seller-1".to_string(),
        starting_price_cents: 1000,
        reserve_price_cents: 5000,
        current_highest_bid_cents: Some(1500),
        minimum_increment_cents: 200,
        status: "ACTIVE".to_string(),
        start_time: now,
        end_time: now + 300,
        created_at: now,
        updated_at: now,
    };
    listing_auction_session_repo
        .insert(&new_auction)
        .await
        .expect("insert");

    // Place first bid as user-1
    service
        .place_bid_and_persist(&auction_id, "user-1", 1500, now + 10)
        .await
        .expect("first bid");

    // Attempt to place bid as seller-1 (should fail)
    let result = service
        .place_bid_and_persist(&auction_id, "seller-1", 1700, now + 20)
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_place_proxy_bid_rejects_seller_bidding() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);

    let service = test_auction_service(
        listing_auction_session_repo.clone(),
        bid_repo.clone(),
        outbox_repo,
    );

    let auction_id = Uuid::new_v4().to_string();
    let now = 1_700_000_000i64;

    let new_auction = NewListingAuctionSessionRecord {
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
    listing_auction_session_repo
        .insert(&new_auction)
        .await
        .expect("insert");

    let result = service
        .place_proxy_bid_and_persist(&auction_id, "seller-1", 10_000, now + 20)
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_place_bid_triggers_anti_sniping_extension() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);

    let service = test_auction_service(
        listing_auction_session_repo.clone(),
        bid_repo.clone(),
        outbox_repo,
    );

    let auction_id = Uuid::new_v4().to_string();
    let now = 1_700_000_000i64;
    let end_time = now + 300;

    let new_auction = NewListingAuctionSessionRecord {
        id: auction_id.clone(),
        listing_id: "listing-1".to_string(),
        seller_id: "seller-1".to_string(),
        starting_price_cents: 1000,
        reserve_price_cents: 5000,
        current_highest_bid_cents: None,
        minimum_increment_cents: 200,
        status: "ACTIVE".to_string(),
        start_time: now,
        end_time,
        created_at: now,
        updated_at: now,
    };
    listing_auction_session_repo
        .insert(&new_auction)
        .await
        .expect("insert");

    // Bid within last 2 minutes (should extend)
    let bid_time = end_time - 100; // 100 seconds before end (within 120-second window)
    service
        .place_bid_and_persist(&auction_id, "user-1", 1500, bid_time)
        .await
        .expect("place bid");

    // Verify auction end_time was extended
    let updated_auction = listing_auction_session_repo
        .find_by_id(&auction_id)
        .await
        .expect("get auction")
        .expect("auction exists");

    // Should be extended by 120 seconds from bid_time
    assert!(updated_auction.end_time > end_time);
    assert_eq!(updated_auction.status, "EXTENDED");
}

#[tokio::test]
async fn close_auction_publishes_auction_ended_outbox_event() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = test_auction_service(
        listing_auction_session_repo.clone(),
        bid_repo.clone(),
        outbox_repo.clone(),
    );

    let auction_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    listing_auction_session_repo
        .insert(&NewListingAuctionSessionRecord {
            id: auction_id.clone(),
            listing_id: "listing-ended".to_string(),
            seller_id: "seller-ended".to_string(),
            starting_price_cents: 1000,
            reserve_price_cents: 1500,
            current_highest_bid_cents: Some(2000),
            minimum_increment_cents: 200,
            status: "ACTIVE".to_string(),
            start_time: now - 600,
            end_time: now - 1,
            created_at: now - 600,
            updated_at: now - 1,
        })
        .await
        .expect("insert auction");
    bid_repo
        .insert(&NewBidRecord {
            id: Uuid::new_v4().to_string(),
            auction_id: auction_id.clone(),
            bidder_id: "winner".to_string(),
            bid_amount_cents: 2000,
            bid_time: now - 10,
        })
        .await
        .expect("insert bid");

    let closed = service.close_auction(&auction_id).await.expect("close");

    assert_eq!(closed.status, "WON");
    let events = outbox_repo.list_pending(10).await.expect("list events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].aggregate_id, auction_id);
    assert_eq!(events[0].event_type, "AuctionEnded");
    assert!(events[0].payload.contains("\"status\":\"WON\""));
    assert!(events[0].payload.contains("\"winnerId\":\"winner\""));
}

#[tokio::test]
async fn close_auction_unsold_publishes_auction_ended_without_winner_id() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = test_auction_service(
        listing_auction_session_repo.clone(),
        bid_repo.clone(),
        outbox_repo.clone(),
    );

    let auction_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    listing_auction_session_repo
        .insert(&NewListingAuctionSessionRecord {
            id: auction_id.clone(),
            listing_id: "listing-unsold".to_string(),
            seller_id: "seller-unsold".to_string(),
            starting_price_cents: 1000,
            reserve_price_cents: 5000,
            current_highest_bid_cents: Some(2000),
            minimum_increment_cents: 200,
            status: "ACTIVE".to_string(),
            start_time: now - 600,
            end_time: now - 1,
            created_at: now - 600,
            updated_at: now - 1,
        })
        .await
        .expect("insert auction");
    bid_repo
        .insert(&NewBidRecord {
            id: Uuid::new_v4().to_string(),
            auction_id: auction_id.clone(),
            bidder_id: "bidder-below-reserve".to_string(),
            bid_amount_cents: 2000,
            bid_time: now - 10,
        })
        .await
        .expect("insert bid");

    let closed = service.close_auction(&auction_id).await.expect("close");

    assert_eq!(closed.status, "UNSOLD");
    let events = outbox_repo.list_pending(10).await.expect("list events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].aggregate_id, auction_id);
    assert_eq!(events[0].event_type, "AuctionEnded");
    assert!(events[0].payload.contains("\"status\":\"UNSOLD\""));
    assert!(events[0].payload.contains("\"reserveMet\":false"));
    assert!(!events[0].payload.contains("winnerId"));
}
