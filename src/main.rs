use bidmart_auction_service_rust::server::{build_router, connect_pool, run_migrations};
use dotenvy::from_path;
use std::env;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = from_path(".env");
    let _ = dotenvy::from_path_override("../bidmart-infrastructure/.env");

    let database_url =
        env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://bidmart-auction.db".to_string());
    let bind_address = env::var("BIND_ADDRESS").unwrap_or_else(|_| "0.0.0.0:3000".to_string());

    let pool = connect_pool(&database_url).await?;
    run_migrations(&pool).await?;

    let app = build_router(pool);
    let listener = TcpListener::bind(&bind_address).await?;

    axum::serve(listener, app).await?;

    Ok(())
}
