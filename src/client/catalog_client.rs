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

#[derive(Debug, Clone)]
pub struct HttpCatalogClient {
    base_url: url::Url,
}

impl HttpCatalogClient {
    pub fn new(base_url: impl AsRef<str>) -> Result<Self, CatalogClientError> {
        let base_url = url::Url::parse(base_url.as_ref())
            .map_err(|error| CatalogClientError::ServiceError(error.to_string()))?;

        if base_url.scheme() != "http" {
            return Err(CatalogClientError::ServiceError(
                "only http catalog URLs are supported".to_string(),
            ));
        }

        Ok(Self { base_url })
    }

    async fn get_json(&self, path: String) -> Result<(hyper::StatusCode, Vec<u8>), CatalogClientError> {
        use http_body_util::{BodyExt, Empty};
        use hyper::body::Bytes;
        use hyper::client::conn::http1;
        use hyper::{Method, Request};
        use hyper_util::rt::TokioIo;
        use tokio::net::TcpStream;

        let host = self
            .base_url
            .host_str()
            .ok_or_else(|| CatalogClientError::ServiceError("catalog URL is missing host".to_string()))?;
        let port = self
            .base_url
            .port_or_known_default()
            .ok_or_else(|| CatalogClientError::ServiceError("catalog URL is missing port".to_string()))?;
        let stream = TcpStream::connect((host, port))
            .await
            .map_err(|error| CatalogClientError::NetworkError(error.to_string()))?;
        let io = TokioIo::new(stream);
        let (mut sender, connection) = http1::handshake(io)
            .await
            .map_err(|error| CatalogClientError::NetworkError(error.to_string()))?;

        tokio::spawn(async move {
            let _ = connection.await;
        });

        let host_header = match self.base_url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_string(),
        };
        let request = Request::builder()
            .method(Method::GET)
            .uri(path)
            .header("host", host_header)
            .header("accept", "application/json")
            .body(Empty::<Bytes>::new())
            .map_err(|error| CatalogClientError::ServiceError(error.to_string()))?;

        let response = sender
            .send_request(request)
            .await
            .map_err(|error| CatalogClientError::NetworkError(error.to_string()))?;
        let status = response.status();
        let body = response
            .into_body()
            .collect()
            .await
            .map_err(|error| CatalogClientError::NetworkError(error.to_string()))?
            .to_bytes()
            .to_vec();

        Ok((status, body))
    }
}

#[async_trait::async_trait]
impl CatalogClient for HttpCatalogClient {
    async fn get_listing_summary(
        &self,
        listing_id: &str,
    ) -> Result<ListingSummary, CatalogClientError> {
        let path = format!("/api/v1/catalogue/listings/{listing_id}/summary");
        let (status, body) = self.get_json(path).await?;

        if status == hyper::StatusCode::NOT_FOUND {
            return Err(CatalogClientError::ListingNotFound(listing_id.to_string()));
        }

        if !status.is_success() {
            return Err(CatalogClientError::ServiceError(format!(
                "catalog service returned {status}"
            )));
        }

        serde_json::from_slice(&body)
            .map_err(|error| CatalogClientError::ServiceError(error.to_string()))
    }
}
