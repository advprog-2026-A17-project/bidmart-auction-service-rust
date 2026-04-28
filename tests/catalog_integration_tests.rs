use std::sync::{Arc, Mutex};

use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};

use bidmart_auction_service_rust::client::{
    CatalogClient, CatalogClientError, ListingSummary,
};
use bidmart_auction_service_rust::persistence::repositories::{
    AuctionRepository, BidRepository, OutboxRepository,
};
use bidmart_auction_service_rust::service::auction_service::{
    AuctionService, CreateAuctionCommand,
};

#[derive(Debug)]
struct MockCatalogClient {
    listing: Arc<Mutex<Result<ListingSummary, CatalogClientError>>>,
}

impl MockCatalogClient {
    fn new(listing: ListingSummary) -> Self {
        Self {
            listing: Arc::new(Mutex::new(Ok(listing))),
        }
    }
}

#[async_trait::async_trait]
impl CatalogClient for MockCatalogClient {
    async fn get_listing_summary(
        &self,
        _listing_id: &str,
    ) -> Result<ListingSummary, CatalogClientError> {
        self.listing.lock().expect("catalog lock").clone()
    }
}

async fn setup_test_db() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
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

fn create_command() -> CreateAuctionCommand {
    let now = chrono::Utc::now().timestamp();

    CreateAuctionCommand {
        listing_id: "listing-catalog-1".to_string(),
        seller_id: "seller-catalog-1".to_string(),
        starting_price_cents: 1000,
        reserve_price_cents: 5000,
        minimum_increment_cents: 200,
        start_time: now - 60,
        end_time: now + 600,
    }
}

async fn service_with_catalog(
    catalog_client: Arc<dyn CatalogClient>,
) -> (AuctionService, AuctionRepository) {
    let pool = setup_test_db().await;
    let auction_repo = AuctionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = AuctionService::new_with_catalog(
        auction_repo.clone(),
        bid_repo,
        outbox_repo,
        catalog_client,
    );

    (service, auction_repo)
}

#[tokio::test]
async fn create_auction_rejects_inactive_catalog_listing() {
    let command = create_command();
    let catalog_client = Arc::new(MockCatalogClient::new(ListingSummary {
        id: command.listing_id.clone(),
        seller_id: command.seller_id.clone(),
        status: "CANCELLED".to_string(),
    }));
    let (service, auction_repo) = service_with_catalog(catalog_client).await;

    let result = service.create_auction(command).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Listing is not active"));
    assert!(auction_repo.list_all().await.expect("list auctions").is_empty());
}

#[tokio::test]
async fn create_auction_rejects_catalog_seller_mismatch() {
    let command = create_command();
    let catalog_client = Arc::new(MockCatalogClient::new(ListingSummary {
        id: command.listing_id.clone(),
        seller_id: "different-seller".to_string(),
        status: "ACTIVE".to_string(),
    }));
    let (service, auction_repo) = service_with_catalog(catalog_client).await;

    let result = service.create_auction(command).await;

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Listing seller does not match auction seller"));
    assert!(auction_repo.list_all().await.expect("list auctions").is_empty());
}

#[tokio::test]
async fn create_auction_accepts_active_catalog_listing_for_matching_seller() {
    let command = create_command();
    let catalog_client = Arc::new(MockCatalogClient::new(ListingSummary {
        id: command.listing_id.clone(),
        seller_id: command.seller_id.clone(),
        status: "ACTIVE".to_string(),
    }));
    let (service, auction_repo) = service_with_catalog(catalog_client).await;

    let auction = service
        .create_auction(command.clone())
        .await
        .expect("create auction");

    assert_eq!(auction.listing_id, command.listing_id);
    assert_eq!(auction.seller_id, command.seller_id);
    assert_eq!(auction_repo.list_all().await.expect("list auctions").len(), 1);
}
