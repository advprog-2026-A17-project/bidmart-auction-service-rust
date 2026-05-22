use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::from_fn;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};

use crate::http::metrics_auth::require_metrics_basic_auth;

use crate::http::dto::{
    AuctionPageResponse, AuctionResponse, BidCursorPageResponse, BidResponse, CreateAuctionRequest,
    ErrorResponse, PlaceBidRequest, PlaceProxyBidRequest, ProxyBidResponse,
};
use crate::service::auction_service::{
    AuctionService, CloseListingAuctionSessionError, CreateAuctionError,
    GetListingAuctionSessionError, ListBidsError, ListListingAuctionSessionsError,
    ListPendingClosureError, PlaceBidError,
};

/// Lightweight, lock-free request metrics counters.
/// These are global statics because the Prometheus endpoint must be able to
/// read them independently of any request-scoped state.
pub struct RequestMetrics {
    pub total_requests: AtomicU64,
    pub total_errors: AtomicU64,
    /// Requests completed within the satisfied threshold (<= 500ms).
    pub apdex_satisfied: AtomicU64,
    /// Requests completed within the tolerating threshold (<= 2000ms).
    pub apdex_tolerating: AtomicU64,
    /// Requests slower than the tolerating threshold (> 2000ms).
    pub apdex_frustrated: AtomicU64,
    // Histogram buckets for latency distribution (cumulative).
    pub latency_le_5ms: AtomicU64,
    pub latency_le_25ms: AtomicU64,
    pub latency_le_50ms: AtomicU64,
    pub latency_le_100ms: AtomicU64,
    pub latency_le_250ms: AtomicU64,
    pub latency_le_500ms: AtomicU64,
    pub latency_le_1000ms: AtomicU64,
    pub latency_le_2500ms: AtomicU64,
    pub latency_le_inf: AtomicU64,
    pub latency_sum_us: AtomicU64,
    // Per-endpoint counters
    pub bids_placed: AtomicU64,
    pub auctions_created: AtomicU64,
    pub auctions_closed: AtomicU64,
}

impl RequestMetrics {
    const fn new() -> Self {
        Self {
            total_requests: AtomicU64::new(0),
            total_errors: AtomicU64::new(0),
            apdex_satisfied: AtomicU64::new(0),
            apdex_tolerating: AtomicU64::new(0),
            apdex_frustrated: AtomicU64::new(0),
            latency_le_5ms: AtomicU64::new(0),
            latency_le_25ms: AtomicU64::new(0),
            latency_le_50ms: AtomicU64::new(0),
            latency_le_100ms: AtomicU64::new(0),
            latency_le_250ms: AtomicU64::new(0),
            latency_le_500ms: AtomicU64::new(0),
            latency_le_1000ms: AtomicU64::new(0),
            latency_le_2500ms: AtomicU64::new(0),
            latency_le_inf: AtomicU64::new(0),
            latency_sum_us: AtomicU64::new(0),
            bids_placed: AtomicU64::new(0),
            auctions_created: AtomicU64::new(0),
            auctions_closed: AtomicU64::new(0),
        }
    }

    /// Record a request with the given latency.
    pub fn record_request(&self, duration_us: u64, is_error: bool) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        if is_error {
            self.total_errors.fetch_add(1, Ordering::Relaxed);
        }

        let ms = duration_us / 1000;
        // APDEX thresholds: satisfied <= 500ms, tolerating <= 2000ms
        if ms <= 500 {
            self.apdex_satisfied.fetch_add(1, Ordering::Relaxed);
        } else if ms <= 2000 {
            self.apdex_tolerating.fetch_add(1, Ordering::Relaxed);
        } else {
            self.apdex_frustrated.fetch_add(1, Ordering::Relaxed);
        }

        // Cumulative histogram buckets
        if ms <= 5 {
            self.latency_le_5ms.fetch_add(1, Ordering::Relaxed);
        }
        if ms <= 25 {
            self.latency_le_25ms.fetch_add(1, Ordering::Relaxed);
        }
        if ms <= 50 {
            self.latency_le_50ms.fetch_add(1, Ordering::Relaxed);
        }
        if ms <= 100 {
            self.latency_le_100ms.fetch_add(1, Ordering::Relaxed);
        }
        if ms <= 250 {
            self.latency_le_250ms.fetch_add(1, Ordering::Relaxed);
        }
        if ms <= 500 {
            self.latency_le_500ms.fetch_add(1, Ordering::Relaxed);
        }
        if ms <= 1000 {
            self.latency_le_1000ms.fetch_add(1, Ordering::Relaxed);
        }
        if ms <= 2500 {
            self.latency_le_2500ms.fetch_add(1, Ordering::Relaxed);
        }
        self.latency_le_inf.fetch_add(1, Ordering::Relaxed);
        self.latency_sum_us
            .fetch_add(duration_us, Ordering::Relaxed);
    }
}

pub static METRICS: RequestMetrics = RequestMetrics::new();

#[derive(Debug, Clone)]
pub struct AppState {
    auction_service: Arc<AuctionService>,
}

pub fn create_router(auction_service: AuctionService) -> Router {
    let state = AppState {
        auction_service: Arc::new(auction_service),
    };
    Router::new()
        .route("/listings", get(list_auctions).post(create_auction))
        .route("/listings/:listing_id", get(get_auction_by_id))
        .route(
            "/metrics",
            get(metrics).layer(from_fn(require_metrics_basic_auth)),
        )
        .route("/listings/:listing_id/bids", get(list_bids).post(place_bid))
        .route(
            "/listings/:listing_id/bids/cursor",
            get(list_bids_cursor).post(place_proxy_bid),
        )
        .route(
            "/listings/:listing_id/proxy-bids/:bidder_id",
            get(get_proxy_bid).delete(delete_proxy_bid),
        )
        .route("/api/v1/listings", get(list_auctions).post(create_auction))
        .route(
            "/api/v1/listings/pending-closure",
            get(list_pending_closure),
        )
        .route("/api/v1/listings/:listing_id", get(get_auction_by_id))
        .route(
            "/api/v1/listings/:listing_id/close",
            axum::routing::post(close_auction),
        )
        .route(
            "/api/v1/listings/:listing_id/bids",
            get(list_bids).post(place_bid),
        )
        .route(
            "/api/v1/listings/:listing_id/bids/cursor",
            get(list_bids_cursor).post(place_proxy_bid),
        )
        .route(
            "/api/v1/listings/:listing_id/proxy-bids/:bidder_id",
            get(get_proxy_bid).delete(delete_proxy_bid),
        )
        .layer(axum::middleware::map_response(security_headers))
        .with_state(state)
}

/// Adds secure-by-default headers to every response.
/// These prevent common web vulnerabilities (MIME-sniffing, clickjacking, XSS).
async fn security_headers(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert("x-content-type-options", "nosniff".parse().unwrap());
    headers.insert("x-frame-options", "DENY".parse().unwrap());
    headers.insert("x-xss-protection", "1; mode=block".parse().unwrap());
    headers.insert(
        "referrer-policy",
        "strict-origin-when-cross-origin".parse().unwrap(),
    );
    headers.insert(
        "cache-control",
        "no-store, no-cache, must-revalidate".parse().unwrap(),
    );
    response
}

async fn metrics() -> impl IntoResponse {
    static STARTED_AT: OnceLock<Instant> = OnceLock::new();
    let uptime_seconds = STARTED_AT.get_or_init(Instant::now).elapsed().as_secs_f64();

    let total = METRICS.total_requests.load(Ordering::Relaxed);
    let errors = METRICS.total_errors.load(Ordering::Relaxed);
    let satisfied = METRICS.apdex_satisfied.load(Ordering::Relaxed);
    let tolerating = METRICS.apdex_tolerating.load(Ordering::Relaxed);
    let bids = METRICS.bids_placed.load(Ordering::Relaxed);
    let created = METRICS.auctions_created.load(Ordering::Relaxed);
    let closed = METRICS.auctions_closed.load(Ordering::Relaxed);
    let sum_us = METRICS.latency_sum_us.load(Ordering::Relaxed);
    let sum_s = sum_us as f64 / 1_000_000.0;

    // APDEX = (satisfied + tolerating / 2) / total
    let apdex = if total > 0 {
        (satisfied as f64 + tolerating as f64 / 2.0) / total as f64
    } else {
        1.0
    };

    let le_5 = METRICS.latency_le_5ms.load(Ordering::Relaxed);
    let le_25 = METRICS.latency_le_25ms.load(Ordering::Relaxed);
    let le_50 = METRICS.latency_le_50ms.load(Ordering::Relaxed);
    let le_100 = METRICS.latency_le_100ms.load(Ordering::Relaxed);
    let le_250 = METRICS.latency_le_250ms.load(Ordering::Relaxed);
    let le_500 = METRICS.latency_le_500ms.load(Ordering::Relaxed);
    let le_1000 = METRICS.latency_le_1000ms.load(Ordering::Relaxed);
    let le_2500 = METRICS.latency_le_2500ms.load(Ordering::Relaxed);
    let le_inf = METRICS.latency_le_inf.load(Ordering::Relaxed);

    (
        [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
        format!(
            "# HELP bidmart_service_up Service availability gauge\n\
             # TYPE bidmart_service_up gauge\n\
             bidmart_service_up{{service=\"auction\"}} 1\n\
             # HELP bidmart_service_uptime_seconds Service uptime in seconds\n\
             # TYPE bidmart_service_uptime_seconds gauge\n\
             bidmart_service_uptime_seconds{{service=\"auction\"}} {uptime_seconds}\n\
             # HELP bidmart_http_requests_total Total HTTP requests\n\
             # TYPE bidmart_http_requests_total counter\n\
             bidmart_http_requests_total{{service=\"auction\"}} {total}\n\
             # HELP bidmart_http_errors_total Total HTTP error responses\n\
             # TYPE bidmart_http_errors_total counter\n\
             bidmart_http_errors_total{{service=\"auction\"}} {errors}\n\
             # HELP bidmart_apdex_score APDEX score (threshold 500ms satisfied, 2000ms tolerating)\n\
             # TYPE bidmart_apdex_score gauge\n\
             bidmart_apdex_score{{service=\"auction\"}} {apdex:.4}\n\
             # HELP bidmart_apdex_satisfied_total Requests completed within satisfied threshold\n\
             # TYPE bidmart_apdex_satisfied_total counter\n\
             bidmart_apdex_satisfied_total{{service=\"auction\"}} {satisfied}\n\
             # HELP bidmart_apdex_tolerating_total Requests completed within tolerating threshold\n\
             # TYPE bidmart_apdex_tolerating_total counter\n\
             bidmart_apdex_tolerating_total{{service=\"auction\"}} {tolerating}\n\
             # HELP bidmart_http_request_duration_seconds HTTP request latency histogram\n\
             # TYPE bidmart_http_request_duration_seconds histogram\n\
             bidmart_http_request_duration_seconds_bucket{{service=\"auction\",le=\"0.005\"}} {le_5}\n\
             bidmart_http_request_duration_seconds_bucket{{service=\"auction\",le=\"0.025\"}} {le_25}\n\
             bidmart_http_request_duration_seconds_bucket{{service=\"auction\",le=\"0.05\"}} {le_50}\n\
             bidmart_http_request_duration_seconds_bucket{{service=\"auction\",le=\"0.1\"}} {le_100}\n\
             bidmart_http_request_duration_seconds_bucket{{service=\"auction\",le=\"0.25\"}} {le_250}\n\
             bidmart_http_request_duration_seconds_bucket{{service=\"auction\",le=\"0.5\"}} {le_500}\n\
             bidmart_http_request_duration_seconds_bucket{{service=\"auction\",le=\"1.0\"}} {le_1000}\n\
             bidmart_http_request_duration_seconds_bucket{{service=\"auction\",le=\"2.5\"}} {le_2500}\n\
             bidmart_http_request_duration_seconds_bucket{{service=\"auction\",le=\"+Inf\"}} {le_inf}\n\
             bidmart_http_request_duration_seconds_sum{{service=\"auction\"}} {sum_s}\n\
             bidmart_http_request_duration_seconds_count{{service=\"auction\"}} {total}\n\
             # HELP bidmart_bids_placed_total Total bids placed\n\
             # TYPE bidmart_bids_placed_total counter\n\
             bidmart_bids_placed_total{{service=\"auction\"}} {bids}\n\
             # HELP bidmart_auctions_created_total Total auctions created\n\
             # TYPE bidmart_auctions_created_total counter\n\
             bidmart_auctions_created_total{{service=\"auction\"}} {created}\n\
             # HELP bidmart_auctions_closed_total Total auctions closed/settled\n\
             # TYPE bidmart_auctions_closed_total counter\n\
             bidmart_auctions_closed_total{{service=\"auction\"}} {closed}\n"
        ),
    )
}

async fn create_auction(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(mut request): Json<CreateAuctionRequest>,
) -> Result<(StatusCode, Json<AuctionResponse>), ApiError> {
    let start = Instant::now();
    apply_trusted_user_id(&headers, &mut request.seller_id, "seller_id")?;
    let command = request.try_into_command().map_err(ApiError::bad_request)?;
    let result = state.auction_service.create_auction(command).await;
    METRICS.record_request(start.elapsed().as_micros() as u64, result.is_err());
    let auction = result?;
    METRICS.auctions_created.fetch_add(1, Ordering::Relaxed);
    Ok((StatusCode::CREATED, Json(auction.into())))
}

async fn list_auctions(
    State(state): State<AppState>,
) -> Result<Json<AuctionPageResponse>, ApiError> {
    let auctions = state.auction_service.list_auctions().await?;
    let items: Vec<AuctionResponse> = auctions.into_iter().map(AuctionResponse::from).collect();
    let size = items.len() as i64;

    Ok(Json(AuctionPageResponse {
        items,
        page: 0,
        size,
        total_items: size,
        total_pages: if size == 0 { 0 } else { 1 },
    }))
}

async fn get_auction_by_id(
    State(state): State<AppState>,
    Path(listing_id): Path<String>,
) -> Result<Json<AuctionResponse>, ApiError> {
    let auction = state
        .auction_service
        .get_auction_by_id(&listing_id)
        .await?
        .ok_or_else(|| ApiError::not_found("listing not found"))?;

    Ok(Json(auction.into()))
}

async fn list_pending_closure(
    State(state): State<AppState>,
) -> Result<Json<Vec<AuctionResponse>>, ApiError> {
    let auctions = state.auction_service.list_pending_closure().await?;
    let response = auctions.into_iter().map(AuctionResponse::from).collect();
    Ok(Json(response))
}

async fn close_auction(
    State(state): State<AppState>,
    Path(listing_id): Path<String>,
) -> Result<Json<AuctionResponse>, ApiError> {
    let start = Instant::now();
    let result = state.auction_service.close_auction(&listing_id).await;
    METRICS.record_request(start.elapsed().as_micros() as u64, result.is_err());
    let auction = result?;
    METRICS.auctions_closed.fetch_add(1, Ordering::Relaxed);
    Ok(Json(auction.into()))
}

async fn place_bid(
    State(state): State<AppState>,
    Path(listing_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<PlaceBidRequest>,
) -> Result<(StatusCode, Json<BidResponse>), ApiError> {
    let start = Instant::now();
    let bidder_id = resolve_trusted_user_id(&headers, request.bidder_id(), "bidder_id")?;
    let bid_amount_cents = request
        .bid_amount_cents()
        .ok_or_else(|| ApiError::bad_request("bid_amount is required"))?;

    let result = state
        .auction_service
        .place_bid_and_persist(
            &listing_id,
            &bidder_id,
            bid_amount_cents,
            chrono::Utc::now().timestamp(),
        )
        .await;
    METRICS.record_request(start.elapsed().as_micros() as u64, result.is_err());
    let bid = result?;
    METRICS.bids_placed.fetch_add(1, Ordering::Relaxed);
    Ok((StatusCode::CREATED, Json(bid.into())))
}

async fn list_bids(
    State(state): State<AppState>,
    Path(listing_id): Path<String>,
) -> Result<Json<Vec<BidResponse>>, ApiError> {
    let bids = state.auction_service.list_bids(&listing_id).await?;
    let response = bids.into_iter().map(BidResponse::from).collect();

    Ok(Json(response))
}

#[derive(Debug, Clone, serde::Deserialize)]
struct BidCursorQuery {
    cursor: Option<String>,
    limit: Option<i64>,
}

async fn list_bids_cursor(
    State(state): State<AppState>,
    Path(listing_id): Path<String>,
    Query(query): Query<BidCursorQuery>,
) -> Result<Json<BidCursorPageResponse>, ApiError> {
    let page = state
        .auction_service
        .list_bids_with_cursor(&listing_id, query.cursor.as_deref(), query.limit)
        .await?;
    let items = page.items.into_iter().map(BidResponse::from).collect();

    Ok(Json(BidCursorPageResponse {
        items,
        next_cursor: page.next_cursor,
        size: page.size,
    }))
}

async fn place_proxy_bid(
    State(state): State<AppState>,
    Path(listing_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<PlaceProxyBidRequest>,
) -> Result<(StatusCode, Json<BidResponse>), ApiError> {
    let start = Instant::now();
    let bidder_id = resolve_trusted_user_id(&headers, request.bidder_id(), "bidder_id")?;
    let max_bid_amount_cents = request
        .max_bid_amount_cents()
        .ok_or_else(|| ApiError::bad_request("max_bid_amount is required"))?;

    let result = state
        .auction_service
        .place_proxy_bid_and_persist(
            &listing_id,
            &bidder_id,
            max_bid_amount_cents,
            chrono::Utc::now().timestamp(),
        )
        .await;
    METRICS.record_request(start.elapsed().as_micros() as u64, result.is_err());
    let bid = result?;
    METRICS.bids_placed.fetch_add(1, Ordering::Relaxed);

    Ok((StatusCode::CREATED, Json(bid.into())))
}

async fn get_proxy_bid(
    State(state): State<AppState>,
    Path((listing_id, bidder_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<ProxyBidResponse>, ApiError> {
    let trusted_bidder_id = resolve_trusted_user_id(&headers, Some(&bidder_id), "bidder_id")?;
    let proxy_bid = state
        .auction_service
        .get_proxy_bid(&listing_id, &trusted_bidder_id)
        .await?
        .ok_or_else(|| ApiError::not_found("proxy bid not found"))?;

    Ok(Json(proxy_bid.into()))
}

async fn delete_proxy_bid(
    State(state): State<AppState>,
    Path((listing_id, bidder_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let trusted_bidder_id = resolve_trusted_user_id(&headers, Some(&bidder_id), "bidder_id")?;
    state
        .auction_service
        .delete_proxy_bid(&listing_id, &trusted_bidder_id)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
}

impl ApiError {
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
        }
    }
}

fn apply_trusted_user_id(
    headers: &HeaderMap,
    request_user_id: &mut Option<String>,
    field_name: &str,
) -> Result<(), ApiError> {
    let Some(trusted_user_id) = trusted_user_id(headers) else {
        return Ok(());
    };

    if let Some(body_user_id) = request_user_id.as_deref().filter(|value| !value.is_empty())
        && body_user_id != trusted_user_id
    {
        return Err(ApiError::forbidden(format!(
            "{field_name} does not match authenticated user"
        )));
    }

    *request_user_id = Some(trusted_user_id);
    Ok(())
}

fn resolve_trusted_user_id(
    headers: &HeaderMap,
    request_user_id: Option<&str>,
    field_name: &str,
) -> Result<String, ApiError> {
    let mut resolved = request_user_id.map(str::to_string);
    apply_trusted_user_id(headers, &mut resolved, field_name)?;
    resolved.ok_or_else(|| ApiError::bad_request(format!("{field_name} is required")))
}

fn trusted_user_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-user-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

impl From<CreateAuctionError> for ApiError {
    fn from(error: CreateAuctionError) -> Self {
        match error {
            CreateAuctionError::InvalidInput(message) => Self {
                status: StatusCode::BAD_REQUEST,
                message,
            },
            CreateAuctionError::DatabaseError(message) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message,
            },
        }
    }
}

impl From<GetListingAuctionSessionError> for ApiError {
    fn from(error: GetListingAuctionSessionError) -> Self {
        match error {
            GetListingAuctionSessionError::DatabaseError(message) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message,
            },
        }
    }
}

impl From<ListListingAuctionSessionsError> for ApiError {
    fn from(error: ListListingAuctionSessionsError) -> Self {
        match error {
            ListListingAuctionSessionsError::DatabaseError(message) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message,
            },
        }
    }
}

impl From<ListPendingClosureError> for ApiError {
    fn from(error: ListPendingClosureError) -> Self {
        match error {
            ListPendingClosureError::DatabaseError(message) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message,
            },
        }
    }
}

impl From<CloseListingAuctionSessionError> for ApiError {
    fn from(error: CloseListingAuctionSessionError) -> Self {
        match error {
            CloseListingAuctionSessionError::AuctionNotFound => Self {
                status: StatusCode::NOT_FOUND,
                message: "listing not found".to_string(),
            },
            CloseListingAuctionSessionError::AuctionNotEnded => Self {
                status: StatusCode::BAD_REQUEST,
                message: "listing has not reached its end time".to_string(),
            },
            CloseListingAuctionSessionError::WalletError(message) => Self {
                status: StatusCode::PAYMENT_REQUIRED,
                message,
            },
            CloseListingAuctionSessionError::DatabaseError(message) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message,
            },
        }
    }
}

impl From<PlaceBidError> for ApiError {
    fn from(error: PlaceBidError) -> Self {
        match error {
            PlaceBidError::AuctionNotFound => Self {
                status: StatusCode::NOT_FOUND,
                message: "listing not found".to_string(),
            },
            PlaceBidError::BidError(error) => Self {
                status: StatusCode::BAD_REQUEST,
                message: error.to_string(),
            },
            PlaceBidError::CatalogError(message) => Self {
                status: StatusCode::BAD_REQUEST,
                message,
            },
            PlaceBidError::WalletError(message) => Self {
                status: StatusCode::PAYMENT_REQUIRED,
                message,
            },
            PlaceBidError::DatabaseError(message) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message,
            },
        }
    }
}

impl From<ListBidsError> for ApiError {
    fn from(error: ListBidsError) -> Self {
        match error {
            ListBidsError::InvalidInput(message) => Self {
                status: StatusCode::BAD_REQUEST,
                message,
            },
            ListBidsError::DatabaseError(message) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message,
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorResponse {
                message: self.message,
            }),
        )
            .into_response()
    }
}
