use std::future::Future;
use std::time::Duration;

use thiserror::Error;
use tokio::task::JoinHandle;

use crate::client::http_service_client::{HttpServiceClient, HttpServiceClientError};
use crate::persistence::models::OutboxEventRecord;
use crate::persistence::repositories::OutboxRepository;

#[derive(Debug, Clone)]
pub struct OutboxScheduler {
    outbox_repo: OutboxRepository,
}

impl OutboxScheduler {
    pub fn new(outbox_repo: OutboxRepository) -> Self {
        Self { outbox_repo }
    }

    pub async fn publish_pending<F, Fut>(
        &self,
        limit: i64,
        mut publish: F,
    ) -> Result<OutboxPublishReport, OutboxSchedulerError>
    where
        F: FnMut(OutboxEventRecord) -> Fut,
        Fut: Future<Output = Result<(), OutboxPublishError>>,
    {
        let events = self
            .outbox_repo
            .list_pending(limit)
            .await
            .map_err(|error| OutboxSchedulerError::DatabaseError(error.to_string()))?;

        let mut report = OutboxPublishReport {
            attempted: events.len(),
            published: 0,
            failed: 0,
        };

        for event in events {
            match publish(event.clone()).await {
                Ok(()) => {
                    let published_at = chrono::Utc::now().timestamp();
                    self.outbox_repo
                        .mark_published(&event.id, published_at)
                        .await
                        .map_err(|error| OutboxSchedulerError::DatabaseError(error.to_string()))?;
                    report.published += 1;
                }
                Err(_) => {
                    report.failed += 1;
                }
            }
        }

        Ok(report)
    }

    pub fn spawn_polling<F, Fut>(
        &self,
        interval: Duration,
        limit: i64,
        publish: F,
    ) -> JoinHandle<()>
    where
        F: Fn(OutboxEventRecord) -> Fut + Clone + Send + 'static,
        Fut: Future<Output = Result<(), OutboxPublishError>> + Send + 'static,
    {
        let scheduler = self.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);

            loop {
                ticker.tick().await;
                let publish = publish.clone();
                let _ = scheduler.publish_pending(limit, publish).await;
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxPublishReport {
    pub attempted: usize,
    pub published: usize,
    pub failed: usize,
}

#[derive(Debug, Clone)]
pub struct HttpOutboxPublisher {
    client: HttpServiceClient,
    path: String,
}

impl HttpOutboxPublisher {
    pub fn new(
        base_url: impl AsRef<str>,
        path: impl Into<String>,
    ) -> Result<Self, OutboxPublishError> {
        let client = HttpServiceClient::new(base_url, "outbox relay")
            .map_err(|error| OutboxPublishError::new(error.to_string()))?;
        Ok(Self {
            client,
            path: path.into(),
        })
    }

    pub async fn publish(&self, event: OutboxEventRecord) -> Result<(), OutboxPublishError> {
        let payload = serde_json::from_str::<serde_json::Value>(&event.payload)
            .unwrap_or_else(|_| serde_json::Value::String(event.payload.clone()));
        let envelope = serde_json::json!({
            "id": event.id,
            "aggregate_id": event.aggregate_id,
            "event_type": event.event_type,
            "payload": payload,
            "created_at": event.created_at
        });
        let response = self
            .client
            .post_json(
                self.path.clone(),
                serde_json::to_vec(&envelope)
                    .map_err(|error| OutboxPublishError::new(error.to_string()))?,
            )
            .await
            .map_err(map_http_error)?;

        if !response.status.is_success() {
            return Err(OutboxPublishError::new(format!(
                "outbox relay returned {}",
                response.status
            )));
        }

        Ok(())
    }
}

fn map_http_error(error: HttpServiceClientError) -> OutboxPublishError {
    OutboxPublishError::new(error.to_string())
}

#[derive(Debug, Clone, Error)]
#[error("{message}")]
pub struct OutboxPublishError {
    message: String,
}

impl OutboxPublishError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[derive(Debug, Error)]
pub enum OutboxSchedulerError {
    #[error("Database error: {0}")]
    DatabaseError(String),
}
