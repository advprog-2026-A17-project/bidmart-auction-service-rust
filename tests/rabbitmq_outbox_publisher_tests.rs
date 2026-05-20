use std::time::Duration;

use bidmart_auction_service_rust::persistence::models::OutboxEventRecord;
use bidmart_auction_service_rust::scheduler::rabbitmq_outbox_publisher::{
    build_event_envelope, event_type_to_routing_key, RabbitMqOutboxPublisher,
};

fn sample_event(payload: &str) -> OutboxEventRecord {
    OutboxEventRecord {
        id: "event-1".to_string(),
        aggregate_id: "auction-1".to_string(),
        event_type: "AuctionEnded".to_string(),
        payload: payload.to_string(),
        published: false,
        published_at: None,
        created_at: 123,
        updated_at: 123,
    }
}

#[test]
fn event_type_to_routing_key_maps_known_types() {
    assert_eq!(event_type_to_routing_key("AuctionCreated"), "auction.created.v1");
    assert_eq!(event_type_to_routing_key("AuctionEnded"), "auction.ended.v1");
    assert_eq!(event_type_to_routing_key("BidPlaced"), "auction.bid-placed.v1");
    assert_eq!(event_type_to_routing_key("Outbid"), "auction.outbid.v1");
    assert_eq!(event_type_to_routing_key("Custom"), "Custom");
}

#[test]
fn build_event_envelope_handles_json_payload() {
    let event = sample_event("{\"value\":123}");
    let body = build_event_envelope(&event, "auction.ended.v1").expect("build envelope");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("parse json");
    assert_eq!(json["eventType"], "auction.ended.v1");
    assert_eq!(json["payload"]["value"], 123);
}

#[test]
fn build_event_envelope_handles_plain_payload() {
    let event = sample_event("raw-payload");
    let body = build_event_envelope(&event, "auction.ended.v1").expect("build envelope");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("parse json");
    assert_eq!(json["payload"], "raw-payload");
}

#[tokio::test]
async fn publish_returns_error_on_bad_connection() {
    let publisher = RabbitMqOutboxPublisher::new("amqp://127.0.0.1:1/%2f", "bidmart.events");
    let event = sample_event("{}");
    let result = tokio::time::timeout(Duration::from_secs(1), publisher.publish(event)).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_err());
}

#[tokio::test]
async fn publisher_fn_propagates_publish_error() {
    let publisher = RabbitMqOutboxPublisher::new("amqp://127.0.0.1:1/%2f", "bidmart.events");
    let publish = publisher.publisher_fn();
    let event = sample_event("{}");
    let result = tokio::time::timeout(Duration::from_secs(1), publish(event)).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_err());
}
