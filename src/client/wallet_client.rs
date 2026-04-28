use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoldFundsRequest {
    pub user_id: String,
    pub amount_cents: i64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoldFundsResponse {
    pub hold_id: String,
    pub user_id: String,
    pub amount_cents: i64,
}

#[derive(Debug, Error)]
pub enum WalletClientError {
    #[error("Insufficient balance: {0}")]
    InsufficientBalance(String),
    #[error("Wallet service error: {0}")]
    ServiceError(String),
    #[error("Network error: {0}")]
    NetworkError(String),
}

#[async_trait::async_trait]
pub trait WalletClient: Send + Sync {
    async fn hold_funds(&self, request: HoldFundsRequest) -> Result<HoldFundsResponse, WalletClientError>;
    async fn release_hold(&self, hold_id: &str) -> Result<(), WalletClientError>;
    async fn convert_hold_to_payment(&self, hold_id: &str) -> Result<(), WalletClientError>;
}

#[derive(Debug, Clone)]
pub struct HttpWalletClient {
    base_url: url::Url,
}

impl HttpWalletClient {
    pub fn new(base_url: impl AsRef<str>) -> Result<Self, WalletClientError> {
        let base_url = url::Url::parse(base_url.as_ref())
            .map_err(|error| WalletClientError::ServiceError(error.to_string()))?;

        if base_url.scheme() != "http" {
            return Err(WalletClientError::ServiceError(
                "only http wallet URLs are supported".to_string(),
            ));
        }

        Ok(Self { base_url })
    }

    async fn post_funds(
        &self,
        path: &str,
        request: WalletFundsRequest,
    ) -> Result<(), WalletClientError> {
        let body = serde_json::to_vec(&request)
            .map_err(|error| WalletClientError::ServiceError(error.to_string()))?;
        let (status, response_body) = self.post_json(path.to_string(), body).await?;

        if status.is_success() {
            return Ok(());
        }

        let message = String::from_utf8_lossy(&response_body).to_string();
        if status == hyper::StatusCode::BAD_REQUEST || status == hyper::StatusCode::PAYMENT_REQUIRED
        {
            return Err(WalletClientError::InsufficientBalance(message));
        }

        Err(WalletClientError::ServiceError(format!(
            "wallet service returned {status}: {message}"
        )))
    }

    async fn post_json(
        &self,
        path: String,
        body: Vec<u8>,
    ) -> Result<(hyper::StatusCode, Vec<u8>), WalletClientError> {
        use http_body_util::{BodyExt, Full};
        use hyper::body::Bytes;
        use hyper::client::conn::http1;
        use hyper::{Method, Request};
        use hyper_util::rt::TokioIo;
        use tokio::net::TcpStream;

        let host = self
            .base_url
            .host_str()
            .ok_or_else(|| WalletClientError::ServiceError("wallet URL is missing host".to_string()))?;
        let port = self
            .base_url
            .port_or_known_default()
            .ok_or_else(|| WalletClientError::ServiceError("wallet URL is missing port".to_string()))?;
        let stream = TcpStream::connect((host, port))
            .await
            .map_err(|error| WalletClientError::NetworkError(error.to_string()))?;
        let io = TokioIo::new(stream);
        let (mut sender, connection) = http1::handshake(io)
            .await
            .map_err(|error| WalletClientError::NetworkError(error.to_string()))?;

        tokio::spawn(async move {
            let _ = connection.await;
        });

        let host_header = match self.base_url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_string(),
        };
        let content_length = body.len().to_string();
        let request = Request::builder()
            .method(Method::POST)
            .uri(path)
            .header("host", host_header)
            .header("accept", "application/json")
            .header("content-type", "application/json")
            .header("content-length", content_length)
            .body(Full::new(Bytes::from(body)))
            .map_err(|error| WalletClientError::ServiceError(error.to_string()))?;

        let response = sender
            .send_request(request)
            .await
            .map_err(|error| WalletClientError::NetworkError(error.to_string()))?;
        let status = response.status();
        let body = response
            .into_body()
            .collect()
            .await
            .map_err(|error| WalletClientError::NetworkError(error.to_string()))?
            .to_bytes()
            .to_vec();

        Ok((status, body))
    }
}

#[async_trait::async_trait]
impl WalletClient for HttpWalletClient {
    async fn hold_funds(
        &self,
        request: HoldFundsRequest,
    ) -> Result<HoldFundsResponse, WalletClientError> {
        let hold_id = wallet_hold_id(&request.user_id, request.amount_cents);
        let response = HoldFundsResponse {
            hold_id,
            user_id: request.user_id.clone(),
            amount_cents: request.amount_cents,
        };
        let request = WalletFundsRequest {
            user_id: request.user_id,
            amount: cents_to_decimal(request.amount_cents),
            description: request.reason,
        };

        self.post_funds("/api/v1/wallet/hold", request).await?;

        Ok(response)
    }

    async fn release_hold(&self, hold_id: &str) -> Result<(), WalletClientError> {
        let (user_id, amount_cents) = parse_wallet_hold_id(hold_id)?;
        self.post_funds(
            "/api/v1/wallet/release",
            WalletFundsRequest {
                user_id,
                amount: cents_to_decimal(amount_cents),
                description: "Release auction bid hold".to_string(),
            },
        )
        .await
    }

    async fn convert_hold_to_payment(&self, hold_id: &str) -> Result<(), WalletClientError> {
        let (user_id, amount_cents) = parse_wallet_hold_id(hold_id)?;
        self.post_funds(
            "/api/v1/wallet/convert",
            WalletFundsRequest {
                user_id,
                amount: cents_to_decimal(amount_cents),
                description: "Convert winning auction bid hold".to_string(),
            },
        )
        .await
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WalletFundsRequest {
    user_id: String,
    amount: f64,
    description: String,
}

fn wallet_hold_id(user_id: &str, amount_cents: i64) -> String {
    format!("{user_id}:{amount_cents}")
}

fn parse_wallet_hold_id(hold_id: &str) -> Result<(String, i64), WalletClientError> {
    let (user_id, amount_cents) = hold_id
        .rsplit_once(':')
        .ok_or_else(|| WalletClientError::ServiceError("invalid wallet hold id".to_string()))?;
    let amount_cents = amount_cents
        .parse::<i64>()
        .map_err(|error| WalletClientError::ServiceError(error.to_string()))?;

    Ok((user_id.to_string(), amount_cents))
}

fn cents_to_decimal(cents: i64) -> f64 {
    cents as f64 / 100.0
}
