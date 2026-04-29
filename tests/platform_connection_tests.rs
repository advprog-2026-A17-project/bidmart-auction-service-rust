#[test]
fn api_docs_describe_rust_auction_service_as_http_connected() {
    let docs = std::fs::read_to_string("../API_DOCS.md").expect("read api docs");

    assert!(docs.contains("## bidmart-auction-service-rust"));
    assert!(docs.contains("Active auction service implementation"));
    assert!(docs.contains("Gateway path: /api/v1/auctions"));
    assert!(!docs.contains("No HTTP API yet"));
}

#[test]
fn dockerfile_uses_rust_builder_supported_by_locked_dependencies() {
    let dockerfile = std::fs::read_to_string("Dockerfile").expect("read dockerfile");

    assert!(dockerfile.contains("FROM rust:1.86-bookworm AS builder"));
}
