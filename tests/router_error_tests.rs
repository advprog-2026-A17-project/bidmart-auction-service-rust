use axum::http::StatusCode;

use bidmart_auction_service_rust::http::router::ApiError;
use bidmart_auction_service_rust::listing_auction_session::{BidError, Money};
use bidmart_auction_service_rust::service::auction_service::{
    CloseListingAuctionSessionError, CreateAuctionError, GetListingAuctionSessionError,
    ListBidsError, ListListingAuctionSessionsError, ListPendingClosureError, PlaceBidError,
};

// ============================================
// CreateAuctionError conversions
// ============================================

#[test]
fn create_auction_invalid_input_maps_to_400() {
    let error: ApiError = CreateAuctionError::InvalidInput("bad data".to_string()).into();
    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert!(error.message.contains("bad data"));
}

#[test]
fn create_auction_database_error_maps_to_500() {
    let error: ApiError = CreateAuctionError::DatabaseError("db failure".to_string()).into();
    assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
}

// ============================================
// GetListingAuctionSessionError conversions
// ============================================

#[test]
fn get_auction_db_error_maps_to_500() {
    let error: ApiError =
        GetListingAuctionSessionError::DatabaseError("query failed".to_string()).into();
    assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
}

// ============================================
// ListListingAuctionSessionsError conversions
// ============================================

#[test]
fn list_auctions_db_error_maps_to_500() {
    let error: ApiError = ListListingAuctionSessionsError::DatabaseError("fail".to_string()).into();
    assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
}

// ============================================
// ListPendingClosureError conversions
// ============================================

#[test]
fn pending_closure_db_error_maps_to_500() {
    let error: ApiError = ListPendingClosureError::DatabaseError("fail".to_string()).into();
    assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
}

// ============================================
// CloseListingAuctionSessionError conversions
// ============================================

#[test]
fn close_not_found_maps_to_404() {
    let error: ApiError = CloseListingAuctionSessionError::AuctionNotFound.into();
    assert_eq!(error.status, StatusCode::NOT_FOUND);
}

#[test]
fn close_not_ended_maps_to_400() {
    let error: ApiError = CloseListingAuctionSessionError::AuctionNotEnded.into();
    assert_eq!(error.status, StatusCode::BAD_REQUEST);
}

#[test]
fn close_wallet_error_maps_to_402() {
    let error: ApiError =
        CloseListingAuctionSessionError::WalletError("insufficient".to_string()).into();
    assert_eq!(error.status, StatusCode::PAYMENT_REQUIRED);
}

#[test]
fn close_db_error_maps_to_500() {
    let error: ApiError = CloseListingAuctionSessionError::DatabaseError("fail".to_string()).into();
    assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
}

// ============================================
// PlaceBidError conversions
// ============================================

#[test]
fn place_bid_not_found_maps_to_404() {
    let error: ApiError = PlaceBidError::AuctionNotFound.into();
    assert_eq!(error.status, StatusCode::NOT_FOUND);
}

#[test]
fn place_bid_bid_error_maps_to_400() {
    let error: ApiError = PlaceBidError::BidError(BidError::BidTooLow {
        minimum: Money::from_cents(1000),
    })
    .into();
    assert_eq!(error.status, StatusCode::BAD_REQUEST);
}

#[test]
fn place_bid_catalog_error_maps_to_400() {
    let error: ApiError = PlaceBidError::CatalogError("listing inactive".to_string()).into();
    assert_eq!(error.status, StatusCode::BAD_REQUEST);
}

#[test]
fn place_bid_wallet_error_maps_to_402() {
    let error: ApiError = PlaceBidError::WalletError("insufficient balance".to_string()).into();
    assert_eq!(error.status, StatusCode::PAYMENT_REQUIRED);
}

#[test]
fn place_bid_db_error_maps_to_500() {
    let error: ApiError = PlaceBidError::DatabaseError("fail".to_string()).into();
    assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
}

// ============================================
// ListBidsError conversions
// ============================================

#[test]
fn list_bids_invalid_input_maps_to_400() {
    let error: ApiError = ListBidsError::InvalidInput("bad cursor".to_string()).into();
    assert_eq!(error.status, StatusCode::BAD_REQUEST);
}

#[test]
fn list_bids_db_error_maps_to_500() {
    let error: ApiError = ListBidsError::DatabaseError("fail".to_string()).into();
    assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
}

// ============================================
// ApiError construction helpers
// ============================================

#[test]
fn api_error_bad_request_creates_400() {
    let error = ApiError::bad_request("test");
    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert_eq!(error.message, "test");
}

#[test]
fn api_error_not_found_creates_404() {
    let error = ApiError::not_found("missing");
    assert_eq!(error.status, StatusCode::NOT_FOUND);
    assert_eq!(error.message, "missing");
}

#[test]
fn api_error_forbidden_creates_403() {
    let error = ApiError::forbidden("no access");
    assert_eq!(error.status, StatusCode::FORBIDDEN);
    assert_eq!(error.message, "no access");
}
