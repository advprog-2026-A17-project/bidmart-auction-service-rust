use std::sync::Arc;
use std::time::Duration;

use sqlx::AnyPool;

use bidmart_auction_service_rust::client::{
    HoldFundsRequest, HoldResponse, WalletClient, WalletClientError,
};
use bidmart_auction_service_rust::persistence::models::NewListingAuctionSessionRecord;
use bidmart_auction_service_rust::persistence::repositories::{
    ListingAuctionSessionRepository, BidRepository, OutboxRepository,
};
use bidmart_auction_service_rust::service::auction_service::AuctionService;

#[derive(Debug)]
struct DelayingWalletClient;

#[async_trait::async_trait]
impl WalletClient for DelayingWalletClient {
    async fn hold_funds(
        &self,
        request: HoldFundsRequest,
    ) -> Result<HoldResponse, WalletClientError> {
        if request.user_id == "slow-low-bidder" {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        Ok(HoldResponse {
            id: request.hold_id,
            status: "ACTIVE".to_string(),
            amount: request.amount,
        })
    }

    async fn release_hold(&self, _hold_id: &str) -> Result<(), WalletClientError> {
        Ok(())
    }

    async fn convert_hold_to_payment(&self, _hold_id: &str) -> Result<(), WalletClientError> {
        Ok(())
    }
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
async fn concurrent_bids_on_same_auction_do_not_overwrite_highest_bid_with_stale_state() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = AuctionService::new_with_wallet(
        listing_auction_session_repo.clone(),
        bid_repo,
        outbox_repo,
        Arc::new(DelayingWalletClient),
    );

    let auction_id = uuid::Uuid::new_v4().to_string();
    let now = 1_700_000_000i64;
    listing_auction_session_repo
        .insert(&NewListingAuctionSessionRecord {
            id: auction_id.clone(),
            listing_id: "listing-concurrent".to_string(),
            seller_id: "seller-concurrent".to_string(),
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

    let low_service = service.clone();
    let low_auction_id = auction_id.clone();
    let low_bid = tokio::spawn(async move {
        low_service
            .place_bid_and_persist(&low_auction_id, "slow-low-bidder", 1500, now + 10)
            .await
    });

    tokio::time::sleep(Duration::from_millis(5)).await;

    let high_service = service.clone();
    let high_auction_id = auction_id.clone();
    let high_bid = tokio::spawn(async move {
        high_service
            .place_bid_and_persist(&high_auction_id, "fast-high-bidder", 2000, now + 20)
            .await
    });

    low_bid.await.expect("join low bid").expect("low bid");
    high_bid.await.expect("join high bid").expect("high bid");

    let auction = listing_auction_session_repo
        .find_by_id(&auction_id)
        .await
        .expect("find auction")
        .expect("auction exists");
    assert_eq!(auction.current_highest_bid_cents, Some(2000));
}

#[tokio::test]
async fn separate_service_instances_do_not_overwrite_highest_bid_with_stale_state() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool.clone());
    let first_instance = AuctionService::new_with_wallet(
        listing_auction_session_repo.clone(),
        bid_repo.clone(),
        outbox_repo.clone(),
        Arc::new(DelayingWalletClient),
    );
    let second_instance = AuctionService::new_with_wallet(
        listing_auction_session_repo.clone(),
        bid_repo.clone(),
        outbox_repo,
        Arc::new(DelayingWalletClient),
    );

    let auction_id = uuid::Uuid::new_v4().to_string();
    let now = 1_700_000_000i64;
    listing_auction_session_repo
        .insert(&NewListingAuctionSessionRecord {
            id: auction_id.clone(),
            listing_id: "listing-db-concurrent".to_string(),
            seller_id: "seller-concurrent".to_string(),
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

    let low_auction_id = auction_id.clone();
    let low_bid = tokio::spawn(async move {
        first_instance
            .place_bid_and_persist(&low_auction_id, "slow-low-bidder", 1500, now + 10)
            .await
    });

    tokio::time::sleep(Duration::from_millis(5)).await;

    let high_auction_id = auction_id.clone();
    let high_bid = tokio::spawn(async move {
        second_instance
            .place_bid_and_persist(&high_auction_id, "fast-high-bidder", 2000, now + 20)
            .await
    });

    low_bid.await.expect("join low bid").expect("low bid");
    high_bid.await.expect("join high bid").expect("high bid");

    let auction = listing_auction_session_repo
        .find_by_id(&auction_id)
        .await
        .expect("find auction")
        .expect("auction exists");
    assert_eq!(auction.current_highest_bid_cents, Some(2000));
}

#[tokio::test]
async fn proxy_bid_auto_counters_later_standard_bid() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool.clone());
    let service = AuctionService::new_with_wallet(
        listing_auction_session_repo.clone(),
        bid_repo.clone(),
        outbox_repo,
        Arc::new(DelayingWalletClient),
    );

    let auction_id = uuid::Uuid::new_v4().to_string();
    let now = 1_700_000_000i64;
    listing_auction_session_repo
        .insert(&NewListingAuctionSessionRecord {
            id: auction_id.clone(),
            listing_id: "listing-proxy-auto-counter".to_string(),
            seller_id: "seller-concurrent".to_string(),
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
        .place_proxy_bid_and_persist(&auction_id, "proxy-a", 5000, now + 1)
        .await
        .expect("place proxy");
    service
        .place_bid_and_persist(&auction_id, "manual-b", 1400, now + 2)
        .await
        .expect("place manual bid");

    let top_bid = bid_repo
        .find_winning_bid(&auction_id)
        .await
        .expect("find winning bid")
        .expect("winning bid exists");
    assert_eq!(top_bid.bidder_id, "proxy-a");
    assert_eq!(top_bid.bid_amount_cents, 1600);
}

#[tokio::test]
async fn concurrent_proxy_bids_across_instances_resolve_deterministically() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool.clone());
    let first_instance = AuctionService::new_with_wallet(
        listing_auction_session_repo.clone(),
        bid_repo.clone(),
        outbox_repo.clone(),
        Arc::new(DelayingWalletClient),
    );
    let second_instance = AuctionService::new_with_wallet(
        listing_auction_session_repo.clone(),
        bid_repo.clone(),
        outbox_repo,
        Arc::new(DelayingWalletClient),
    );

    let auction_id = uuid::Uuid::new_v4().to_string();
    let now = 1_700_000_000i64;
    listing_auction_session_repo
        .insert(&NewListingAuctionSessionRecord {
            id: auction_id.clone(),
            listing_id: "listing-proxy-concurrent".to_string(),
            seller_id: "seller-concurrent".to_string(),
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

    let a_id = auction_id.clone();
    let first = tokio::spawn(async move {
        first_instance
            .place_proxy_bid_and_persist(&a_id, "proxy-a", 5000, now + 1)
            .await
    });
    let b_id = auction_id.clone();
    let second = tokio::spawn(async move {
        second_instance
            .place_proxy_bid_and_persist(&b_id, "proxy-b", 4800, now + 2)
            .await
    });

    first.await.expect("join first").expect("first proxy");
    second.await.expect("join second").expect("second proxy");

    let top_bid = bid_repo
        .find_winning_bid(&auction_id)
        .await
        .expect("find winning bid")
        .expect("winning bid exists");
    assert_eq!(top_bid.bidder_id, "proxy-a");
    assert_eq!(top_bid.bid_amount_cents, 5000);
}
