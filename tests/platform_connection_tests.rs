#[test]
fn docker_compose_routes_gateway_auction_traffic_to_rust_service() {
    let compose = std::fs::read_to_string("../docker-compose.yml").expect("read docker compose");

    assert!(compose.contains("AUCTION_SERVICE_URL: http://auction-service:8082"));
    assert!(compose.contains("context: ./bidmart-auction-service-rust"));
    assert!(compose.contains("BIND_ADDRESS: 0.0.0.0:8082"));
    assert!(compose.contains("DATABASE_URL: sqlite:///data/bidmart-auction.db"));
    assert!(compose.contains("auction-rust-data:/data"));
}

#[test]
fn api_docs_describe_rust_auction_service_as_http_connected() {
    let docs = std::fs::read_to_string("../API_DOCS.md").expect("read api docs");

    assert!(docs.contains("## bidmart-auction-service-rust"));
    assert!(docs.contains("Active auction service implementation"));
    assert!(docs.contains("Gateway path: /api/v1/auctions"));
    assert!(!docs.contains("No HTTP API yet"));
}
