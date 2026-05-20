use serde::{Deserialize, Serialize};
use thiserror::Error;
use tonic::client::Grpc;
use tonic::codec::ProstCodec;
use tonic::codegen::http::uri::PathAndQuery;
use tonic::metadata::{Ascii, MetadataValue};
use tonic::transport::Endpoint;

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
            let hold_response: HoldResponse = serde_json::from_slice(&response.body)
                .map_err(|e| WalletClientError::ServiceError(e.to_string()))?;
            return Ok(hold_response);
        }

        let message = String::from_utf8_lossy(&response.body).to_string();
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

#[derive(Debug, Clone)]
pub struct GrpcWalletClient {
    endpoint: Endpoint,
    internal_service_token: Option<MetadataValue<Ascii>>,
}

impl GrpcWalletClient {
    pub fn new(endpoint: impl AsRef<str>) -> Result<Self, WalletClientError> {
        let endpoint = endpoint.as_ref().trim();
        if endpoint.is_empty() {
            return Err(WalletClientError::ServiceError(
                "wallet gRPC endpoint is empty".to_string(),
            ));
        }

        let endpoint = Endpoint::from_shared(endpoint.to_string())
            .map_err(|error| WalletClientError::ServiceError(error.to_string()))?;
        let internal_service_token = std::env::var("GATEWAY_INTERNAL_TOKEN")
            .unwrap_or_else(|_| DEFAULT_INTERNAL_SERVICE_TOKEN.to_string());
        let internal_service_token = MetadataValue::try_from(internal_service_token.as_str())
            .map_err(|error| WalletClientError::ServiceError(error.to_string()))?;

        Ok(Self {
            endpoint,
            internal_service_token: Some(internal_service_token),
        })
    }

    fn add_internal_token<T>(&self, request: &mut tonic::Request<T>) {
        if let Some(internal_service_token) = &self.internal_service_token {
            request
                .metadata_mut()
                .insert("x-internal-service-token", internal_service_token.clone());
        }
    }
}

#[async_trait::async_trait]
impl WalletClient for GrpcWalletClient {
    async fn hold_funds(
        &self,
        request: HoldFundsRequest,
    ) -> Result<HoldResponse, WalletClientError> {
        let channel = self
            .endpoint
            .connect()
            .await
            .map_err(|error| WalletClientError::NetworkError(error.to_string()))?;
        let mut grpc = Grpc::new(channel);
        grpc.ready()
            .await
            .map_err(|error| WalletClientError::ServiceError(error.to_string()))?;
        let mut grpc_request = tonic::Request::new(GrpcHoldFundsRequest {
            user_id: request.user_id,
            role: request.role,
            hold_id: request.hold_id,
            auction_id: request.auction_id,
            bid_id: request.bid_id,
            amount: request.amount,
            expires_at: request.expires_at,
        });
        self.add_internal_token(&mut grpc_request);

        let response: GrpcHoldFundsResponse = grpc
            .unary(
                grpc_request,
                PathAndQuery::from_static("/wallet.v1.WalletService/HoldFunds"),
                ProstCodec::default(),
            )
            .await
            .map_err(WalletClientError::from_grpc_status)?
            .into_inner();

        Ok(HoldResponse {
            id: response.id,
            status: response.status,
            amount: response.amount,
        })
    }

    async fn release_hold(&self, hold_id: &str) -> Result<(), WalletClientError> {
        let channel = self
            .endpoint
            .connect()
            .await
            .map_err(|error| WalletClientError::NetworkError(error.to_string()))?;
        let mut grpc = Grpc::new(channel);
        grpc.ready()
            .await
            .map_err(|error| WalletClientError::ServiceError(error.to_string()))?;
        let mut grpc_request = tonic::Request::new(GrpcReleaseFundsRequest {
            hold_id: hold_id.to_string(),
        });
        self.add_internal_token(&mut grpc_request);

        let _: tonic::Response<GrpcEmptyResponse> = grpc
            .unary(
                grpc_request,
                PathAndQuery::from_static("/wallet.v1.WalletService/ReleaseHold"),
                ProstCodec::default(),
            )
            .await
            .map_err(WalletClientError::from_grpc_status)?;

        Ok(())
    }

    async fn convert_hold_to_payment(&self, hold_id: &str) -> Result<(), WalletClientError> {
        let channel = self
            .endpoint
            .connect()
            .await
            .map_err(|error| WalletClientError::NetworkError(error.to_string()))?;
        let mut grpc = Grpc::new(channel);
        grpc.ready()
            .await
            .map_err(|error| WalletClientError::ServiceError(error.to_string()))?;
        let mut grpc_request = tonic::Request::new(GrpcConvertFundsRequest {
            hold_id: hold_id.to_string(),
        });
        self.add_internal_token(&mut grpc_request);

        let _: tonic::Response<GrpcEmptyResponse> = grpc
            .unary(
                grpc_request,
                PathAndQuery::from_static("/wallet.v1.WalletService/ConvertHoldToPayment"),
                ProstCodec::default(),
            )
            .await
            .map_err(WalletClientError::from_grpc_status)?;

        Ok(())
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

    fn from_grpc_status(status: tonic::Status) -> Self {
        let message = status.message().to_string();
        match status.code() {
            tonic::Code::InvalidArgument if message.contains("INSUFFICIENT") => {
                Self::InsufficientBalance(message)
            }
            tonic::Code::Unavailable
            | tonic::Code::DeadlineExceeded
            | tonic::Code::Cancelled
            | tonic::Code::Unknown => Self::NetworkError(message),
            code => Self::ServiceError(format!("gRPC {code:?}: {message}")),
        }
    }
}


#[derive(Clone, PartialEq, ::prost::Message)]
struct GrpcHoldFundsRequest {
    #[prost(string, tag = "1")]
    user_id: String,
    #[prost(string, optional, tag = "2")]
    role: Option<String>,
    #[prost(string, tag = "3")]
    hold_id: String,
    #[prost(string, tag = "4")]
    auction_id: String,
    #[prost(string, tag = "5")]
    bid_id: String,
    #[prost(uint64, tag = "6")]
    amount: u64,
    #[prost(string, tag = "7")]
    expires_at: String,
}

#[derive(Clone, PartialEq, ::prost::Message)]
struct GrpcHoldFundsResponse {
    #[prost(string, tag = "1")]
    id: String,
    #[prost(string, tag = "2")]
    status: String,
    #[prost(uint64, tag = "3")]
    amount: u64,
}

#[derive(Clone, PartialEq, ::prost::Message)]
struct GrpcReleaseFundsRequest {
    #[prost(string, tag = "1")]
    hold_id: String,
}

#[derive(Clone, PartialEq, ::prost::Message)]
struct GrpcConvertFundsRequest {
    #[prost(string, tag = "1")]
    hold_id: String,
}

#[derive(Clone, PartialEq, ::prost::Message)]
struct GrpcEmptyResponse {}
