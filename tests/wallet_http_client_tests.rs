use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{Value, json};
use tokio::net::TcpListener;

use bidmart_auction_service_rust::client::{
    GrpcWalletClient, HoldFundsRequest, HttpWalletClient, WalletClient, WalletClientError,
};

async fn serve_wallet_response(status: StatusCode, body: serde_json::Value) -> String {
    let app = Router::new().route(
        "/api/v1/wallet/hold",
        post(move |Json(_payload): Json<Value>| async move { (status, Json(body)) }),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind wallet test server");
    let address = listener.local_addr().expect("read local address");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve wallet test server");
    });

    format!("http://{address}")
}

async fn serve_wallet_release_response(status: StatusCode) -> String {
    let app = Router::new()
        .route(
            "/api/v1/wallet/release",
            post({
                let release_status = status;
                move || async move { release_status }
            }),
        )
        .route(
            "/api/v1/wallet/convert",
            post({
                let convert_status = status;
                move || async move { convert_status }
            }),
        );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind wallet test server");
    let address = listener.local_addr().expect("read local address");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve wallet test server");
    });

    format!("http://{address}")
}

#[derive(Clone)]
struct WalletState {
    hold_requests: Arc<Mutex<Vec<Value>>>,
    internal_tokens: Arc<Mutex<Vec<Option<String>>>>,
}

#[tokio::test]
async fn http_wallet_client_posts_hold_request_to_wallet_api() {
    let state = WalletState {
        hold_requests: Arc::new(Mutex::new(Vec::new())),
        internal_tokens: Arc::new(Mutex::new(Vec::new())),
    };
    let captured_requests = state.hold_requests.clone();
    let captured_tokens = state.internal_tokens.clone();

    let app = Router::new()
        .route(
            "/api/v1/wallet/hold",
            post(
                |State(state): State<WalletState>,
                 headers: HeaderMap,
                 Json(payload): Json<Value>| async move {
                    let internal_token = headers
                        .get("x-internal-service-token")
                        .and_then(|value| value.to_str().ok())
                        .map(ToOwned::to_owned);
                    state
                        .internal_tokens
                        .lock()
                        .expect("lock internal tokens")
                        .push(internal_token);
                    state
                        .hold_requests
                        .lock()
                        .expect("lock hold requests")
                        .push(payload);

                    (
                        StatusCode::OK,
                        Json(json!({
                            "id": "mock-hold-123",
                            "status": "ACTIVE",
                            "amount": 1550
                        })),
                    )
                },
            ),
        )
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind wallet test server");
    let address = listener.local_addr().expect("read local address");
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve wallet test server");
    });

    let client = HttpWalletClient::new(format!("http://{address}")).expect("create wallet client");

    let response = client
        .hold_funds(HoldFundsRequest {
            user_id: "bidder-http-1".to_string(),
            role: Some("BUYER".to_string()),
            hold_id: "mock-hold-123".to_string(),
            auction_id: "auction-http-1".to_string(),
            bid_id: "bid-http-1".to_string(),
            amount: 1550,
            expires_at: "2026-12-31T23:59:59Z".to_string(),
        })
        .await
        .expect("hold funds");

    assert_eq!(response.id, "mock-hold-123");
    assert_eq!(response.status, "ACTIVE");
    assert_eq!(response.amount, 1550);

    let requests = captured_requests.lock().expect("lock captured requests");
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["userId"], json!("bidder-http-1"));
    assert_eq!(requests[0]["role"], json!("BUYER"));
    assert_eq!(requests[0]["holdId"], json!("mock-hold-123"));
    assert_eq!(requests[0]["auctionId"], json!("auction-http-1"));
    assert_eq!(requests[0]["bidId"], json!("bid-http-1"));
    assert_eq!(requests[0]["amount"], json!(1550));
    assert_eq!(requests[0]["expiresAt"], json!("2026-12-31T23:59:59Z"));

    let tokens = captured_tokens.lock().expect("lock captured tokens");
    assert_eq!(
        tokens.as_slice(),
        &[Some("bidmart-local-internal-token".to_string())]
    );
}

#[tokio::test]
async fn http_wallet_client_reports_insufficient_balance() {
    let base_url =
        serve_wallet_response(StatusCode::BAD_REQUEST, json!("INSUFFICIENT_BALANCE")).await;
    let client = HttpWalletClient::new(base_url).expect("create wallet client");

    let error = client
        .hold_funds(HoldFundsRequest {
            user_id: "bidder-http-1".to_string(),
            role: Some("BUYER".to_string()),
            hold_id: "mock-hold-123".to_string(),
            auction_id: "auction-http-1".to_string(),
            bid_id: "bid-http-1".to_string(),
            amount: 1550,
            expires_at: "2026-12-31T23:59:59Z".to_string(),
        })
        .await
        .unwrap_err();

    assert!(matches!(error, WalletClientError::InsufficientBalance(_)));
}

#[tokio::test]
async fn http_wallet_client_returns_error_on_invalid_json() {
    let base_url = serve_wallet_response(StatusCode::OK, json!("not-json")).await;
    let client = HttpWalletClient::new(base_url).expect("create wallet client");

    let error = client
        .hold_funds(HoldFundsRequest {
            user_id: "bidder-http-1".to_string(),
            role: Some("BUYER".to_string()),
            hold_id: "mock-hold-123".to_string(),
            auction_id: "auction-http-1".to_string(),
            bid_id: "bid-http-1".to_string(),
            amount: 1550,
            expires_at: "2026-12-31T23:59:59Z".to_string(),
        })
        .await
        .unwrap_err();

    assert!(matches!(error, WalletClientError::ServiceError(_)));
}

#[tokio::test]
async fn http_wallet_client_release_and_convert_failures() {
    let base_url = serve_wallet_release_response(StatusCode::INTERNAL_SERVER_ERROR).await;
    let client = HttpWalletClient::new(base_url).expect("create wallet client");

    let release_error = client.release_hold("hold-1").await.unwrap_err();
    assert!(matches!(release_error, WalletClientError::ServiceError(_)));

    let convert_error = client.convert_hold_to_payment("hold-1").await.unwrap_err();
    assert!(matches!(convert_error, WalletClientError::ServiceError(_)));
}

#[tokio::test]
async fn http_wallet_client_release_and_convert_success() {
    let base_url = serve_wallet_release_response(StatusCode::OK).await;
    let client = HttpWalletClient::new(base_url).expect("create wallet client");

    client.release_hold("hold-1").await.expect("release hold");
    client
        .convert_hold_to_payment("hold-1")
        .await
        .expect("convert hold");
}

#[test]
fn grpc_wallet_client_rejects_empty_endpoint() {
    let error = GrpcWalletClient::new("").unwrap_err();
    assert!(matches!(error, WalletClientError::ServiceError(_)));
}

#[tokio::test]
async fn grpc_wallet_client_returns_network_error_when_unavailable() {
    let client = GrpcWalletClient::new("http://127.0.0.1:1").expect("grpc client");
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        client.hold_funds(HoldFundsRequest {
            user_id: "bidder-1".to_string(),
            role: Some("BUYER".to_string()),
            hold_id: "hold-1".to_string(),
            auction_id: "auction-1".to_string(),
            bid_id: "bid-1".to_string(),
            amount: 1200,
            expires_at: "2026-12-31T23:59:59Z".to_string(),
        }),
    )
    .await
    .expect("timeout");

    assert!(matches!(
        result.unwrap_err(),
        WalletClientError::NetworkError(_)
    ));
}
