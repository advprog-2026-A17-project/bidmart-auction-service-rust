use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::client::http_service_client::{HttpServiceClient, HttpServiceClientError};

const DEFAULT_INTERNAL_SERVICE_TOKEN: &str = "bidmart-local-internal-token";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HoldFundsRequest {
    pub user_id: String,
    pub role: Option<String>,
    pub hold_id: String,
    pub auction_id: String,
    pub bid_id: String,
    pub amount: u64,
    pub expires_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HoldResponse {
    pub id: String,
    pub status: String,
    pub amount: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseFundsRequest {
    pub hold_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertFundsRequest {
    pub hold_id: String,
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
    async fn hold_funds(
        &self,
        request: HoldFundsRequest,
    ) -> Result<HoldResponse, WalletClientError>;
    async fn release_hold(&self, hold_id: &str) -> Result<(), WalletClientError>;
    async fn convert_hold_to_payment(&self, hold_id: &str) -> Result<(), WalletClientError>;
}

#[derive(Debug, Clone)]
pub struct HttpWalletClient {
    client: HttpServiceClient,
}

impl HttpWalletClient {
    pub fn new(base_url: impl AsRef<str>) -> Result<Self, WalletClientError> {
        let internal_service_token = std::env::var("GATEWAY_INTERNAL_TOKEN")
            .unwrap_or_else(|_| DEFAULT_INTERNAL_SERVICE_TOKEN.to_string());
        let client = HttpServiceClient::new(base_url, "wallet")
            .map_err(WalletClientError::from_http_error)?
            .with_internal_service_token(internal_service_token);
        Ok(Self { client })
    }
}

#[async_trait::async_trait]
impl WalletClient for HttpWalletClient {
    async fn hold_funds(
        &self,
        request: HoldFundsRequest,
    ) -> Result<HoldResponse, WalletClientError> {
        let body = serde_json::to_vec(&request)
            .map_err(|error| WalletClientError::ServiceError(error.to_string()))?;

        let response = self
            .client
            .post_json("/api/v1/wallet/hold".to_string(), body)
            .await
            .map_err(WalletClientError::from_http_error)?;

        if response.status.is_success() {
            // Sekarang kita benar-benar membaca response dari Wallet Service
            let hold_response: HoldResponse = serde_json::from_slice(&response.body)
                .map_err(|e| WalletClientError::ServiceError(e.to_string()))?;
            return Ok(hold_response);
        }

        let message = String::from_utf8_lossy(&response.body).to_string();
        // Cek error code INSUFFICIENT_ACTIVE_BALANCE yang kita buat tadi
        if response.status == hyper::StatusCode::BAD_REQUEST && message.contains("INSUFFICIENT") {
            return Err(WalletClientError::InsufficientBalance(message));
        }

        Err(WalletClientError::ServiceError(format!(
            "wallet service returned {}: {message}",
            response.status
        )))
    }

    async fn release_hold(&self, hold_id: &str) -> Result<(), WalletClientError> {
        let request = ReleaseFundsRequest {
            hold_id: hold_id.to_string(),
        };
        let body = serde_json::to_vec(&request)
            .map_err(|error| WalletClientError::ServiceError(error.to_string()))?;

        let response = self
            .client
            .post_json("/api/v1/wallet/release".to_string(), body)
            .await
            .map_err(WalletClientError::from_http_error)?;

        if response.status.is_success() {
            Ok(())
        } else {
            Err(WalletClientError::ServiceError("Failed to release".into()))
        }
    }

    async fn convert_hold_to_payment(&self, hold_id: &str) -> Result<(), WalletClientError> {
        let request = ConvertFundsRequest {
            hold_id: hold_id.to_string(),
        };
        let body = serde_json::to_vec(&request)
            .map_err(|error| WalletClientError::ServiceError(error.to_string()))?;

        let response = self
            .client
            .post_json("/api/v1/wallet/convert".to_string(), body)
            .await
            .map_err(WalletClientError::from_http_error)?;

        if response.status.is_success() {
            Ok(())
        } else {
            Err(WalletClientError::ServiceError("Failed to convert".into()))
        }
    }
}

impl WalletClientError {
    fn from_http_error(error: HttpServiceClientError) -> Self {
        match error {
            HttpServiceClientError::Configuration(message) => Self::ServiceError(message),
            HttpServiceClientError::Network(message) => Self::NetworkError(message),
            HttpServiceClientError::Request(message) => Self::ServiceError(message),
        }
    }
}
