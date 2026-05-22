use bidmart_auction_service_rust::persistence::models::OutboxEventRecord;
use bidmart_auction_service_rust::scheduler::rabbitmq_outbox_publisher::{
    build_event_envelope, event_type_to_routing_key,
};

fn event(event_type: &str, payload: &str) -> OutboxEventRecord {
    OutboxEventRecord {
        id: "evt-1".to_string(),
        aggregate_id: "agg-1".to_string(),
        event_type: event_type.to_string(),
        payload: payload.to_string(),
        published: false,
        published_at: None,
        created_at: 1000,
        updated_at: 1000,
    }
}

// ============================================
// Routing key mapping – exhaustive coverage
// ============================================

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
    assert_eq!(event_type_to_routing_key("UnknownEvent"), "UnknownEvent");
}

#[test]
fn routing_key_empty_passes_through() {
    assert_eq!(event_type_to_routing_key(""), "");
}

// ============================================
// Event envelope building – edge cases
// ============================================

#[test]
fn envelope_contains_event_id_and_aggregate_id() {
    let e = event("BidPlaced", r#"{"key":"value"}"#);
    let body = build_event_envelope(&e, "auction.bid-placed.v1").unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["eventId"], "evt-1");
    assert_eq!(json["aggregateId"], "agg-1");
}

#[test]
fn envelope_event_type_uses_routing_key_not_domain_type() {
    let e = event("AuctionEnded", "{}");
    let body = build_event_envelope(&e, "auction.ended.v1").unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["eventType"], "auction.ended.v1");
}

#[test]
fn envelope_version_is_always_one() {
    let e = event("BidPlaced", "{}");
    let body = build_event_envelope(&e, "auction.bid-placed.v1").unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["eventVersion"], 1);
}

#[test]
fn envelope_includes_created_at() {
    let e = event("BidPlaced", "{}");
    let body = build_event_envelope(&e, "auction.bid-placed.v1").unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["createdAt"], 1000);
}

#[test]
fn envelope_parses_json_payload_as_object() {
    let e = event("BidPlaced", r#"{"listingId":"l-1","amount":5000}"#);
    let body = build_event_envelope(&e, "auction.bid-placed.v1").unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["payload"].is_object());
    assert_eq!(json["payload"]["listingId"], "l-1");
    assert_eq!(json["payload"]["amount"], 5000);
}

#[test]
fn envelope_treats_invalid_json_payload_as_string() {
    let e = event("AuctionEnded", "not valid json {{ }}");
    let body = build_event_envelope(&e, "auction.ended.v1").unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["payload"].is_string());
    assert_eq!(json["payload"], "not valid json {{ }}");
}

#[test]
fn envelope_handles_empty_payload() {
    let e = event("AuctionEnded", "");
    let body = build_event_envelope(&e, "auction.ended.v1").unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    // Empty string is not valid JSON, so it should be wrapped as a string value
    assert_eq!(json["payload"], "");
}

#[test]
fn envelope_handles_array_payload() {
    let e = event("BidPlaced", r#"[1,2,3]"#);
    let body = build_event_envelope(&e, "auction.bid-placed.v1").unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["payload"].is_array());
}

#[test]
fn envelope_handles_numeric_payload() {
    let e = event("BidPlaced", "42");
    let body = build_event_envelope(&e, "auction.bid-placed.v1").unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["payload"], 42);
}

#[test]
fn envelope_handles_nested_json_payload() {
    let payload = r#"{"auction":{"id":"a-1","bids":[{"amount":100},{"amount":200}]}}"#;
    let e = event("BidPlaced", payload);
    let body = build_event_envelope(&e, "auction.bid-placed.v1").unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["payload"]["auction"]["bids"].as_array().unwrap().len(),
        2
    );
}
