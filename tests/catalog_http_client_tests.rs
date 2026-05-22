use axum::extract::Path;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;
use tokio::net::TcpListener;

use bidmart_auction_service_rust::client::{
    CatalogClient, CatalogClientError, GrpcCatalogClient, HttpCatalogClient,
};

async fn serve_response(status: StatusCode, body: serde_json::Value) -> String {
    let app = Router::new().route(
        "/api/v1/catalogue/listings/:listing_id/summary",
        get(move |Path(_): Path<String>| async move { (status, Json(body)) }),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind catalogue test server");
    let address = listener.local_addr().expect("read local address");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve catalogue test server");
    });

    format!("http://{address}")
}

#[tokio::test]
async fn http_catalog_client_fetches_listing_summary_from_catalogue_api() {
    let app = Router::new().route(
        "/api/v1/catalogue/listings/:listing_id/summary",
        get(|Path(listing_id): Path<String>| async move {
            if listing_id == "listing-http-1" {
                (
                    StatusCode::OK,
                    Json(json!({
                        "id": listing_id,
                        "sellerId": "seller-http-1",
                        "status": "ACTIVE"
                    })),
                )
            } else {
                (
                    StatusCode::NOT_FOUND,
                    Json(json!({
                        "message": "listing not found"
                    })),
                )
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind catalogue test server");
    let address = listener.local_addr().expect("read local address");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve catalogue test server");
    });

    let client = HttpCatalogClient::new(format!("http://{address}")).expect("create client");
    let summary = client
        .get_listing_summary("listing-http-1")
        .await
        .expect("fetch listing summary");

    assert_eq!(summary.id, "listing-http-1");
    assert_eq!(summary.seller_id, "seller-http-1");
    assert_eq!(summary.status, "ACTIVE");
}

#[tokio::test]
async fn http_catalog_client_returns_not_found_error() {
    let base_url =
        serve_response(StatusCode::NOT_FOUND, json!({"message": "listing not found"})).await;
    let client = HttpCatalogClient::new(base_url).expect("create client");

    let error = client
        .get_listing_summary("missing-listing")
        .await
        .unwrap_err();

    assert!(matches!(error, CatalogClientError::ListingNotFound(_)));
}

#[tokio::test]
async fn http_catalog_client_returns_service_error() {
    let base_url =
        serve_response(StatusCode::INTERNAL_SERVER_ERROR, json!({"message": "boom"})).await;
    let client = HttpCatalogClient::new(base_url).expect("create client");

    let error = client
        .get_listing_summary("listing-http-1")
        .await
        .unwrap_err();

    assert!(matches!(error, CatalogClientError::ServiceError(_)));
}

#[tokio::test]
async fn http_catalog_client_returns_error_on_invalid_json() {
    let app = Router::new().route(
        "/api/v1/catalogue/listings/:listing_id/summary",
        get(|| async move { (StatusCode::OK, "not-json") }),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind catalogue test server");
    let address = listener.local_addr().expect("read local address");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve catalogue test server");
    });

    let client = HttpCatalogClient::new(format!("http://{address}")).expect("create client");
    let error = client
        .get_listing_summary("listing-http-1")
        .await
        .unwrap_err();

    assert!(matches!(error, CatalogClientError::ServiceError(_)));
}

#[test]
fn grpc_catalog_client_rejects_empty_endpoint() {
    let error = GrpcCatalogClient::new("").unwrap_err();
    assert!(matches!(error, CatalogClientError::ServiceError(_)));
}

#[tokio::test]
async fn grpc_catalog_client_returns_network_error_when_unavailable() {
    let client = GrpcCatalogClient::new("http://127.0.0.1:1").expect("grpc client");
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        client.get_listing_summary("listing-1"),
    )
    .await
    .expect("timeout");

    assert!(matches!(result.unwrap_err(), CatalogClientError::NetworkError(_)));
}
