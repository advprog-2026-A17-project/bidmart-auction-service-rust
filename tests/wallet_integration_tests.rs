use bidmart_auction_service_rust::client::{
    HoldFundsRequest, HoldResponse, WalletClient, WalletClientError,
};
use bidmart_auction_service_rust::persistence::models::NewAuctionRecord;
use bidmart_auction_service_rust::persistence::repositories::{
    AuctionRepository, BidRepository, OutboxRepository,
};
use bidmart_auction_service_rust::service::auction_service::AuctionService;
use sqlx::AnyPool;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub struct MockWalletClient {
    holds: Arc<Mutex<Vec<HoldFundsRequest>>>,
    released_holds: Arc<Mutex<Vec<String>>>,
    converted_holds: Arc<Mutex<Vec<String>>>,
    fail_next_hold: Arc<Mutex<bool>>,
    drop_bids_on_hold: Arc<Mutex<Option<AnyPool>>>,
}

impl MockWalletClient {
    pub fn new() -> Self {
        Self {
            holds: Arc::new(Mutex::new(Vec::new())),
            released_holds: Arc::new(Mutex::new(Vec::new())),
            converted_holds: Arc::new(Mutex::new(Vec::new())),
            fail_next_hold: Arc::new(Mutex::new(false)),
            drop_bids_on_hold: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_fail_next_hold(&self) {
        *self.fail_next_hold.lock().unwrap() = true;
    }

    pub fn set_drop_bids_on_hold(&self, pool: AnyPool) {
        *self.drop_bids_on_hold.lock().unwrap() = Some(pool);
    }

    pub fn get_holds(&self) -> Vec<HoldFundsRequest> {
        self.holds.lock().unwrap().clone()
    }

    pub fn get_released_holds(&self) -> Vec<String> {
        self.released_holds.lock().unwrap().clone()
    }

    pub fn get_converted_holds(&self) -> Vec<String> {
        self.converted_holds.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl WalletClient for MockWalletClient {
    async fn hold_funds(
        &self,
        request: HoldFundsRequest,
    ) -> Result<HoldResponse, WalletClientError> {
        if *self.fail_next_hold.lock().unwrap() {
            *self.fail_next_hold.lock().unwrap() = false;
            return Err(WalletClientError::InsufficientBalance(
                "Not enough balance".to_string(),
            ));
        }
        let response = HoldResponse {
            id: request.hold_id.clone(),
            status: "ACTIVE".to_string(),
            amount: request.amount,
        };

        self.holds.lock().unwrap().push(request);
        let drop_pool = self.drop_bids_on_hold.lock().unwrap().clone();
        if let Some(pool) = drop_pool {
            sqlx::query("DROP TABLE bids")
                .execute(&pool)
                .await
                .map_err(|error| WalletClientError::ServiceError(error.to_string()))?;
        }
        Ok(response)
    }

    async fn release_hold(&self, hold_id: &str) -> Result<(), WalletClientError> {
        let mut holds = self.holds.lock().unwrap();
        holds.retain(|h| h.hold_id != hold_id);
        self.released_holds
            .lock()
            .unwrap()
            .push(hold_id.to_string());
        Ok(())
    }

    async fn convert_hold_to_payment(&self, hold_id: &str) -> Result<(), WalletClientError> {
        let mut holds = self.holds.lock().unwrap();
        holds.retain(|h| h.hold_id != hold_id);
        self.converted_holds
            .lock()
            .unwrap()
            .push(hold_id.to_string());
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
async fn test_place_bid_holds_funds_from_wallet() {
    let pool = setup_test_db().await;
    let auction_repo = AuctionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);

    let wallet_client = Arc::new(MockWalletClient::new());
    let service = AuctionService::new_with_wallet(
        auction_repo.clone(),
        bid_repo.clone(),
        outbox_repo,
        wallet_client.clone(),
    );

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
    auction_repo.insert(&new_auction).await.expect("insert");

    let result = service
        .place_bid_and_persist(&auction_id, "user-1", 1500, now + 10)
        .await;

    let bid = result.expect("bid should be placed");

    // Verify wallet was called to hold funds
    let holds = wallet_client.get_holds();
    assert_eq!(holds.len(), 1);
    assert_eq!(holds[0].user_id, "user-1");
    assert_eq!(holds[0].bid_id, bid.id);
    assert_eq!(holds[0].amount, 1500);
}

#[tokio::test]
async fn test_place_bid_releases_wallet_hold_when_bid_persistence_fails() {
    let pool = setup_test_db().await;
    let auction_repo = AuctionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool.clone());

    let wallet_client = Arc::new(MockWalletClient::new());
    wallet_client.set_drop_bids_on_hold(pool);

    let service = AuctionService::new_with_wallet(
        auction_repo.clone(),
        bid_repo,
        outbox_repo,
        wallet_client.clone(),
    );

    let auction_id = Uuid::new_v4().to_string();
    let now = 1_700_000_000i64;

    let new_auction = NewAuctionRecord {
        id: auction_id.clone(),
        listing_id: "listing-compensation".to_string(),
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
    auction_repo.insert(&new_auction).await.expect("insert");

    let result = service
        .place_bid_and_persist(&auction_id, "user-1", 1500, now + 10)
        .await;

    assert!(result.is_err());
    assert!(wallet_client.get_holds().is_empty());
    assert_eq!(wallet_client.get_released_holds().len(), 1);
}

#[tokio::test]
async fn test_place_bid_rejected_when_wallet_insufficient_balance() {
    let pool = setup_test_db().await;
    let auction_repo = AuctionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);

    let wallet_client = Arc::new(MockWalletClient::new());
    wallet_client.set_fail_next_hold();

    let service = AuctionService::new_with_wallet(
        auction_repo.clone(),
        bid_repo.clone(),
        outbox_repo,
        wallet_client.clone(),
    );

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
    auction_repo.insert(&new_auction).await.expect("insert");

    let result = service
        .place_bid_and_persist(&auction_id, "user-1", 1500, now + 10)
        .await;

    assert!(result.is_err());

    // Verify bid was not persisted
    let bids = bid_repo
        .list_by_auction_id_desc(&auction_id)
        .await
        .expect("list bids");
    assert_eq!(bids.len(), 0);
}

#[tokio::test]
async fn test_place_bid_persists_hold_id_and_releases_previous_highest_hold() {
    let pool = setup_test_db().await;
    let auction_repo = AuctionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);

    let wallet_client = Arc::new(MockWalletClient::new());
    let service = AuctionService::new_with_wallet(
        auction_repo.clone(),
        bid_repo.clone(),
        outbox_repo,
        wallet_client.clone(),
    );

    let auction_id = Uuid::new_v4().to_string();
    let now = 1_700_000_000i64;

    auction_repo
        .insert(&NewAuctionRecord {
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
        })
        .await
        .expect("insert auction");

    service
        .place_bid_and_persist(&auction_id, "user-1", 1500, now + 10)
        .await
        .expect("first bid");
    let first_hold = wallet_client.get_holds()[0].hold_id.clone();

    service
        .place_bid_and_persist(&auction_id, "user-2", 2000, now + 20)
        .await
        .expect("second bid");

    let bids = bid_repo
        .list_by_auction_id_desc(&auction_id)
        .await
        .expect("list bids");
    assert_eq!(bids.len(), 2);
    assert!(bids[0].wallet_hold_id.is_some());
    assert!(bids[1].wallet_hold_id.is_some());
    assert_eq!(wallet_client.get_released_holds(), vec![first_hold]);
    assert_eq!(wallet_client.get_holds().len(), 1);
    assert_eq!(wallet_client.get_holds()[0].user_id, "user-2");
}

#[tokio::test]
async fn test_close_won_auction_converts_winning_hold() {
    let pool = setup_test_db().await;
    let auction_repo = AuctionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);

    let wallet_client = Arc::new(MockWalletClient::new());
    let service = AuctionService::new_with_wallet(
        auction_repo.clone(),
        bid_repo.clone(),
        outbox_repo,
        wallet_client.clone(),
    );

    let auction_id = Uuid::new_v4().to_string();
    let now = 1_700_000_000i64;

    auction_repo
        .insert(&NewAuctionRecord {
            id: auction_id.clone(),
            listing_id: "listing-close".to_string(),
            seller_id: "seller-close".to_string(),
            starting_price_cents: 1000,
            reserve_price_cents: 1500,
            current_highest_bid_cents: None,
            minimum_increment_cents: 200,
            status: "ACTIVE".to_string(),
            start_time: now - 600,
            end_time: now + 300,
            created_at: now - 600,
            updated_at: now - 600,
        })
        .await
        .expect("insert auction");

    service
        .place_bid_and_persist(&auction_id, "winner", 1700, now + 10)
        .await
        .expect("winning bid");
    let winning_hold = wallet_client.get_holds()[0].hold_id.clone();

    sqlx::query("UPDATE auctions SET end_time = ? WHERE id = ?")
        .bind(now - 1)
        .bind(&auction_id)
        .execute(&auction_repo.pool)
        .await
        .expect("expire auction");

    let closed = service
        .close_auction(&auction_id)
        .await
        .expect("close auction");

    assert_eq!(closed.status, "WON");
    assert_eq!(wallet_client.get_converted_holds(), vec![winning_hold]);
    assert!(wallet_client.get_holds().is_empty());
}

#[tokio::test]
async fn test_close_unsold_auction_releases_all_holds() {
    let pool = setup_test_db().await;
    let auction_repo = AuctionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);

    let wallet_client = Arc::new(MockWalletClient::new());
    let service = AuctionService::new_with_wallet(
        auction_repo.clone(),
        bid_repo.clone(),
        outbox_repo,
        wallet_client.clone(),
    );

    let auction_id = Uuid::new_v4().to_string();
    let now = 1_700_000_000i64;

    auction_repo
        .insert(&NewAuctionRecord {
            id: auction_id.clone(),
            listing_id: "listing-unsold".to_string(),
            seller_id: "seller-unsold".to_string(),
            starting_price_cents: 1000,
            reserve_price_cents: 10_000,
            current_highest_bid_cents: None,
            minimum_increment_cents: 200,
            status: "ACTIVE".to_string(),
            start_time: now - 600,
            end_time: now + 300,
            created_at: now - 600,
            updated_at: now - 600,
        })
        .await
        .expect("insert auction");

    service
        .place_bid_and_persist(&auction_id, "bidder", 1700, now + 10)
        .await
        .expect("bid below reserve");
    let held = wallet_client.get_holds()[0].hold_id.clone();

    sqlx::query("UPDATE auctions SET end_time = ? WHERE id = ?")
        .bind(now - 1)
        .bind(&auction_id)
        .execute(&auction_repo.pool)
        .await
        .expect("expire auction");

    let closed = service
        .close_auction(&auction_id)
        .await
        .expect("close auction");

    assert_eq!(closed.status, "UNSOLD");
    assert_eq!(wallet_client.get_released_holds(), vec![held]);
    assert!(wallet_client.get_holds().is_empty());
}
