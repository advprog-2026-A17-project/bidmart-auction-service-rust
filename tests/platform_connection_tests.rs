#[test]
fn api_docs_describe_rust_auction_service_as_http_connected() {
    let router = std::fs::read_to_string("src/http/router.rs").expect("read router");

    assert!(router.contains("/api/v1/auctions"));
    assert!(router.contains("/api/v1/auctions/pending-closure"));
    assert!(router.contains("/api/v1/auctions/:auction_id"));
    assert!(router.contains("/api/v1/auctions/:auction_id/close"));
    assert!(router.contains("/api/v1/auctions/:auction_id/bids"));
}

#[test]
fn dockerfile_uses_rust_builder_supported_by_locked_dependencies() {
    let dockerfile = std::fs::read_to_string("Dockerfile").expect("read dockerfile");

    assert!(dockerfile.contains("FROM rust:1.88-bookworm AS builder"));
    assert!(dockerfile.contains("cargo build --release --locked"));
}

#[test]
fn runtime_database_layer_supports_postgresql_urls_from_compose() {
    let cargo = std::fs::read_to_string("Cargo.toml").expect("read cargo");
    let server = std::fs::read_to_string("src/server.rs").expect("read server");
    let repositories =
        std::fs::read_to_string("src/persistence/repositories.rs").expect("read repositories");

    assert!(cargo.contains("\"postgres\""));
    assert!(cargo.contains("\"any\""));
    assert!(server.contains("AnyPoolOptions"));
    assert!(server.contains("install_default_drivers"));
    assert!(server.contains("postgresql://"));
    assert!(repositories.contains("AnyPool"));
    assert!(repositories.contains("$1"));
    assert!(!server.contains("SqliteConnectOptions"));
}
