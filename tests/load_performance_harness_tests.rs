use std::sync::Arc;
use std::time::Instant;

use bidmart_auction_service_rust::persistence::models::NewListingAuctionSessionRecord;
use bidmart_auction_service_rust::persistence::repositories::{
    ListingAuctionSessionRepository, BidRepository, OutboxRepository,
};
use bidmart_auction_service_rust::service::auction_service::AuctionService;

struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.state
    }
}

async fn setup_test_db() -> sqlx::AnyPool {
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
#[ignore = "manual load harness"]
async fn run_seeded_bidding_load_harness() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = Arc::new(AuctionService::new(
        listing_auction_session_repo.clone(),
        bid_repo.clone(),
        outbox_repo,
    ));

    let now = chrono::Utc::now().timestamp();
    let auction_id = "auction-load-seeded".to_string();
    listing_auction_session_repo
        .insert(&NewListingAuctionSessionRecord {
            id: auction_id.clone(),
            listing_id: "listing-load-seeded".to_string(),
            seller_id: "seller-load-seeded".to_string(),
            starting_price_cents: 10_000,
            reserve_price_cents: 20_000,
            current_highest_bid_cents: None,
            minimum_increment_cents: 100,
            status: "ACTIVE".to_string(),
            start_time: now - 60,
            end_time: now + 600,
            created_at: now,
            updated_at: now,
        })
        .await
        .expect("insert auction");

    let attempts = 300usize;
    let mut rng = Lcg::new(20260516);
    let mut handles = Vec::with_capacity(attempts);

    for idx in 0..attempts {
        let service = service.clone();
        let auction_id = auction_id.clone();
        let random = rng.next();
        let bidder_id = format!("load-bidder-{}", random % 120);
        let bid_amount = 10_000 + ((random % 15_000) as i64);
        let bid_time = now + 1 + (idx as i64 % 45);

        handles.push(tokio::spawn(async move {
            let started = Instant::now();
            let result = service
                .place_bid_and_persist(&auction_id, &bidder_id, bid_amount, bid_time)
                .await;
            (result.is_ok(), started.elapsed())
        }));
    }

    let mut accepted = 0usize;
    let mut latencies_ms = Vec::with_capacity(attempts);
    for handle in handles {
        let (ok, latency) = handle.await.expect("join task");
        if ok {
            accepted += 1;
        }
        latencies_ms.push(latency.as_millis() as u64);
    }
    latencies_ms.sort_unstable();

    let p50 = latencies_ms[latencies_ms.len() / 2];
    let p95 = latencies_ms[(latencies_ms.len() * 95) / 100];
    let max = *latencies_ms.last().expect("max latency");
    let apdex_t_ms = 100u64;
    let satisfied = latencies_ms
        .iter()
        .filter(|latency| **latency <= apdex_t_ms)
        .count();
    let tolerating = latencies_ms
        .iter()
        .filter(|latency| **latency > apdex_t_ms && **latency <= apdex_t_ms * 4)
        .count();
    let apdex_score = (satisfied as f64 + (tolerating as f64 / 2.0)) / latencies_ms.len() as f64;

    let bids = bid_repo
        .list_by_auction_id_desc(&auction_id)
        .await
        .expect("list bids");

    assert!(accepted > 0);
    assert!(!bids.is_empty());
    println!(
        "load-harness seed=20260516 attempts={attempts} accepted={accepted} bids={} p50_ms={p50} p95_ms={p95} max_ms={max} apdex_t_ms={apdex_t_ms} apdex={apdex_score:.3} satisfied={satisfied} tolerating={tolerating}",
        bids.len()
    );
}
