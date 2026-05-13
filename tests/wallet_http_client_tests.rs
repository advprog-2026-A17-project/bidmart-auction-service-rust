use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{Value, json};
use tokio::net::TcpListener;

use bidmart_auction_service_rust::client::{HoldFundsRequest, HttpWalletClient, WalletClient};

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
