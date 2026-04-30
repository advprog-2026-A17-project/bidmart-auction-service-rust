use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use tower::ServiceExt;

use bidmart_auction_service_rust::server::{
    build_router, catalog_client_from_url, wallet_client_from_url,
};

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

#[tokio::test]
async fn build_router_wires_service_repositories_into_http_app() {
    let pool = setup_test_db().await;
    let app = build_router(pool);

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/auctions/missing-auction")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("call app");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[test]
fn catalog_client_from_url_builds_http_catalog_client_when_configured() {
    let client = catalog_client_from_url(Some("http://catalogue-service:8081"))
        .expect("build catalog client");

    assert!(client.is_some());
}

#[test]
fn catalog_client_from_url_skips_catalog_client_when_unconfigured() {
    let client = catalog_client_from_url(None).expect("build catalog client");

    assert!(client.is_none());
}

#[test]
fn wallet_client_from_url_builds_http_wallet_client_when_configured() {
    let client =
        wallet_client_from_url(Some("http://wallet-service:8083")).expect("build wallet client");

    assert!(client.is_some());
}

#[test]
fn wallet_client_from_url_skips_wallet_client_when_unconfigured() {
    let client = wallet_client_from_url(None).expect("build wallet client");

    assert!(client.is_none());
}
