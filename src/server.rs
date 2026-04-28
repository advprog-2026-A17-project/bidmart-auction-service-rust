use axum::Router;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

use crate::http::router::create_router;
use crate::persistence::repositories::{AuctionRepository, BidRepository, OutboxRepository};
use crate::service::auction_service::AuctionService;

pub fn build_router(pool: SqlitePool) -> Router {
    let auction_repo = AuctionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let auction_service = AuctionService::new(auction_repo, bid_repo, outbox_repo);

    create_router(auction_service)
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
