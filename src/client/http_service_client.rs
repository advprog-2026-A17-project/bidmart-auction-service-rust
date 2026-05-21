use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::client::conn::http1::Builder as Http1Builder;
use hyper::{Method, Request, StatusCode, Uri};
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use hyper_util::rt::TokioIo;
use thiserror::Error;
use tokio::net::TcpStream;

type HttpsClient = Client<hyper_rustls::HttpsConnector<HttpConnector>, Full<Bytes>>;

#[derive(Debug, Clone)]
pub(crate) struct HttpServiceClient {
    base_url: url::Url,
    service_name: &'static str,
    internal_service_token: Option<String>,
    https_client: Option<HttpsClient>,
}

impl HttpServiceClient {
    pub(crate) fn new(
        base_url: impl AsRef<str>,
        service_name: &'static str,
    ) -> Result<Self, HttpServiceClientError> {
        let base_url = url::Url::parse(base_url.as_ref())
            .map_err(|error| HttpServiceClientError::Configuration(error.to_string()))?;

        let scheme = base_url.scheme();
        if scheme != "http" && scheme != "https" {
            return Err(HttpServiceClientError::Configuration(format!(
                "only http/https {service_name} URLs are supported"
            )));
        }

        let https_client = if scheme == "https" {
            let https = HttpsConnectorBuilder::new()
                .with_native_roots()
                .expect("TLS native root certificates should be available")
                .https_or_http()
                .enable_http1()
                .build();
            Some(Client::builder(TokioExecutor::new()).build(https))
        } else {
            None
        };

        Ok(Self {
            base_url,
            service_name,
            internal_service_token: None,
            https_client,
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
        if self.https_client.is_some() {
            return self.send_https(method, path, body, content_type).await;
        }

        self.send_http(method, path, body, content_type).await
    }

    async fn send_https(
        &self,
        method: Method,
        path: String,
        body: Vec<u8>,
        content_type: Option<&str>,
    ) -> Result<HttpResponse, HttpServiceClientError> {
        let client = self.https_client.as_ref().ok_or_else(|| {
            HttpServiceClientError::Configuration(format!(
                "{} HTTPS client is not configured",
                self.service_name
            ))
        })?;
        let request = self.build_request(method, path, body, content_type)?;
        let response = client
            .request(request)
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

    async fn send_http(
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
        let (mut sender, connection) = Http1Builder::new()
            .handshake(io)
            .await
            .map_err(|error| HttpServiceClientError::Network(error.to_string()))?;

        tokio::spawn(async move {
            let _ = connection.await;
        });

        let request = self.build_request(method, path, body, content_type)?;
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

    fn build_request(
        &self,
        method: Method,
        path: String,
        body: Vec<u8>,
        content_type: Option<&str>,
    ) -> Result<Request<Full<Bytes>>, HttpServiceClientError> {
        let uri: Uri = if self.base_url.scheme() == "https" {
            let joined = self
                .base_url
                .join(&path)
                .map_err(|error: url::ParseError| {
                    HttpServiceClientError::Configuration(error.to_string())
                })?;
            joined
                .to_string()
                .parse::<Uri>()
                .map_err(|error: hyper::http::uri::InvalidUri| {
                    HttpServiceClientError::Configuration(error.to_string())
                })?
        } else {
            path.parse::<Uri>().map_err(|error: hyper::http::uri::InvalidUri| {
                HttpServiceClientError::Configuration(error.to_string())
            })?
        };

        let content_length = body.len().to_string();
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("accept", "application/json")
            .header("content-length", content_length);

        if self.base_url.scheme() == "http" {
            let host = self.base_url.host_str().ok_or_else(|| {
                HttpServiceClientError::Configuration(format!(
                    "{} URL is missing host",
                    self.service_name
                ))
            })?;
            let host_header = match self.base_url.port() {
                Some(port) => format!("{host}:{port}"),
                None => host.to_string(),
            };
            request = request.header("host", host_header);
        }

        if let Some(content_type) = content_type {
            request = request.header("content-type", content_type);
        }
        if let Some(internal_service_token) = &self.internal_service_token {
            request = request.header("x-internal-service-token", internal_service_token);
        }

        request
            .body(Full::new(Bytes::from(body)))
            .map_err(|error| HttpServiceClientError::Request(error.to_string()))
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
