mod common;

use bidmart_auction_service_rust::client::{
    HoldFundsRequest, HoldResponse, WalletClient, WalletClientError,
};
use bidmart_auction_service_rust::persistence::models::{
    NewBidRecord, NewListingAuctionSessionRecord, NewOutboxEventRecord,
};
use bidmart_auction_service_rust::persistence::repositories::{
    BidRepository, ListingAuctionSessionRepository, OutboxRepository,
};
use bidmart_auction_service_rust::scheduler::outbox_scheduler::{
    OutboxPublishError, OutboxScheduler,
};
use bidmart_auction_service_rust::service::auction_service::{
    AuctionService, CreateAuctionCommand,
};
use sqlx::{AnyPool, Row};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Mutex;

async fn setup_test_db() -> AnyPool {
    let pool = bidmart_auction_service_rust::server::connect_pool("sqlite::memory:")
        .await
        .expect("connect to in-memory db");
    bidmart_auction_service_rust::server::run_migrations(&pool)
        .await
        .expect("run migrations");
    pool
}

fn service_with_catalog(pool: AnyPool) -> AuctionService {
    AuctionService::new_with_catalog(
        ListingAuctionSessionRepository::new(pool.clone()),
        BidRepository::new(pool.clone()),
        OutboxRepository::new(pool),
        common::always_active_catalog(),
    )
}

fn service_with_wallet(pool: AnyPool, wallet_client: Arc<dyn WalletClient>) -> AuctionService {
    AuctionService::new_with_clients(
        ListingAuctionSessionRepository::new(pool.clone()),
        BidRepository::new(pool.clone()),
        OutboxRepository::new(pool),
        Some(wallet_client),
        Some(common::always_active_catalog()),
    )
}

#[derive(Default)]
struct FailFirstConvertWallet {
    fail_next_convert: AtomicBool,
    converted: Mutex<Vec<String>>,
}

impl FailFirstConvertWallet {
    fn new() -> Self {
        Self {
            fail_next_convert: AtomicBool::new(true),
            converted: Mutex::new(Vec::new()),
        }
    }

    async fn converted_holds(&self) -> Vec<String> {
        self.converted.lock().await.clone()
    }
}

#[async_trait::async_trait]
impl WalletClient for FailFirstConvertWallet {
    async fn hold_funds(
        &self,
        request: HoldFundsRequest,
    ) -> Result<HoldResponse, WalletClientError> {
        Ok(HoldResponse {
            id: request.hold_id,
            status: "ACTIVE".to_string(),
            amount: request.amount,
        })
    }

    async fn release_hold(&self, _hold_id: &str) -> Result<(), WalletClientError> {
        Ok(())
    }

    async fn convert_hold_to_payment(&self, hold_id: &str) -> Result<(), WalletClientError> {
        if self.fail_next_convert.swap(false, Ordering::SeqCst) {
            return Err(WalletClientError::ServiceError(
                "transient convert failure".to_string(),
            ));
        }
        self.converted.lock().await.push(hold_id.to_string());
        Ok(())
    }

    async fn credit_seller_escrow(
        &self,
        _seller_id: &str,
        _amount_cents: u64,
        _correlation_id: &str,
    ) -> Result<(), WalletClientError> {
        Ok(())
    }
}

#[tokio::test]
async fn create_auction_creates_pending_closure_job() {
    let pool = setup_test_db().await;
    let service = service_with_catalog(pool.clone());
    let now = chrono::Utc::now().timestamp();

    let auction = service
        .create_auction(CreateAuctionCommand {
            listing_id: "closure-job-listing".to_string(),
            seller_id: "seller-1".to_string(),
            auction_type: "ENGLISH".to_string(),
            starting_price_cents: 1000,
            reserve_price_cents: 1500,
            minimum_increment_cents: 100,
            start_time: now,
            end_time: now + 600,
        })
        .await
        .expect("create auction");

    let row = sqlx::query("SELECT due_at, status FROM auction_closure_jobs WHERE auction_id = $1")
        .bind(&auction.id)
        .fetch_one(&pool)
        .await
        .expect("closure job");
    assert_eq!(row.get::<i64, _>("due_at"), auction.end_time);
    assert_eq!(row.get::<String, _>("status"), "PENDING");
}

#[tokio::test]
async fn anti_sniping_extension_updates_closure_job_due_time() {
    let pool = setup_test_db().await;
    let listing_repo = ListingAuctionSessionRepository::new(pool.clone());
    let service = service_with_catalog(pool.clone());
    let now = chrono::Utc::now().timestamp();
    let auction_id = "anti-snipe-job";

    listing_repo
        .insert(&NewListingAuctionSessionRecord {
            id: auction_id.to_string(),
            listing_id: auction_id.to_string(),
            seller_id: "seller-1".to_string(),
            starting_price_cents: 1000,
            reserve_price_cents: 1500,
            current_highest_bid_cents: None,
            minimum_increment_cents: 100,
            status: "ACTIVE".to_string(),
            start_time: now - 10,
            end_time: now + 100,
            created_at: now - 10,
            updated_at: now - 10,
        })
        .await
        .expect("insert auction");

    service
        .place_bid_and_persist(auction_id, "buyer-1", 1000, now + 50)
        .await
        .expect("place bid");

    let auction = listing_repo
        .find_by_id(auction_id)
        .await
        .expect("find auction")
        .expect("auction exists");
    let due_at: i64 =
        sqlx::query_scalar("SELECT due_at FROM auction_closure_jobs WHERE auction_id = $1")
            .bind(auction_id)
            .fetch_one(&pool)
            .await
            .expect("closure job due");
    assert_eq!(due_at, auction.end_time);
}

#[tokio::test]
async fn closure_job_retries_settlement_after_wallet_failure() {
    let pool = setup_test_db().await;
    let listing_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let wallet = Arc::new(FailFirstConvertWallet::new());
    let service = service_with_wallet(pool.clone(), wallet.clone());
    let now = chrono::Utc::now().timestamp();
    let auction_id = "settlement-retry-auction";
    let hold_id = "hold-winning-bid";

    listing_repo
        .insert(&NewListingAuctionSessionRecord {
            id: auction_id.to_string(),
            listing_id: auction_id.to_string(),
            seller_id: "seller-1".to_string(),
            starting_price_cents: 1000,
            reserve_price_cents: 1000,
            current_highest_bid_cents: Some(1500),
            minimum_increment_cents: 100,
            status: "ACTIVE".to_string(),
            start_time: now - 600,
            end_time: now - 1,
            created_at: now - 600,
            updated_at: now - 10,
        })
        .await
        .expect("insert auction");
    bid_repo
        .insert_with_wallet_hold(
            &NewBidRecord {
                id: "winning-bid".to_string(),
                auction_id: auction_id.to_string(),
                bidder_id: "buyer-1".to_string(),
                bid_amount_cents: 1500,
                bid_time: now - 20,
            },
            Some(hold_id),
        )
        .await
        .expect("insert bid");

    let first = service.process_one_pending_closure().await;
    assert!(first.is_err());

    let job =
        sqlx::query("SELECT status, last_error FROM auction_closure_jobs WHERE auction_id = $1")
            .bind(auction_id)
            .fetch_one(&pool)
            .await
            .expect("closure job");
    assert_eq!(job.get::<String, _>("status"), "SETTLING");
    assert!(
        job.get::<Option<String>, _>("last_error")
            .as_deref()
            .unwrap_or_default()
            .contains("transient convert failure")
    );

    sqlx::query(
        "UPDATE auction_closure_jobs SET due_at = $1, locked_until = NULL WHERE auction_id = $2",
    )
    .bind(now - 1)
    .bind(auction_id)
    .execute(&pool)
    .await
    .expect("make retry due");

    let second = service
        .process_one_pending_closure()
        .await
        .expect("retry settlement")
        .expect("settled auction");
    assert_eq!(second.status, "WON");
    assert_eq!(wallet.converted_holds().await, vec![hold_id.to_string()]);

    let status: String =
        sqlx::query_scalar("SELECT status FROM auction_closure_jobs WHERE auction_id = $1")
            .bind(auction_id)
            .fetch_one(&pool)
            .await
            .expect("closure status");
    assert_eq!(status, "DONE");
}

#[tokio::test]
async fn outbox_publish_failure_records_backoff_state() {
    let pool = setup_test_db().await;
    let outbox_repo = OutboxRepository::new(pool.clone());
    let scheduler = OutboxScheduler::new(outbox_repo.clone());
    let now = chrono::Utc::now().timestamp();

    outbox_repo
        .insert(&NewOutboxEventRecord {
            id: "event-backoff".to_string(),
            aggregate_id: "auction-1".to_string(),
            event_type: "BidPlaced".to_string(),
            payload: "{}".to_string(),
            published: false,
            published_at: None,
            created_at: now,
            updated_at: now,
        })
        .await
        .expect("insert outbox");

    let report = scheduler
        .publish_pending(10, |_| async {
            Err(OutboxPublishError::new("publisher unavailable"))
        })
        .await
        .expect("publish pending");
    assert_eq!(report.failed, 1);

    let row = sqlx::query(
        "SELECT attempts, next_attempt_at, last_error, CASE WHEN locked_until IS NULL THEN 1 ELSE 0 END AS lock_cleared FROM outbox_events WHERE id = $1",
    )
    .bind("event-backoff")
    .fetch_one(&pool)
    .await
    .expect("outbox row");
    assert_eq!(row.get::<i64, _>("attempts"), 1);
    assert!(row.get::<i64, _>("next_attempt_at") >= now);
    assert_eq!(
        row.get::<Option<String>, _>("last_error").as_deref(),
        Some("publisher unavailable")
    );
    assert_eq!(row.get::<i64, _>("lock_cleared"), 1);
}

#[tokio::test]
async fn migrations_upgrade_old_outbox_schema_without_data_loss() {
    let pool = bidmart_auction_service_rust::server::connect_pool("sqlite::memory:")
        .await
        .expect("connect to in-memory db");
    sqlx::query(
        "CREATE TABLE outbox_events (
            id TEXT PRIMARY KEY,
            aggregate_id TEXT NOT NULL,
            event_type TEXT NOT NULL,
            payload TEXT NOT NULL,
            published BOOLEAN NOT NULL,
            published_at INTEGER,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        )",
    )
    .execute(&pool)
    .await
    .expect("create old outbox");
    sqlx::query(
        "INSERT INTO outbox_events (id, aggregate_id, event_type, payload, published, published_at, created_at, updated_at)
         VALUES ('old-event', 'auction-1', 'BidPlaced', '{}', false, NULL, 1, 1)",
    )
    .execute(&pool)
    .await
    .expect("insert old event");

    bidmart_auction_service_rust::server::run_migrations(&pool)
        .await
        .expect("upgrade schema");

    let row = sqlx::query("SELECT attempts, next_attempt_at FROM outbox_events WHERE id = $1")
        .bind("old-event")
        .fetch_one(&pool)
        .await
        .expect("old event survives");
    assert_eq!(row.get::<i64, _>("attempts"), 0);
    assert_eq!(row.get::<i64, _>("next_attempt_at"), 0);
}
