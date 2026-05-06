use axum::Router;
use sqlx::AnyPool;
use sqlx::any::{AnyPoolOptions, install_default_drivers};
use std::env;
use std::sync::Arc;

use crate::client::{
    CatalogClient, CatalogClientError, HttpCatalogClient, HttpWalletClient, WalletClient,
    WalletClientError,
};
use crate::http::router::create_router;
use crate::persistence::repositories::{AuctionRepository, BidRepository, OutboxRepository};
use crate::service::auction_service::AuctionService;

pub fn default_database_url() -> String {
    "postgresql://postgres:postgres@localhost:5432/bidmart_auction".to_string()
}

pub fn build_router(pool: AnyPool) -> Router {
    let auction_repo = AuctionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let catalog_url = env::var("CATALOGUE_SERVICE_URL").ok();
    let catalog_client = catalog_client_from_url(catalog_url.as_deref())
        .expect("CATALOGUE_SERVICE_URL must be a valid http URL");
    let wallet_url = env::var("WALLET_SERVICE_URL").ok();
    let wallet_client = wallet_client_from_url(wallet_url.as_deref())
        .expect("WALLET_SERVICE_URL must be a valid http URL");
    let auction_service = AuctionService::new_with_clients(
        auction_repo,
        bid_repo,
        outbox_repo,
        wallet_client,
        catalog_client,
    );

    create_router(auction_service)
}

pub fn catalog_client_from_url(
    base_url: Option<&str>,
) -> Result<Option<Arc<dyn CatalogClient>>, CatalogClientError> {
    let Some(base_url) = base_url.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };

    Ok(Some(Arc::new(HttpCatalogClient::new(base_url)?)))
}

pub fn wallet_client_from_url(
    base_url: Option<&str>,
) -> Result<Option<Arc<dyn WalletClient>>, WalletClientError> {
    let Some(base_url) = base_url.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };

    Ok(Some(Arc::new(HttpWalletClient::new(base_url)?)))
}

pub async fn connect_pool(database_url: &str) -> Result<AnyPool, sqlx::Error> {
    install_default_drivers();
    let max_connections = if database_url == "sqlite::memory:" {
        1
    } else {
        5
    };

    AnyPoolOptions::new()
        .max_connections(max_connections)
        .connect(database_url)
        .await
}

pub async fn run_migrations(pool: &AnyPool) -> Result<(), sqlx::Error> {
    let sql = include_str!("../migrations/20260428000000_init.sql");

    for statement in sql.split(';') {
        let trimmed = statement.trim();
        if !trimmed.is_empty() {
            sqlx::query(trimmed).execute(pool).await?;
        }
    }

    Ok(())
}
