use serde::{Deserialize, Serialize};
use thiserror::Error;
use tonic::client::Grpc;
use tonic::codec::ProstCodec;
use tonic::codegen::http::uri::PathAndQuery;
use tonic::transport::Endpoint;

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

#[derive(Debug, Clone)]
pub struct GrpcCatalogClient {
    endpoint: Endpoint,
}

impl GrpcCatalogClient {
    pub fn new(endpoint: impl AsRef<str>) -> Result<Self, CatalogClientError> {
        let endpoint = endpoint.as_ref().trim();
        if endpoint.is_empty() {
            return Err(CatalogClientError::ServiceError(
                "catalog gRPC endpoint is empty".to_string(),
            ));
        }

        let endpoint = Endpoint::from_shared(endpoint.to_string())
            .map_err(|error| CatalogClientError::ServiceError(error.to_string()))?;

        Ok(Self { endpoint })
    }
}

#[async_trait::async_trait]
impl CatalogClient for GrpcCatalogClient {
    async fn get_listing_summary(
        &self,
        listing_id: &str,
    ) -> Result<ListingSummary, CatalogClientError> {
        let channel = self
            .endpoint
            .connect()
            .await
            .map_err(|error| CatalogClientError::NetworkError(error.to_string()))?;
        let mut grpc = Grpc::new(channel);
        grpc.ready()
            .await
            .map_err(|error| CatalogClientError::ServiceError(error.to_string()))?;
        let grpc_request = tonic::Request::new(GrpcGetListingSummaryRequest {
            listing_id: listing_id.to_string(),
        });

        let response: GrpcGetListingSummaryResponse = grpc
            .unary(
                grpc_request,
                PathAndQuery::from_static("/catalogue.v1.CatalogueService/GetListingSummary"),
                ProstCodec::default(),
            )
            .await
            .map_err(CatalogClientError::from_grpc_status)?
            .into_inner();

        Ok(ListingSummary {
            id: response.id,
            seller_id: response.seller_id,
            status: response.status,
        })
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

    fn from_grpc_status(status: tonic::Status) -> Self {
        let message = status.message().to_string();
        match status.code() {
            tonic::Code::NotFound => Self::ListingNotFound(message),
            tonic::Code::Unavailable
            | tonic::Code::DeadlineExceeded
            | tonic::Code::Cancelled
            | tonic::Code::Unknown => Self::NetworkError(message),
            code => Self::ServiceError(format!("gRPC {code:?}: {message}")),
        }
    }
}

#[derive(Clone, PartialEq, ::prost::Message)]
struct GrpcGetListingSummaryRequest {
    #[prost(string, tag = "1")]
    listing_id: String,
}

#[derive(Clone, PartialEq, ::prost::Message)]
struct GrpcGetListingSummaryResponse {
    #[prost(string, tag = "1")]
    id: String,
    #[prost(string, tag = "2")]
    seller_id: String,
    #[prost(string, tag = "3")]
    status: String,
}
