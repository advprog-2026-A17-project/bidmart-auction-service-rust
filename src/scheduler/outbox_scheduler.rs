use std::future::Future;
use std::time::Duration;

use thiserror::Error;
use tokio::task::JoinHandle;

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
