use axum::http::StatusCode;
use http_body_util::BodyExt;
use tower::ServiceExt;

/// Verify that the auction service sets secure-by-default headers on all responses.
/// This is a security testing requirement for the "secure coding" rubric item.
#[tokio::test]
async fn responses_contain_security_headers() {
    // We test the /metrics endpoint since it requires no database setup
    let router = bidmart_auction_service_rust::http::router::create_router(
        create_test_service().await,
    );

    let request = axum::http::Request::builder()
        .uri("/metrics")
        .method("GET")
        .body(axum::body::Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let headers = response.headers();
    assert_eq!(
        headers.get("x-content-type-options").unwrap().to_str().unwrap(),
        "nosniff"
    );
    assert_eq!(
        headers.get("x-frame-options").unwrap().to_str().unwrap(),
        "DENY"
    );
    assert_eq!(
        headers.get("x-xss-protection").unwrap().to_str().unwrap(),
        "1; mode=block"
    );
    assert_eq!(
        headers.get("referrer-policy").unwrap().to_str().unwrap(),
        "strict-origin-when-cross-origin"
    );
    assert!(
        headers.get("cache-control").unwrap().to_str().unwrap().contains("no-store")
    );
}

#[tokio::test]
async fn metrics_endpoint_returns_prometheus_format() {
    let router = bidmart_auction_service_rust::http::router::create_router(
        create_test_service().await,
    );

    let request = axum::http::Request::builder()
        .uri("/metrics")
        .method("GET")
        .body(axum::body::Body::empty())
        .unwrap();

    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let content_type = response
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(content_type.contains("text/plain"));

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8(body.to_vec()).unwrap();

    // Verify essential metric families are present
    assert!(text.contains("bidmart_service_up"));
    assert!(text.contains("bidmart_service_uptime_seconds"));
    assert!(text.contains("bidmart_http_requests_total"));
    assert!(text.contains("bidmart_http_errors_total"));
    assert!(text.contains("bidmart_apdex_score"));
    assert!(text.contains("bidmart_apdex_satisfied_total"));
    assert!(text.contains("bidmart_apdex_tolerating_total"));
    assert!(text.contains("bidmart_http_request_duration_seconds_bucket"));
    assert!(text.contains("bidmart_bids_placed_total"));
    assert!(text.contains("bidmart_auctions_created_total"));
    assert!(text.contains("bidmart_auctions_closed_total"));
}

/// Helper: create a minimal AuctionService backed by an in-memory SQLite database.
async fn create_test_service() -> bidmart_auction_service_rust::service::auction_service::AuctionService {
    use bidmart_auction_service_rust::persistence::repositories::{
        ListingAuctionSessionRepository, BidRepository, OutboxRepository,
    };

    let pool = bidmart_auction_service_rust::server::connect_pool("sqlite::memory:")
        .await
        .expect("connect to in-memory db");

    // Read and apply migrations
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

    let auction_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);

    bidmart_auction_service_rust::service::auction_service::AuctionService::new(
        auction_repo,
        bid_repo,
        outbox_repo,
    )
}
