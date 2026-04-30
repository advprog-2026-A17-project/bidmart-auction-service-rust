use sqlx::SqlitePool;
use uuid::Uuid;

use bidmart_auction_service_rust::persistence::models::{NewAuctionRecord, NewBidRecord, NewOutboxEventRecord};
use bidmart_auction_service_rust::persistence::repositories::{AuctionRepository, BidRepository, OutboxRepository};

async fn setup_test_db() -> SqlitePool {
    // Use in-memory SQLite database
    let pool = SqlitePool::connect("sqlite::memory:")
        .await
        .expect("connect to in-memory db");

    // Read and apply migrations
    let sql = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("migrations/20260428000000_init.sql"),
    )
    .expect("read migration");
    
    // Split and execute each statement
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
async fn test_insert_and_find_auction() {
    let pool = setup_test_db().await;
    let repo = AuctionRepository::new(pool);

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

    let inserted = repo.insert(&new_auction).await.expect("insert auction");
    assert_eq!(inserted.id, auction_id);
    assert_eq!(inserted.seller_id, "seller-1");
    assert_eq!(inserted.status, "ACTIVE");

    let found = repo.find_by_id(&auction_id).await.expect("find auction");
    assert!(found.is_some());
    assert_eq!(found.unwrap().id, auction_id);
}

#[tokio::test]
async fn test_insert_and_list_bids() {
    let pool = setup_test_db().await;
    let auction_repo = AuctionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());

    let auction_id = Uuid::new_v4().to_string();
    let now = 1_700_000_000i64;

    // Create auction first
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
    auction_repo.insert(&new_auction).await.expect("insert auction");

    // Insert bids
    let bid1_id = Uuid::new_v4().to_string();
    let bid1 = NewBidRecord {
        id: bid1_id.clone(),
        auction_id: auction_id.clone(),
        bidder_id: "user-1".to_string(),
        bid_amount_cents: 1500,
        bid_time: now + 10,
    };
    bid_repo.insert(&bid1).await.expect("insert bid 1");

    let bid2_id = Uuid::new_v4().to_string();
    let bid2 = NewBidRecord {
        id: bid2_id.clone(),
        auction_id: auction_id.clone(),
        bidder_id: "user-2".to_string(),
        bid_amount_cents: 2000,
        bid_time: now + 20,
    };
    bid_repo.insert(&bid2).await.expect("insert bid 2");

    // List bids (should be ordered DESC by amount)
    let bids = bid_repo
        .list_by_auction_id_desc(&auction_id)
        .await
        .expect("list bids");
    assert_eq!(bids.len(), 2);
    assert_eq!(bids[0].bid_amount_cents, 2000); // Higher bid first

    // Find winning bid
    let winning = bid_repo
        .find_winning_bid(&auction_id)
        .await
        .expect("find winning");
    assert!(winning.is_some());
    assert_eq!(winning.unwrap().bid_amount_cents, 2000);
}

#[tokio::test]
async fn test_winning_bid_tie_breaks_equal_amount_and_time_by_bid_id() {
    let pool = setup_test_db().await;
    let auction_repo = AuctionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool);

    let auction_id = Uuid::new_v4().to_string();
    let now = 1_700_000_000i64;

    auction_repo
        .insert(&NewAuctionRecord {
            id: auction_id.clone(),
            listing_id: "listing-fairness".to_string(),
            seller_id: "seller-1".to_string(),
            starting_price_cents: 1000,
            reserve_price_cents: 1500,
            current_highest_bid_cents: None,
            minimum_increment_cents: 100,
            status: "ACTIVE".to_string(),
            start_time: now,
            end_time: now + 300,
            created_at: now,
            updated_at: now,
        })
        .await
        .expect("insert auction");

    bid_repo
        .insert(&NewBidRecord {
            id: "bid-b".to_string(),
            auction_id: auction_id.clone(),
            bidder_id: "bidder-b".to_string(),
            bid_amount_cents: 1500,
            bid_time: now + 10,
        })
        .await
        .expect("insert second lexical bid first");
    bid_repo
        .insert(&NewBidRecord {
            id: "bid-a".to_string(),
            auction_id: auction_id.clone(),
            bidder_id: "bidder-a".to_string(),
            bid_amount_cents: 1500,
            bid_time: now + 10,
        })
        .await
        .expect("insert first lexical bid second");

    let winning = bid_repo
        .find_winning_bid(&auction_id)
        .await
        .expect("find winning")
        .expect("winning bid");

    assert_eq!(winning.id, "bid-a");
    assert_eq!(winning.bidder_id, "bidder-a");
}

#[tokio::test]
async fn test_outbox_insert_list_and_mark_published() {
    let pool = setup_test_db().await;
    let outbox_repo = OutboxRepository::new(pool);

    let now = 1_700_000_000i64;
    let event_id1 = Uuid::new_v4().to_string();
    let event_id2 = Uuid::new_v4().to_string();
    let auction_id = Uuid::new_v4().to_string();

    // Insert pending events
    let new_event1 = NewOutboxEventRecord {
        id: event_id1.clone(),
        aggregate_id: auction_id.clone(),
        event_type: "BidPlaced".to_string(),
        payload: r#"{"auction_id":"auction-1","bidder_id":"user-1"}"#.to_string(),
        published: false,
        published_at: None,
        created_at: now,
        updated_at: now,
    };

    let inserted1 = outbox_repo
        .insert(&new_event1)
        .await
        .expect("insert event 1");
    assert_eq!(inserted1.id, event_id1);
    assert!(!inserted1.published);

    let new_event2 = NewOutboxEventRecord {
        id: event_id2.clone(),
        aggregate_id: auction_id.clone(),
        event_type: "AuctionEnded".to_string(),
        payload: r#"{"auction_id":"auction-1"}"#.to_string(),
        published: false,
        published_at: None,
        created_at: now + 1,
        updated_at: now + 1,
    };

    outbox_repo
        .insert(&new_event2)
        .await
        .expect("insert event 2");

    // List pending events (should be in created_at order)
    let pending = outbox_repo
        .list_pending(10)
        .await
        .expect("list pending");
    assert_eq!(pending.len(), 2);
    assert_eq!(pending[0].id, event_id1); // First inserted
    assert_eq!(pending[1].id, event_id2); // Second inserted

    // Mark first event as published
    outbox_repo
        .mark_published(&event_id1, now + 100)
        .await
        .expect("mark published");

    // List pending again (should only have event2)
    let pending_after = outbox_repo
        .list_pending(10)
        .await
        .expect("list pending after");
    assert_eq!(pending_after.len(), 1);
    assert_eq!(pending_after[0].id, event_id2);
}
