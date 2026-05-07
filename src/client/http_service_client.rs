use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::client::conn::http1;
use hyper::{Method, Request, StatusCode};
use hyper_util::rt::TokioIo;
use thiserror::Error;
use tokio::net::TcpStream;

#[derive(Debug, Clone)]
pub(crate) struct HttpServiceClient {
    base_url: url::Url,
    service_name: &'static str,
    internal_service_token: Option<String>,
}

impl HttpServiceClient {
    pub(crate) fn new(
        base_url: impl AsRef<str>,
        service_name: &'static str,
    ) -> Result<Self, HttpServiceClientError> {
        let base_url = url::Url::parse(base_url.as_ref())
            .map_err(|error| HttpServiceClientError::Configuration(error.to_string()))?;

        if base_url.scheme() != "http" {
            return Err(HttpServiceClientError::Configuration(format!(
                "only http {service_name} URLs are supported"
            )));
        }

        Ok(Self {
            base_url,
            service_name,
            internal_service_token: None,
        })
    }

    pub(crate) fn with_internal_service_token(mut self, token: impl Into<String>) -> Self {
        let token = token.into();
        if !token.trim().is_empty() {
            self.internal_service_token = Some(token);
        }
        self
    }

    pub(crate) async fn get(
        &self,
        path: impl Into<String>,
    ) -> Result<HttpResponse, HttpServiceClientError> {
        self.send(Method::GET, path.into(), Vec::new(), None).await
    }

    pub(crate) async fn post_json(
        &self,
        path: impl Into<String>,
        body: Vec<u8>,
    ) -> Result<HttpResponse, HttpServiceClientError> {
        self.send(Method::POST, path.into(), body, Some("application/json"))
            .await
    }

    async fn send(
        &self,
        method: Method,
        path: String,
        body: Vec<u8>,
        content_type: Option<&str>,
    ) -> Result<HttpResponse, HttpServiceClientError> {
        let host = self.base_url.host_str().ok_or_else(|| {
            HttpServiceClientError::Configuration(format!(
                "{} URL is missing host",
                self.service_name
            ))
        })?;
        let port = self.base_url.port_or_known_default().ok_or_else(|| {
            HttpServiceClientError::Configuration(format!(
                "{} URL is missing port",
                self.service_name
            ))
        })?;
        let stream = TcpStream::connect((host, port))
            .await
            .map_err(|error| HttpServiceClientError::Network(error.to_string()))?;
        let io = TokioIo::new(stream);
        let (mut sender, connection) = http1::handshake(io)
            .await
            .map_err(|error| HttpServiceClientError::Network(error.to_string()))?;

        tokio::spawn(async move {
            let _ = connection.await;
        });

        let host_header = match self.base_url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_string(),
        };
        let content_length = body.len().to_string();
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("host", host_header)
            .header("accept", "application/json")
            .header("content-length", content_length);

        if let Some(content_type) = content_type {
            request = request.header("content-type", content_type);
        }
        if let Some(internal_service_token) = &self.internal_service_token {
            request = request.header("x-internal-service-token", internal_service_token);
        }

        let request = request
            .body(Full::new(Bytes::from(body)))
            .map_err(|error| HttpServiceClientError::Request(error.to_string()))?;
        let response = sender
            .send_request(request)
            .await
            .map_err(|error| HttpServiceClientError::Network(error.to_string()))?;
        let status = response.status();
        let body = response
            .into_body()
            .collect()
            .await
            .map_err(|error| HttpServiceClientError::Network(error.to_string()))?
            .to_bytes()
            .to_vec();

        Ok(HttpResponse { status, body })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HttpResponse {
    pub(crate) status: StatusCode,
    pub(crate) body: Vec<u8>,
}

#[derive(Debug, Clone, Error)]
pub(crate) enum HttpServiceClientError {
    #[error("{0}")]
    Configuration(String),
    #[error("{0}")]
    Network(String),
    #[error("{0}")]
    Request(String),
}
