use axum::Router;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::env;
use std::str::FromStr;
use std::sync::Arc;

use crate::client::{CatalogClient, CatalogClientError, HttpCatalogClient};
use crate::http::router::create_router;
use crate::persistence::repositories::{AuctionRepository, BidRepository, OutboxRepository};
use crate::service::auction_service::AuctionService;

pub fn build_router(pool: SqlitePool) -> Router {
    let auction_repo = AuctionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let catalog_url = env::var("CATALOGUE_SERVICE_URL").ok();
    let catalog_client = catalog_client_from_url(catalog_url.as_deref())
        .expect("CATALOGUE_SERVICE_URL must be a valid http URL");
    let auction_service = match catalog_client {
        Some(catalog_client) => {
            AuctionService::new_with_catalog(auction_repo, bid_repo, outbox_repo, catalog_client)
        }
        None => AuctionService::new(auction_repo, bid_repo, outbox_repo),
    };

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

pub async fn connect_pool(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::from_str(database_url)?.create_if_missing(true);

    SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
}

pub async fn run_migrations(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let sql = include_str!("../migrations/20260428000000_init.sql");

    for statement in sql.split(';') {
        let trimmed = statement.trim();
        if !trimmed.is_empty() {
            sqlx::query(trimmed).execute(pool).await?;
        }
    }

    Ok(())
}
