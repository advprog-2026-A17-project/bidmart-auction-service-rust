use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::client::http_service_client::{HttpServiceClient, HttpServiceClientError};

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

#[derive(Debug, Clone)]
pub struct HttpCatalogClient {
    client: HttpServiceClient,
}

impl HttpCatalogClient {
    pub fn new(base_url: impl AsRef<str>) -> Result<Self, CatalogClientError> {
        let client = HttpServiceClient::new(base_url, "catalog")
            .map_err(CatalogClientError::from_http_error)?;

        Ok(Self { client })
    }
}

#[async_trait::async_trait]
impl CatalogClient for HttpCatalogClient {
    async fn get_listing_summary(
        &self,
        listing_id: &str,
    ) -> Result<ListingSummary, CatalogClientError> {
        let path = format!("/api/v1/catalogue/listings/{listing_id}/summary");
        let response = self
            .client
            .get(path)
            .await
            .map_err(CatalogClientError::from_http_error)?;

        if response.status == hyper::StatusCode::NOT_FOUND {
            return Err(CatalogClientError::ListingNotFound(listing_id.to_string()));
        }

        if !response.status.is_success() {
            return Err(CatalogClientError::ServiceError(format!(
                "catalog service returned {}",
                response.status
            )));
        }

        serde_json::from_slice(&response.body)
            .map_err(|error| CatalogClientError::ServiceError(error.to_string()))
    }
}

impl CatalogClientError {
    fn from_http_error(error: HttpServiceClientError) -> Self {
        match error {
            HttpServiceClientError::Configuration(message) => Self::ServiceError(message),
            HttpServiceClientError::Network(message) => Self::NetworkError(message),
            HttpServiceClientError::Request(message) => Self::ServiceError(message),
        }
    }
}
