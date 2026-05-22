use axum::Router;
use sqlx::AnyPool;
use sqlx::any::{AnyPoolOptions, install_default_drivers};
use std::env;
use std::sync::Arc;

use crate::client::{
    CatalogClient, CatalogClientError, GrpcCatalogClient, GrpcWalletClient, HttpCatalogClient,
    HttpWalletClient, WalletClient, WalletClientError,
};
use crate::http::router::create_router;
use crate::persistence::repositories::{ListingAuctionSessionRepository, BidRepository, OutboxRepository};
use crate::service::auction_service::AuctionService;

pub fn default_database_url() -> String {
    "postgresql://postgres:postgres@localhost:5432/bidmart_auction".to_string()
}

pub fn build_router(pool: AnyPool) -> (Router, AuctionService) {
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let catalog_grpc_url = env::var("CATALOGUE_GRPC_URL").ok();
    let catalog_http_url = env::var("CATALOGUE_SERVICE_URL").ok();
    let catalog_client =
        catalog_client_from_endpoints(catalog_grpc_url.as_deref(), catalog_http_url.as_deref())
            .expect("CATALOGUE_GRPC_URL or CATALOGUE_SERVICE_URL must be valid");
    let wallet_grpc_url = env::var("WALLET_GRPC_URL").ok();
    let wallet_http_url = env::var("WALLET_SERVICE_URL").ok();
    let wallet_client =
        wallet_client_from_endpoints(wallet_grpc_url.as_deref(), wallet_http_url.as_deref())
            .expect("WALLET_GRPC_URL or WALLET_SERVICE_URL must be valid");
    let auction_service = AuctionService::new_with_clients(
        listing_auction_session_repo,
        bid_repo,
        outbox_repo,
        wallet_client,
        catalog_client,
    );

    (create_router(auction_service.clone()), auction_service)
}

pub fn catalog_client_from_url(
    base_url: Option<&str>,
) -> Result<Option<Arc<dyn CatalogClient>>, CatalogClientError> {
    let Some(base_url) = base_url.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };

    Ok(Some(Arc::new(HttpCatalogClient::new(base_url)?)))
}

pub fn catalog_client_from_endpoints(
    grpc_endpoint: Option<&str>,
    http_base_url: Option<&str>,
) -> Result<Option<Arc<dyn CatalogClient>>, CatalogClientError> {
    if let Some(grpc_endpoint) = grpc_endpoint.filter(|value| !value.trim().is_empty()) {
        return Ok(Some(Arc::new(GrpcCatalogClient::new(grpc_endpoint)?)));
    }

    if let Some(http_base_url) = http_base_url.filter(|value| !value.trim().is_empty()) {
        return Ok(Some(Arc::new(HttpCatalogClient::new(http_base_url)?)));
    }

    Ok(None)
}

pub fn wallet_client_from_url(
    base_url: Option<&str>,
) -> Result<Option<Arc<dyn WalletClient>>, WalletClientError> {
    let Some(base_url) = base_url.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };

    Ok(Some(Arc::new(HttpWalletClient::new(base_url)?)))
}

/// Prefer HTTP when both endpoints are configured: seller escrow and other
/// post-close wallet operations are HTTP-only on the wallet service today.
pub fn wallet_client_from_endpoints(
    grpc_endpoint: Option<&str>,
    http_base_url: Option<&str>,
) -> Result<Option<Arc<dyn WalletClient>>, WalletClientError> {
    if let Some(http_base_url) = http_base_url.filter(|value| !value.trim().is_empty()) {
        return Ok(Some(Arc::new(HttpWalletClient::new(http_base_url)?)));
    }

    if let Some(grpc_endpoint) = grpc_endpoint.filter(|value| !value.trim().is_empty()) {
        return Ok(Some(Arc::new(GrpcWalletClient::new(grpc_endpoint)?)));
    }

    Ok(None)
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
