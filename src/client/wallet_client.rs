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
