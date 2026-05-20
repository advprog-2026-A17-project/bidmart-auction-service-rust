//! Tests for WalletClientError and CatalogClientError Display/conversion paths.

use bidmart_auction_service_rust::client::wallet_client::*;
use bidmart_auction_service_rust::client::catalog_client::*;

// ============================
// WalletClientError Display
// ============================

#[test]
fn wallet_error_insufficient_balance_display() {
    let e = WalletClientError::InsufficientBalance("low funds".into());
    assert!(format!("{e}").contains("Insufficient"));
}

#[test]
fn wallet_error_service_display() {
    let e = WalletClientError::ServiceError("timeout".into());
    assert!(format!("{e}").contains("service error"));
}

#[test]
fn wallet_error_network_display() {
    let e = WalletClientError::NetworkError("refused".into());
    assert!(format!("{e}").contains("Network"));
}

#[test]
fn wallet_grpc_client_empty_endpoint() {
    let result = GrpcWalletClient::new("");
    assert!(result.is_err());
}

#[test]
fn wallet_grpc_client_whitespace_endpoint() {
    let result = GrpcWalletClient::new("   ");
    assert!(result.is_err());
}

#[test]
fn wallet_grpc_client_valid_endpoint() {
    let result = GrpcWalletClient::new("http://localhost:9090");
    assert!(result.is_ok());
}

// ============================
// CatalogClientError Display
// ============================

#[test]
fn catalog_error_not_found_display() {
    let e = CatalogClientError::ListingNotFound("listing-1".into());
    assert!(format!("{e}").contains("not found"));
}

#[test]
fn catalog_error_service_display() {
    let e = CatalogClientError::ServiceError("timeout".into());
    assert!(format!("{e}").contains("service error"));
}

#[test]
fn catalog_error_network_display() {
    let e = CatalogClientError::NetworkError("refused".into());
    assert!(format!("{e}").contains("Network"));
}

#[test]
fn catalog_grpc_client_empty_endpoint() {
    let result = GrpcCatalogClient::new("");
    assert!(result.is_err());
}

#[test]
fn catalog_grpc_client_whitespace_endpoint() {
    let result = GrpcCatalogClient::new("   ");
    assert!(result.is_err());
}

#[test]
fn catalog_grpc_client_valid_endpoint() {
    let result = GrpcCatalogClient::new("http://localhost:9091");
    assert!(result.is_ok());
}

// ============================
// HttpWalletClient construction
// ============================

#[test]
fn http_wallet_client_invalid_url() {
    let result = HttpWalletClient::new("not-a-url");
    assert!(result.is_err());
}

#[test]
fn http_wallet_client_https_rejected() {
    let result = HttpWalletClient::new("https://localhost:8080");
    assert!(result.is_err());
}

#[test]
fn http_wallet_client_valid_url() {
    let result = HttpWalletClient::new("http://localhost:8080");
    assert!(result.is_ok());
}

// ============================
// HttpCatalogClient construction
// ============================

#[test]
fn http_catalog_client_invalid_url() {
    let result = HttpCatalogClient::new("not-a-url");
    assert!(result.is_err());
}

#[test]
fn http_catalog_client_valid_url() {
    let result = HttpCatalogClient::new("http://localhost:8080");
    assert!(result.is_ok());
}

// ============================
// gRPC connection failure paths
// ============================

#[tokio::test]
async fn grpc_wallet_hold_fails_on_unreachable_server() {
    let client = GrpcWalletClient::new("http://localhost:59998").unwrap();
    let req = HoldFundsRequest {
        user_id: "u1".into(),
        role: None,
        hold_id: "h1".into(),
        auction_id: "a1".into(),
        bid_id: "b1".into(),
        amount: 1000,
        expires_at: "2099-01-01T00:00:00Z".into(),
    };
    let result = client.hold_funds(req).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn grpc_wallet_release_fails_on_unreachable_server() {
    let client = GrpcWalletClient::new("http://localhost:59998").unwrap();
    let result = client.release_hold("hold-1").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn grpc_wallet_convert_fails_on_unreachable_server() {
    let client = GrpcWalletClient::new("http://localhost:59998").unwrap();
    let result = client.convert_hold_to_payment("hold-1").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn grpc_catalog_fails_on_unreachable_server() {
    let client = GrpcCatalogClient::new("http://localhost:59997").unwrap();
    let result = client.get_listing_summary("listing-1").await;
    assert!(result.is_err());
}

