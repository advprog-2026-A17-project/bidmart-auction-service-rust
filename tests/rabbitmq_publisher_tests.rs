//! Tests for rabbitmq_outbox_publisher pure functions and constructor.

use bidmart_auction_service_rust::persistence::models::OutboxEventRecord;
use bidmart_auction_service_rust::scheduler::rabbitmq_outbox_publisher::{
    RabbitMqOutboxPublisher, build_event_envelope, event_type_to_routing_key,
};

fn sample_event(event_type: &str, payload: &str) -> OutboxEventRecord {
    OutboxEventRecord {
        id: "evt-1".to_string(),
        aggregate_id: "agg-1".to_string(),
        event_type: event_type.to_string(),
        payload: payload.to_string(),
        published: false,
        published_at: None,
        created_at: 1700000000,
        updated_at: 1700000000,
    }
}

#[test]
fn routing_key_auction_created() {
    assert_eq!(
        event_type_to_routing_key("AuctionCreated"),
        "auction.created.v1"
    );
}

#[test]
fn routing_key_auction_ended() {
    assert_eq!(
        event_type_to_routing_key("AuctionEnded"),
        "auction.ended.v1"
    );
}

#[test]
fn routing_key_bid_placed() {
    assert_eq!(
        event_type_to_routing_key("BidPlaced"),
        "auction.bid-placed.v1"
    );
}

#[test]
fn routing_key_outbid() {
    assert_eq!(event_type_to_routing_key("Outbid"), "auction.outbid.v1");
}

#[test]
fn routing_key_unknown_passes_through() {
    assert_eq!(event_type_to_routing_key("FooBar"), "FooBar");
}

#[test]
fn build_envelope_with_json_payload() {
    let event = sample_event("BidPlaced", r#"{"bidId":"b1","amount":5000}"#);
    let routing_key = event_type_to_routing_key(&event.event_type);
    let envelope_bytes = build_event_envelope(&event, &routing_key).unwrap();
    let envelope: serde_json::Value = serde_json::from_slice(&envelope_bytes).unwrap();

    assert_eq!(envelope["eventId"], "evt-1");
    assert_eq!(envelope["aggregateId"], "agg-1");
    assert_eq!(envelope["eventType"], "auction.bid-placed.v1");
    assert_eq!(envelope["eventVersion"], 1);
    assert_eq!(envelope["payload"]["bidId"], "b1");
    assert_eq!(envelope["createdAt"], 1700000000);
}

#[test]
fn build_envelope_with_invalid_json_falls_back_to_string() {
    let event = sample_event("AuctionEnded", "not-json");
    let routing_key = event_type_to_routing_key(&event.event_type);
    let envelope_bytes = build_event_envelope(&event, &routing_key).unwrap();
    let envelope: serde_json::Value = serde_json::from_slice(&envelope_bytes).unwrap();
    assert_eq!(envelope["payload"], "not-json");
}

#[test]
fn build_envelope_with_empty_payload() {
    let event = sample_event("AuctionCreated", "");
    let routing_key = event_type_to_routing_key(&event.event_type);
    let envelope_bytes = build_event_envelope(&event, &routing_key).unwrap();
    let envelope: serde_json::Value = serde_json::from_slice(&envelope_bytes).unwrap();
    assert_eq!(envelope["eventType"], "auction.created.v1");
    assert_eq!(envelope["payload"], "");
}

#[test]
fn rabbitmq_publisher_new_creates_instance() {
    let publisher =
        RabbitMqOutboxPublisher::new("amqp://guest:guest@localhost:5672/%2f", "bidmart.events");
    let _fn = publisher.publisher_fn();
}

#[tokio::test]
async fn rabbitmq_publisher_publish_fails_when_no_connection() {
    let publisher =
        RabbitMqOutboxPublisher::new("amqp://guest:guest@localhost:59999/%2f", "test.exchange");
    let event = sample_event("BidPlaced", r#"{"test":true}"#);
    let result = publisher.publish(event).await;
    // Should fail because no RabbitMQ server is running on port 59999
    assert!(result.is_err());
}
