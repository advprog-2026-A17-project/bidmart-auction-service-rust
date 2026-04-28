use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingSummary {
    pub id: String,
    pub seller_id: String,
    pub status: String,
}

#[derive(Debug, Clone, Error)]
pub enum CatalogClientError {
    #[error("Listing not found: {0}")]
    ListingNotFound(String),
    #[error("Catalog service error: {0}")]
    ServiceError(String),
    #[error("Network error: {0}")]
    NetworkError(String),
}

#[async_trait::async_trait]
pub trait CatalogClient: Send + Sync {
    async fn get_listing_summary(
        &self,
        listing_id: &str,
    ) -> Result<ListingSummary, CatalogClientError>;
}
