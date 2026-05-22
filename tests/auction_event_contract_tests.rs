mod common;

use bidmart_auction_service_rust::persistence::repositories::{
    BidRepository, ListingAuctionSessionRepository, OutboxRepository,
};
use bidmart_auction_service_rust::server::{connect_pool, run_migrations};
use bidmart_auction_service_rust::service::auction_service::{
    AuctionService, CreateAuctionCommand,
};
use serde_json::Value;
use sqlx::AnyPool;

async fn setup() -> (AnyPool, AuctionService, OutboxRepository) {
    let pool = connect_pool("sqlite::memory:").await.expect("connect db");
    run_migrations(&pool).await.expect("run migrations");
    let outbox = OutboxRepository::new(pool.clone());
    let service = AuctionService::new_with_catalog(
        ListingAuctionSessionRepository::new(pool.clone()),
        BidRepository::new(pool.clone()),
        outbox.clone(),
        common::always_active_catalog(),
    );
    (pool, service, outbox)
}

fn command(listing_id: &str, now: i64) -> CreateAuctionCommand {
    CreateAuctionCommand {
        listing_id: listing_id.to_string(),
        seller_id: "seller-1".to_string(),
        auction_type: "ENGLISH".to_string(),
        starting_price_cents: 1000,
        reserve_price_cents: 1000,
        minimum_increment_cents: 100,
        start_time: now,
        end_time: now + 600,
    }
}

fn payload(
    event_type: &str,
    outbox: &[bidmart_auction_service_rust::persistence::models::OutboxEventRecord],
) -> Value {
    let event = outbox
        .iter()
        .find(|event| event.event_type == event_type)
        .unwrap_or_else(|| panic!("missing event {event_type}"));
    serde_json::from_str(&event.payload).expect("event payload is json")
}

fn assert_fields(payload: &Value, fields: &[&str]) {
    for field in fields {
        assert!(
            payload.get(field).is_some(),
            "payload missing required field {field}: {payload}"
        );
    }
}

#[tokio::test]
async fn auction_created_payload_keeps_required_contract_fields() {
    let (_pool, service, outbox) = setup().await;
    let now = chrono::Utc::now().timestamp();

    service
        .create_auction(command("contract-created", now))
        .await
        .expect("create auction");

    let events = outbox.list_pending(10).await.expect("outbox");
    let payload = payload("AuctionCreated", &events);
    assert_fields(
        &payload,
        &[
            "auctionId",
            "listingId",
            "sellerId",
            "startingPrice",
            "reservePrice",
            "minimumIncrement",
            "status",
            "startTime",
            "endTime",
            "createdAt",
        ],
    );
}

#[tokio::test]
async fn bid_and_outbid_payloads_keep_required_contract_fields() {
    let (_pool, service, outbox) = setup().await;
    let now = chrono::Utc::now().timestamp();
    let auction = service
        .create_auction(command("contract-bids", now))
        .await
        .expect("create auction");

    service
        .place_bid_and_persist(&auction.id, "buyer-1", 1000, now + 10)
        .await
        .expect("first bid");
    service
        .place_bid_and_persist(&auction.id, "buyer-2", 1200, now + 20)
        .await
        .expect("second bid");

    let events = outbox.list_pending(20).await.expect("outbox");
    assert_fields(
        &payload("BidPlaced", &events),
        &[
            "auctionId",
            "listingId",
            "sellerId",
            "bidId",
            "bidderId",
            "amountCents",
            "bidTime",
        ],
    );
    assert_fields(
        &payload("Outbid", &events),
        &[
            "auctionId",
            "listingId",
            "sellerId",
            "previousBidderId",
            "amountCents",
            "currentPrice",
            "outbidAt",
        ],
    );
}

#[tokio::test]
async fn auction_ended_payload_keeps_required_contract_fields() {
    let (pool, service, outbox) = setup().await;
    let now = chrono::Utc::now().timestamp();
    let auction = service
        .create_auction(command("contract-ended", now))
        .await
        .expect("create auction");
    service
        .place_bid_and_persist(&auction.id, "buyer-1", 1000, now + 10)
        .await
        .expect("bid");
    sqlx::query("UPDATE listings SET end_time = $1 WHERE id = $2")
        .bind(now - 1)
        .bind(&auction.id)
        .execute(&pool)
        .await
        .expect("expire auction");

    service
        .close_auction(&auction.id)
        .await
        .expect("close auction");

    let events = outbox.list_pending(20).await.expect("outbox");
    assert_fields(
        &payload("AuctionEnded", &events),
        &[
            "auctionId",
            "listingId",
            "sellerId",
            "status",
            "winnerId",
            "finalPrice",
            "reserveMet",
            "endedAt",
        ],
    );
}
