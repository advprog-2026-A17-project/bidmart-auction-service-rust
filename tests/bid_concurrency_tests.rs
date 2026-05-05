use std::sync::Arc;
use std::time::Duration;

use sqlx::SqlitePool;

use bidmart_auction_service_rust::client::{
    HoldFundsRequest, HoldResponse, WalletClient, WalletClientError,
};
use bidmart_auction_service_rust::persistence::models::NewAuctionRecord;
use bidmart_auction_service_rust::persistence::repositories::{
    AuctionRepository, BidRepository, OutboxRepository,
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
async fn concurrent_bids_on_same_auction_do_not_overwrite_highest_bid_with_stale_state() {
    let pool = setup_test_db().await;
    let auction_repo = AuctionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = AuctionService::new_with_wallet(
        auction_repo.clone(),
        bid_repo,
        outbox_repo,
        Arc::new(DelayingWalletClient),
    );

    let auction_id = uuid::Uuid::new_v4().to_string();
    let now = 1_700_000_000i64;
    auction_repo
        .insert(&NewAuctionRecord {
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

    let auction = auction_repo
        .find_by_id(&auction_id)
        .await
        .expect("find auction")
        .expect("auction exists");
    assert_eq!(auction.current_highest_bid_cents, Some(2000));
}

#[tokio::test]
async fn separate_service_instances_do_not_overwrite_highest_bid_with_stale_state() {
    let pool = setup_test_db().await;
    let auction_repo = AuctionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool.clone());
    let first_instance = AuctionService::new_with_wallet(
        auction_repo.clone(),
        bid_repo.clone(),
        outbox_repo.clone(),
        Arc::new(DelayingWalletClient),
    );
    let second_instance = AuctionService::new_with_wallet(
        auction_repo.clone(),
        bid_repo.clone(),
        outbox_repo,
        Arc::new(DelayingWalletClient),
    );

    let auction_id = uuid::Uuid::new_v4().to_string();
    let now = 1_700_000_000i64;
    auction_repo
        .insert(&NewAuctionRecord {
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

    let auction = auction_repo
        .find_by_id(&auction_id)
        .await
        .expect("find auction")
        .expect("auction exists");
    assert_eq!(auction.current_highest_bid_cents, Some(2000));
}
