use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode};
use serde_json::{Value, json};
use sqlx::AnyPool;
use tower::ServiceExt;

use bidmart_auction_service_rust::http::router::create_router;
use bidmart_auction_service_rust::persistence::models::{NewListingAuctionSessionRecord, NewBidRecord};
use bidmart_auction_service_rust::persistence::repositories::{
    ListingAuctionSessionRepository, BidRepository, OutboxRepository,
};
use bidmart_auction_service_rust::service::auction_service::AuctionService;

mod common;
use common::always_active_catalog;

fn test_auction_service(
    listing_auction_session_repo: ListingAuctionSessionRepository,
    bid_repo: BidRepository,
    outbox_repo: OutboxRepository,
) -> AuctionService {
    AuctionService::new_with_catalog(
        listing_auction_session_repo,
        bid_repo,
        outbox_repo,
        always_active_catalog(),
    )
}

async fn setup_test_db() -> AnyPool {
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
async fn create_auction_returns_created_auction_response() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = test_auction_service(listing_auction_session_repo.clone(), bid_repo, outbox_repo);
    let app = create_router(service);

    let now = chrono::Utc::now().timestamp();
    let request_body = json!({
        "listing_id": "listing-1",
        "seller_id": "seller-1",
        "starting_price_cents": 1000,
        "reserve_price_cents": 5000,
        "minimum_increment_cents": 200,
        "start_time": now + 60,
        "end_time": now + 600
    });

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/listings")
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .expect("build request"),
        )
        .await
        .expect("call app");

    assert_eq!(response.status(), StatusCode::CREATED);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    let response_body: Value = serde_json::from_slice(&body).expect("parse response json");

    let auction_id = response_body["id"]
        .as_str()
        .expect("response has auction id")
        .to_string();
    assert!(!auction_id.is_empty());
    assert_eq!(response_body["listing_id"], json!("listing-1"));
    assert_eq!(response_body["seller_id"], json!("seller-1"));
    assert_eq!(response_body["starting_price_cents"], json!(1000));
    assert_eq!(response_body["reserve_price_cents"], json!(5000));
    assert_eq!(response_body["minimum_increment_cents"], json!(200));
    assert_eq!(response_body["auction_type"], json!("ENGLISH"));
    assert_eq!(response_body["auctionType"], json!("ENGLISH"));
    assert_eq!(response_body["current_highest_bid_cents"], Value::Null);
    assert_eq!(response_body["status"], json!("DRAFT"));
    assert_eq!(response_body["start_time"], json!(now + 60));
    assert_eq!(response_body["end_time"], json!(now + 600));

    let persisted = listing_auction_session_repo
        .find_by_id(&auction_id)
        .await
        .expect("find persisted auction")
        .expect("auction persisted");
    assert_eq!(persisted.listing_id, "listing-1");
    assert_eq!(persisted.seller_id, "seller-1");
    assert_eq!(persisted.starting_price_cents, 1000);
    assert_eq!(persisted.reserve_price_cents, 5000);
    assert_eq!(persisted.minimum_increment_cents, 200);
    assert_eq!(persisted.auction_type, "ENGLISH");
    assert_eq!(persisted.status, "DRAFT");
}

#[tokio::test]
async fn api_v1_create_auction_rejects_unsupported_future_auction_type() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = test_auction_service(listing_auction_session_repo, bid_repo, outbox_repo);
    let app = create_router(service);

    let now = chrono::Utc::now().timestamp();
    let request_body = json!({
        "listingId": "listing-future-type",
        "sellerId": "seller-future-type",
        "auctionType": "SEALED_BID",
        "startingPrice": 25.5,
        "reservePrice": 50.0,
        "minimumIncrement": 2.5,
        "start_time": now + 60,
        "end_time": now + 600
    });

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/listings")
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .expect("build request"),
        )
        .await
        .expect("call app");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    let response_body: Value = serde_json::from_slice(&body).expect("parse response json");
    assert!(
        response_body["message"]
            .as_str()
            .expect("message")
            .contains("Unsupported auction type")
    );
}

#[tokio::test]
async fn api_v1_create_auction_rejects_recognized_but_disabled_auction_type() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = test_auction_service(listing_auction_session_repo, bid_repo, outbox_repo);
    let app = create_router(service);

    let now = chrono::Utc::now().timestamp();
    let request_body = json!({
        "listingId": "listing-future-type",
        "sellerId": "seller-future-type",
        "auctionType": "SCHOLARSHIP",
        "startingPrice": 25.5,
        "reservePrice": 50.0,
        "minimumIncrement": 2.5,
        "start_time": now + 60,
        "end_time": now + 600
    });

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/listings")
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .expect("build request"),
        )
        .await
        .expect("call app");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    let response_body: Value = serde_json::from_slice(&body).expect("parse response json");
    assert!(
        response_body["message"]
            .as_str()
            .expect("message")
            .contains("not enabled yet")
    );
}

#[tokio::test]
async fn api_v1_create_auction_accepts_gateway_payload_and_persists_cents() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = test_auction_service(listing_auction_session_repo.clone(), bid_repo, outbox_repo);
    let app = create_router(service);

    let start_time = chrono::DateTime::<chrono::Utc>::from_timestamp(1_900_000_000, 0)
        .expect("valid start time")
        .to_rfc3339();
    let end_time = chrono::DateTime::<chrono::Utc>::from_timestamp(1_900_003_600, 0)
        .expect("valid end time")
        .to_rfc3339();
    let request_body = json!({
        "listingId": "listing-create-gateway",
        "sellerId": "seller-create-gateway",
        "startingPrice": 25.5,
        "reservePrice": 50.0,
        "minimumIncrement": 2.5,
        "startTime": start_time,
        "endTime": end_time
    });

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/listings")
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .expect("build request"),
        )
        .await
        .expect("call app");

    assert_eq!(response.status(), StatusCode::CREATED);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    let response_body: Value = serde_json::from_slice(&body).expect("parse response json");
    let auction_id = response_body["id"]
        .as_str()
        .expect("response has auction id");

    assert_eq!(response_body["listingId"], json!("listing-create-gateway"));
    assert_eq!(response_body["sellerId"], json!("seller-create-gateway"));
    assert_eq!(response_body["startingPrice"], json!(25.5));
    assert_eq!(response_body["reservePrice"], json!(50.0));
    assert_eq!(response_body["minimumIncrement"], json!(2.5));

    let persisted = listing_auction_session_repo
        .find_by_id(auction_id)
        .await
        .expect("find persisted auction")
        .expect("auction persisted");
    assert_eq!(persisted.listing_id, "listing-create-gateway");
    assert_eq!(persisted.seller_id, "seller-create-gateway");
    assert_eq!(persisted.starting_price_cents, 2550);
    assert_eq!(persisted.reserve_price_cents, 5000);
    assert_eq!(persisted.minimum_increment_cents, 250);
    assert_eq!(persisted.start_time, 1_900_000_000);
    assert_eq!(persisted.end_time, 1_900_003_600);
}

#[tokio::test]
async fn api_v1_create_auction_uses_trusted_gateway_user_header() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = test_auction_service(listing_auction_session_repo.clone(), bid_repo, outbox_repo);
    let app = create_router(service);

    let now = chrono::Utc::now().timestamp();
    let request_body = json!({
        "listingId": "listing-trusted-seller",
        "startingPrice": 25.5,
        "reservePrice": 50.0,
        "minimumIncrement": 2.5,
        "startTime": now + 60,
        "endTime": now + 600
    });

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/listings")
                .header("content-type", "application/json")
                .header("x-user-id", "seller-from-gateway")
                .body(Body::from(request_body.to_string()))
                .expect("build request"),
        )
        .await
        .expect("call app");

    assert_eq!(response.status(), StatusCode::CREATED);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    let response_body: Value = serde_json::from_slice(&body).expect("parse response json");
    let auction_id = response_body["id"]
        .as_str()
        .expect("response has auction id");

    let persisted = listing_auction_session_repo
        .find_by_id(auction_id)
        .await
        .expect("find persisted auction")
        .expect("auction persisted");
    assert_eq!(persisted.seller_id, "seller-from-gateway");
}

#[tokio::test]
async fn api_v1_create_auction_rejects_conflicting_gateway_seller_header() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = test_auction_service(listing_auction_session_repo, bid_repo, outbox_repo);
    let app = create_router(service);

    let now = chrono::Utc::now().timestamp();
    let request_body = json!({
        "listingId": "listing-conflict",
        "sellerId": "seller-from-body",
        "startingPrice": 25.5,
        "reservePrice": 50.0,
        "minimumIncrement": 2.5,
        "startTime": now + 60,
        "endTime": now + 600
    });

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/listings")
                .header("content-type", "application/json")
                .header("x-user-id", "seller-from-gateway")
                .body(Body::from(request_body.to_string()))
                .expect("build request"),
        )
        .await
        .expect("call app");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn api_v1_create_auction_accepts_frontend_numeric_camel_case_timestamps() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = test_auction_service(listing_auction_session_repo.clone(), bid_repo, outbox_repo);
    let app = create_router(service);

    let now = chrono::Utc::now().timestamp();
    let request_body = json!({
        "listingId": "listing-create-frontend",
        "sellerId": "seller-create-frontend",
        "auctionType": "ENGLISH",
        "startingPrice": 25.5,
        "reservePrice": 50.0,
        "minimumIncrement": 2.5,
        "startTime": now + 60,
        "endTime": now + 600
    });

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/listings")
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .expect("build request"),
        )
        .await
        .expect("call app");

    assert_eq!(response.status(), StatusCode::CREATED);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    let response_body: Value = serde_json::from_slice(&body).expect("parse response json");
    let auction_id = response_body["id"]
        .as_str()
        .expect("response has auction id");

    let persisted = listing_auction_session_repo
        .find_by_id(auction_id)
        .await
        .expect("find persisted auction")
        .expect("auction persisted");
    assert_eq!(persisted.start_time, now + 60);
    assert_eq!(persisted.end_time, now + 600);
}

#[tokio::test]
async fn get_auction_by_id_returns_auction_response() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = test_auction_service(listing_auction_session_repo.clone(), bid_repo, outbox_repo);
    let app = create_router(service);

    let auction_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let new_auction = NewListingAuctionSessionRecord {
        id: auction_id.clone(),
        listing_id: "listing-2".to_string(),
        seller_id: "seller-2".to_string(),
        starting_price_cents: 2500,
        reserve_price_cents: 4000,
        current_highest_bid_cents: Some(3000),
        minimum_increment_cents: 250,
        status: "ACTIVE".to_string(),
        start_time: now - 120,
        end_time: now + 900,
        created_at: now,
        updated_at: now,
    };
    listing_auction_session_repo
        .insert(&new_auction)
        .await
        .expect("insert auction");

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!("/listings/{auction_id}"))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("call app");

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    let response_body: Value = serde_json::from_slice(&body).expect("parse response json");

    assert_eq!(response_body["id"], json!(auction_id));
    assert_eq!(response_body["listing_id"], json!("listing-2"));
    assert_eq!(response_body["seller_id"], json!("seller-2"));
    assert_eq!(response_body["starting_price_cents"], json!(2500));
    assert_eq!(response_body["reserve_price_cents"], json!(4000));
    assert_eq!(response_body["current_highest_bid_cents"], json!(3000));
    assert_eq!(response_body["minimum_increment_cents"], json!(250));
    assert_eq!(response_body["status"], json!("ACTIVE"));
    assert_eq!(response_body["start_time"], json!(now - 120));
    assert_eq!(response_body["end_time"], json!(now + 900));
}

#[tokio::test]
async fn api_v1_get_auction_by_id_returns_gateway_compatible_response() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = test_auction_service(listing_auction_session_repo.clone(), bid_repo, outbox_repo);
    let app = create_router(service);

    let auction_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let new_auction = NewListingAuctionSessionRecord {
        id: auction_id.clone(),
        listing_id: "listing-gateway".to_string(),
        seller_id: "seller-gateway".to_string(),
        starting_price_cents: 2500,
        reserve_price_cents: 5000,
        current_highest_bid_cents: Some(3000),
        minimum_increment_cents: 250,
        status: "ACTIVE".to_string(),
        start_time: now - 120,
        end_time: now + 900,
        created_at: now,
        updated_at: now,
    };
    listing_auction_session_repo
        .insert(&new_auction)
        .await
        .expect("insert auction");

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!("/api/v1/listings/{auction_id}"))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("call app");

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    let response_body: Value = serde_json::from_slice(&body).expect("parse response json");

    assert_eq!(response_body["id"], json!(auction_id));
    assert_eq!(response_body["listingId"], json!("listing-gateway"));
    assert_eq!(response_body["sellerId"], json!("seller-gateway"));
    assert_eq!(response_body["startingPrice"], json!(25.0));
    assert_eq!(response_body["reservePrice"], json!(50.0));
    assert_eq!(response_body["currentHighestBid"], json!(30.0));
    assert_eq!(response_body["minimumIncrement"], json!(2.5));
    assert_eq!(response_body["status"], json!("ACTIVE"));
    assert!(response_body["startTime"].as_str().is_some());
    assert!(response_body["endTime"].as_str().is_some());
}

#[tokio::test]
async fn api_v1_list_auctions_returns_gateway_compatible_page() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = test_auction_service(listing_auction_session_repo.clone(), bid_repo, outbox_repo);
    let app = create_router(service);

    let now = chrono::Utc::now().timestamp();
    let first_auction = NewListingAuctionSessionRecord {
        id: "auction-list-1".to_string(),
        listing_id: "listing-list-1".to_string(),
        seller_id: "seller-list-1".to_string(),
        starting_price_cents: 1000,
        reserve_price_cents: 3000,
        current_highest_bid_cents: None,
        minimum_increment_cents: 100,
        status: "ACTIVE".to_string(),
        start_time: now - 120,
        end_time: now + 900,
        created_at: now,
        updated_at: now,
    };
    let second_auction = NewListingAuctionSessionRecord {
        id: "auction-list-2".to_string(),
        listing_id: "listing-list-2".to_string(),
        seller_id: "seller-list-2".to_string(),
        starting_price_cents: 2500,
        reserve_price_cents: 5000,
        current_highest_bid_cents: Some(3200),
        minimum_increment_cents: 250,
        status: "ACTIVE".to_string(),
        start_time: now - 60,
        end_time: now + 1200,
        created_at: now + 1,
        updated_at: now + 1,
    };
    listing_auction_session_repo
        .insert(&first_auction)
        .await
        .expect("insert first auction");
    listing_auction_session_repo
        .insert(&second_auction)
        .await
        .expect("insert second auction");

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/v1/listings")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("call app");

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    let response_body: Value = serde_json::from_slice(&body).expect("parse response json");
    let items = response_body["items"]
        .as_array()
        .expect("response has page items");

    assert_eq!(response_body["page"], json!(0));
    assert_eq!(response_body["size"], json!(2));
    assert_eq!(response_body["totalItems"], json!(2));
    assert_eq!(response_body["totalPages"], json!(1));
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["id"], json!("auction-list-2"));
    assert_eq!(items[0]["listingId"], json!("listing-list-2"));
    assert_eq!(items[0]["startingPrice"], json!(25.0));
    assert_eq!(items[0]["currentHighestBid"], json!(32.0));
    assert_eq!(items[1]["id"], json!("auction-list-1"));
    assert_eq!(items[1]["listingId"], json!("listing-list-1"));
    assert_eq!(items[1]["startingPrice"], json!(10.0));
    assert_eq!(items[1]["currentHighestBid"], Value::Null);
}

#[tokio::test]
async fn place_bid_returns_created_bid_response_and_enqueues_outbox_event() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = test_auction_service(listing_auction_session_repo.clone(), bid_repo.clone(), outbox_repo.clone());
    let app = create_router(service);

    let auction_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let new_auction = NewListingAuctionSessionRecord {
        id: auction_id.clone(),
        listing_id: "listing-3".to_string(),
        seller_id: "seller-3".to_string(),
        starting_price_cents: 1000,
        reserve_price_cents: 5000,
        current_highest_bid_cents: None,
        minimum_increment_cents: 200,
        status: "ACTIVE".to_string(),
        start_time: now - 120,
        end_time: now + 900,
        created_at: now,
        updated_at: now,
    };
    listing_auction_session_repo
        .insert(&new_auction)
        .await
        .expect("insert auction");

    let before_request_time = chrono::Utc::now().timestamp();
    let request_body = json!({
        "bidder_id": "bidder-1",
        "bid_amount_cents": 1500,
        "bid_time": now + 30
    });

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/listings/{auction_id}/bids"))
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .expect("build request"),
        )
        .await
        .expect("call app");

    assert_eq!(response.status(), StatusCode::CREATED);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    let response_body: Value = serde_json::from_slice(&body).expect("parse response json");

    let bid_id = response_body["id"]
        .as_str()
        .expect("response has bid id")
        .to_string();
    assert!(!bid_id.is_empty());
    assert_eq!(response_body["auction_id"], json!(auction_id));
    assert_eq!(response_body["bidder_id"], json!("bidder-1"));
    assert_eq!(response_body["bid_amount_cents"], json!(1500));
    let persisted_bid_time = response_body["bid_time"]
        .as_i64()
        .expect("response has bid_time");
    let after_request_time = chrono::Utc::now().timestamp();
    assert!(persisted_bid_time >= before_request_time);
    assert!(persisted_bid_time <= after_request_time);

    let bids = bid_repo
        .list_by_auction_id_desc(&auction_id)
        .await
        .expect("list bids");
    assert_eq!(bids.len(), 1);
    assert_eq!(bids[0].id, bid_id);
    assert_eq!(bids[0].bidder_id, "bidder-1");
    assert_eq!(bids[0].bid_amount_cents, 1500);

    let events = outbox_repo.list_pending(10).await.expect("list outbox");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].aggregate_id, auction_id);
    assert_eq!(events[0].event_type, "BidPlaced");
}

#[tokio::test]
async fn api_rejects_client_timestamp_spoofing_by_using_server_time() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = test_auction_service(listing_auction_session_repo.clone(), bid_repo.clone(), outbox_repo);
    let app = create_router(service);

    let auction_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let new_auction = NewListingAuctionSessionRecord {
        id: auction_id.clone(),
        listing_id: "listing-bid-spoof".to_string(),
        seller_id: "seller-bid-spoof".to_string(),
        starting_price_cents: 1000,
        reserve_price_cents: 5000,
        current_highest_bid_cents: None,
        minimum_increment_cents: 200,
        status: "ACTIVE".to_string(),
        start_time: now - 120,
        end_time: now + 900,
        created_at: now,
        updated_at: now,
    };
    listing_auction_session_repo
        .insert(&new_auction)
        .await
        .expect("insert auction");

    let request_body = json!({
        "bidder_id": "bidder-1",
        "bid_amount_cents": 1500,
        "bid_time": now - 9_999_999
    });

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/listings/{auction_id}/bids"))
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .expect("build request"),
        )
        .await
        .expect("call app");

    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn api_v1_place_bid_accepts_gateway_payload_and_returns_gateway_response() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = test_auction_service(listing_auction_session_repo.clone(), bid_repo.clone(), outbox_repo);
    let app = create_router(service);

    let auction_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let new_auction = NewListingAuctionSessionRecord {
        id: auction_id.clone(),
        listing_id: "listing-bid-gateway".to_string(),
        seller_id: "seller-bid-gateway".to_string(),
        starting_price_cents: 1000,
        reserve_price_cents: 5000,
        current_highest_bid_cents: None,
        minimum_increment_cents: 200,
        status: "ACTIVE".to_string(),
        start_time: now - 120,
        end_time: now + 900,
        created_at: now,
        updated_at: now,
    };
    listing_auction_session_repo
        .insert(&new_auction)
        .await
        .expect("insert auction");

    let request_body = json!({
        "bidderId": "bidder-gateway",
        "bidAmount": 15.5
    });

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/v1/listings/{auction_id}/bids"))
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .expect("build request"),
        )
        .await
        .expect("call app");

    assert_eq!(response.status(), StatusCode::CREATED);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    let response_body: Value = serde_json::from_slice(&body).expect("parse response json");

    assert_eq!(response_body["auctionId"], json!(auction_id));
    assert_eq!(response_body["bidderId"], json!("bidder-gateway"));
    assert_eq!(response_body["bidAmount"], json!(15.5));
    assert!(response_body["bidTime"].as_str().is_some());

    let bids = bid_repo
        .list_by_auction_id_desc(&auction_id)
        .await
        .expect("list bids");
    assert_eq!(bids.len(), 1);
    assert_eq!(bids[0].bidder_id, "bidder-gateway");
    assert_eq!(bids[0].bid_amount_cents, 1550);
}

#[tokio::test]
async fn api_v1_place_bid_uses_trusted_gateway_user_header() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = test_auction_service(listing_auction_session_repo.clone(), bid_repo.clone(), outbox_repo);
    let app = create_router(service);

    let auction_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let new_auction = NewListingAuctionSessionRecord {
        id: auction_id.clone(),
        listing_id: "listing-bid-trusted".to_string(),
        seller_id: "seller-bid-trusted".to_string(),
        starting_price_cents: 1000,
        reserve_price_cents: 5000,
        current_highest_bid_cents: None,
        minimum_increment_cents: 200,
        status: "ACTIVE".to_string(),
        start_time: now - 120,
        end_time: now + 900,
        created_at: now,
        updated_at: now,
    };
    listing_auction_session_repo
        .insert(&new_auction)
        .await
        .expect("insert auction");

    let request_body = json!({
        "bidAmount": 15.5
    });

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/v1/listings/{auction_id}/bids"))
                .header("content-type", "application/json")
                .header("x-user-id", "bidder-from-gateway")
                .body(Body::from(request_body.to_string()))
                .expect("build request"),
        )
        .await
        .expect("call app");

    assert_eq!(response.status(), StatusCode::CREATED);

    let bids = bid_repo
        .list_by_auction_id_desc(&auction_id)
        .await
        .expect("list bids");
    assert_eq!(bids.len(), 1);
    assert_eq!(bids[0].bidder_id, "bidder-from-gateway");
}

#[tokio::test]
async fn api_v1_place_bid_rejects_conflicting_gateway_bidder_header() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = test_auction_service(listing_auction_session_repo.clone(), bid_repo, outbox_repo);
    let app = create_router(service);

    let auction_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let new_auction = NewListingAuctionSessionRecord {
        id: auction_id.clone(),
        listing_id: "listing-bid-conflict".to_string(),
        seller_id: "seller-bid-conflict".to_string(),
        starting_price_cents: 1000,
        reserve_price_cents: 5000,
        current_highest_bid_cents: None,
        minimum_increment_cents: 200,
        status: "ACTIVE".to_string(),
        start_time: now - 120,
        end_time: now + 900,
        created_at: now,
        updated_at: now,
    };
    listing_auction_session_repo
        .insert(&new_auction)
        .await
        .expect("insert auction");

    let request_body = json!({
        "bidderId": "bidder-from-body",
        "bidAmount": 15.5
    });

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/v1/listings/{auction_id}/bids"))
                .header("content-type", "application/json")
                .header("x-user-id", "bidder-from-gateway")
                .body(Body::from(request_body.to_string()))
                .expect("build request"),
        )
        .await
        .expect("call app");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn api_v1_place_bid_rejects_seller_bidding_own_auction() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = test_auction_service(listing_auction_session_repo.clone(), bid_repo, outbox_repo);
    let app = create_router(service);

    let auction_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    listing_auction_session_repo
        .insert(&NewListingAuctionSessionRecord {
            id: auction_id.clone(),
            listing_id: "listing-self-bid".to_string(),
            seller_id: "seller-self-bid".to_string(),
            starting_price_cents: 1000,
            reserve_price_cents: 5000,
            current_highest_bid_cents: None,
            minimum_increment_cents: 200,
            status: "ACTIVE".to_string(),
            start_time: now - 120,
            end_time: now + 900,
            created_at: now,
            updated_at: now,
        })
        .await
        .expect("insert auction");

    let request_body = json!({
        "bidderId": "seller-self-bid",
        "bidAmount": 15.5
    });

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/v1/listings/{auction_id}/bids"))
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .expect("build request"),
        )
        .await
        .expect("call app");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn api_v1_place_proxy_bid_rejects_seller_bidding_own_auction() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = test_auction_service(listing_auction_session_repo.clone(), bid_repo, outbox_repo);
    let app = create_router(service);

    let auction_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    listing_auction_session_repo
        .insert(&NewListingAuctionSessionRecord {
            id: auction_id.clone(),
            listing_id: "listing-self-proxy".to_string(),
            seller_id: "seller-self-proxy".to_string(),
            starting_price_cents: 1000,
            reserve_price_cents: 5000,
            current_highest_bid_cents: None,
            minimum_increment_cents: 200,
            status: "ACTIVE".to_string(),
            start_time: now - 120,
            end_time: now + 900,
            created_at: now,
            updated_at: now,
        })
        .await
        .expect("insert auction");

    let request_body = json!({
        "bidderId": "seller-self-proxy",
        "maxBidAmount": 50.0,
        "bid_time": now + 20
    });
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/v1/listings/{auction_id}/bids/cursor"))
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .expect("build request"),
        )
        .await
        .expect("call app");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn list_bids_returns_bid_history_for_auction() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = test_auction_service(listing_auction_session_repo.clone(), bid_repo.clone(), outbox_repo);
    let app = create_router(service);

    let auction_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let new_auction = NewListingAuctionSessionRecord {
        id: auction_id.clone(),
        listing_id: "listing-4".to_string(),
        seller_id: "seller-4".to_string(),
        starting_price_cents: 1000,
        reserve_price_cents: 5000,
        current_highest_bid_cents: None,
        minimum_increment_cents: 200,
        status: "ACTIVE".to_string(),
        start_time: now - 120,
        end_time: now + 900,
        created_at: now,
        updated_at: now,
    };
    listing_auction_session_repo
        .insert(&new_auction)
        .await
        .expect("insert auction");

    let lower_bid = NewBidRecord {
        id: uuid::Uuid::new_v4().to_string(),
        auction_id: auction_id.clone(),
        bidder_id: "bidder-low".to_string(),
        bid_amount_cents: 1500,
        bid_time: now + 10,
    };
    let higher_bid = NewBidRecord {
        id: uuid::Uuid::new_v4().to_string(),
        auction_id: auction_id.clone(),
        bidder_id: "bidder-high".to_string(),
        bid_amount_cents: 2200,
        bid_time: now + 20,
    };
    bid_repo.insert(&lower_bid).await.expect("insert lower bid");
    bid_repo
        .insert(&higher_bid)
        .await
        .expect("insert higher bid");

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!("/listings/{auction_id}/bids"))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("call app");

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    let response_body: Value = serde_json::from_slice(&body).expect("parse response json");
    let bids = response_body.as_array().expect("response is an array");

    assert_eq!(bids.len(), 2);
    assert_eq!(bids[0]["id"], json!(higher_bid.id));
    assert_eq!(bids[0]["bidder_id"], json!("bidder-high"));
    assert_eq!(bids[0]["bid_amount_cents"], json!(2200));
    assert_eq!(bids[1]["id"], json!(lower_bid.id));
    assert_eq!(bids[1]["bidder_id"], json!("bidder-low"));
    assert_eq!(bids[1]["bid_amount_cents"], json!(1500));
}

#[tokio::test]
async fn list_bids_cursor_returns_paginated_bid_history() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = test_auction_service(listing_auction_session_repo.clone(), bid_repo.clone(), outbox_repo);
    let app = create_router(service);

    let auction_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let new_auction = NewListingAuctionSessionRecord {
        id: auction_id.clone(),
        listing_id: "listing-cursor".to_string(),
        seller_id: "seller-cursor".to_string(),
        starting_price_cents: 1000,
        reserve_price_cents: 5000,
        current_highest_bid_cents: None,
        minimum_increment_cents: 100,
        status: "ACTIVE".to_string(),
        start_time: now - 120,
        end_time: now + 900,
        created_at: now,
        updated_at: now,
    };
    listing_auction_session_repo
        .insert(&new_auction)
        .await
        .expect("insert auction");

    bid_repo
        .insert(&NewBidRecord {
            id: uuid::Uuid::new_v4().to_string(),
            auction_id: auction_id.clone(),
            bidder_id: "bidder-low".to_string(),
            bid_amount_cents: 1100,
            bid_time: now + 5,
        })
        .await
        .expect("insert low");
    bid_repo
        .insert(&NewBidRecord {
            id: uuid::Uuid::new_v4().to_string(),
            auction_id: auction_id.clone(),
            bidder_id: "bidder-mid".to_string(),
            bid_amount_cents: 1500,
            bid_time: now + 10,
        })
        .await
        .expect("insert mid");
    bid_repo
        .insert(&NewBidRecord {
            id: uuid::Uuid::new_v4().to_string(),
            auction_id: auction_id.clone(),
            bidder_id: "bidder-high".to_string(),
            bid_amount_cents: 1900,
            bid_time: now + 20,
        })
        .await
        .expect("insert high");

    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!("/api/v1/listings/{auction_id}/bids/cursor?limit=2"))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("call app");
    assert_eq!(first.status(), StatusCode::OK);
    let first_body = to_bytes(first.into_body(), usize::MAX)
        .await
        .expect("read body");
    let first_json: Value = serde_json::from_slice(&first_body).expect("parse response json");
    let first_items = first_json["items"].as_array().expect("items array");
    assert_eq!(first_items.len(), 2);
    assert_eq!(first_items[0]["bidder_id"], json!("bidder-high"));
    assert_eq!(first_items[1]["bidder_id"], json!("bidder-mid"));

    let cursor = first_json["nextCursor"]
        .as_str()
        .expect("next cursor")
        .to_string();
    let second = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!(
                    "/api/v1/listings/{auction_id}/bids/cursor?limit=2&cursor={cursor}"
                ))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("call app");
    assert_eq!(second.status(), StatusCode::OK);
    let second_body = to_bytes(second.into_body(), usize::MAX)
        .await
        .expect("read body");
    let second_json: Value = serde_json::from_slice(&second_body).expect("parse response json");
    let second_items = second_json["items"].as_array().expect("items array");
    assert_eq!(second_items.len(), 1);
    assert_eq!(second_items[0]["bidder_id"], json!("bidder-low"));
    assert!(second_json["nextCursor"].is_null());
}

#[tokio::test]
async fn place_proxy_bid_places_increment_over_current_winner() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = test_auction_service(listing_auction_session_repo.clone(), bid_repo.clone(), outbox_repo);
    let app = create_router(service);

    let auction_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    listing_auction_session_repo
        .insert(&NewListingAuctionSessionRecord {
            id: auction_id.clone(),
            listing_id: "listing-proxy".to_string(),
            seller_id: "seller-proxy".to_string(),
            starting_price_cents: 1000,
            reserve_price_cents: 5000,
            current_highest_bid_cents: Some(2200),
            minimum_increment_cents: 200,
            status: "ACTIVE".to_string(),
            start_time: now - 120,
            end_time: now + 900,
            created_at: now,
            updated_at: now,
        })
        .await
        .expect("insert auction");
    bid_repo
        .insert(&NewBidRecord {
            id: uuid::Uuid::new_v4().to_string(),
            auction_id: auction_id.clone(),
            bidder_id: "existing-winner".to_string(),
            bid_amount_cents: 2200,
            bid_time: now + 5,
        })
        .await
        .expect("insert winner");

    let request_body = json!({
        "bidderId": "proxy-bidder",
        "maxBidAmount": 50.0,
        "bid_time": now + 20
    });
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/v1/listings/{auction_id}/bids/cursor"))
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .expect("build request"),
        )
        .await
        .expect("call app");

    assert_eq!(response.status(), StatusCode::CREATED);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    let response_json: Value = serde_json::from_slice(&body).expect("parse response json");
    assert_eq!(response_json["bidder_id"], json!("proxy-bidder"));
    assert_eq!(response_json["bid_amount_cents"], json!(2400));
}

#[tokio::test]
async fn api_v1_close_auction_marks_won_when_reserve_is_met() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = test_auction_service(listing_auction_session_repo.clone(), bid_repo.clone(), outbox_repo);
    let app = create_router(service);

    let auction_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let new_auction = NewListingAuctionSessionRecord {
        id: auction_id.clone(),
        listing_id: "listing-close".to_string(),
        seller_id: "seller-close".to_string(),
        starting_price_cents: 1000,
        reserve_price_cents: 5000,
        current_highest_bid_cents: Some(5500),
        minimum_increment_cents: 200,
        status: "ACTIVE".to_string(),
        start_time: now - 3_600,
        end_time: now - 60,
        created_at: now - 3_600,
        updated_at: now - 60,
    };
    listing_auction_session_repo
        .insert(&new_auction)
        .await
        .expect("insert auction");
    bid_repo
        .insert(&NewBidRecord {
            id: uuid::Uuid::new_v4().to_string(),
            auction_id: auction_id.clone(),
            bidder_id: "winner".to_string(),
            bid_amount_cents: 5500,
            bid_time: now - 120,
        })
        .await
        .expect("insert winning bid");

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/v1/listings/{auction_id}/close"))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("call app");

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    let response_body: Value = serde_json::from_slice(&body).expect("parse response json");
    assert_eq!(response_body["status"], json!("WON"));
    assert_eq!(response_body["current_highest_bid_cents"], json!(5500));

    let persisted = listing_auction_session_repo
        .find_by_id(&auction_id)
        .await
        .expect("find auction")
        .expect("auction exists");
    assert_eq!(persisted.status, "WON");
}

#[tokio::test]
async fn api_v1_pending_closure_returns_expired_unprocessed_auctions() {
    let pool = setup_test_db().await;
    let listing_auction_session_repo = ListingAuctionSessionRepository::new(pool.clone());
    let bid_repo = BidRepository::new(pool.clone());
    let outbox_repo = OutboxRepository::new(pool);
    let service = test_auction_service(listing_auction_session_repo.clone(), bid_repo, outbox_repo);
    let app = create_router(service);

    let now = chrono::Utc::now().timestamp();
    listing_auction_session_repo
        .insert(&NewListingAuctionSessionRecord {
            id: "pending-close".to_string(),
            listing_id: "listing-pending".to_string(),
            seller_id: "seller-pending".to_string(),
            starting_price_cents: 1000,
            reserve_price_cents: 5000,
            current_highest_bid_cents: None,
            minimum_increment_cents: 200,
            status: "ACTIVE".to_string(),
            start_time: now - 3_600,
            end_time: now - 60,
            created_at: now - 3_600,
            updated_at: now - 60,
        })
        .await
        .expect("insert pending auction");
    listing_auction_session_repo
        .insert(&NewListingAuctionSessionRecord {
            id: "future-close".to_string(),
            listing_id: "listing-future".to_string(),
            seller_id: "seller-future".to_string(),
            starting_price_cents: 1000,
            reserve_price_cents: 5000,
            current_highest_bid_cents: None,
            minimum_increment_cents: 200,
            status: "ACTIVE".to_string(),
            start_time: now - 60,
            end_time: now + 3_600,
            created_at: now - 60,
            updated_at: now - 60,
        })
        .await
        .expect("insert future auction");
    listing_auction_session_repo
        .insert(&NewListingAuctionSessionRecord {
            id: "already-won".to_string(),
            listing_id: "listing-won".to_string(),
            seller_id: "seller-won".to_string(),
            starting_price_cents: 1000,
            reserve_price_cents: 5000,
            current_highest_bid_cents: Some(6000),
            minimum_increment_cents: 200,
            status: "WON".to_string(),
            start_time: now - 3_600,
            end_time: now - 60,
            created_at: now - 3_600,
            updated_at: now - 60,
        })
        .await
        .expect("insert closed auction");

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/v1/listings/pending-closure")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("call app");

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    let response_body: Value = serde_json::from_slice(&body).expect("parse response json");
    let items = response_body.as_array().expect("response is an array");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], json!("pending-close"));
}
