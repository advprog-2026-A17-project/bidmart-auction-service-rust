use std::pin::Pin;
use std::sync::Arc;

use lapin::{
    options::BasicPublishOptions, BasicProperties, Channel, Connection, ConnectionProperties,
};
use tokio::sync::RwLock;

use crate::persistence::models::OutboxEventRecord;
use crate::scheduler::outbox_scheduler::OutboxPublishError;

/// Publishes outbox events to RabbitMQ using the `lapin` AMQP client.
///
/// Events stored in the outbox use domain-level `event_type` values like
/// `"AuctionEnded"` or `"BidPlaced"`.  The notification/order service listens
/// on a topic exchange with routing keys such as `auction.ended.v1` and
/// `auction.bid-placed.v1`.  This publisher maps the domain event type to the
/// appropriate routing key before publishing.
#[derive(Clone)]
pub struct RabbitMqOutboxPublisher {
    amqp_url: String,
    exchange: String,
    state: Arc<RwLock<Option<AmqpState>>>,
}

/// Holds the AMQP connection and channel so neither is dropped prematurely.
struct AmqpState {
    _connection: Connection,
    channel: Channel,
}

impl RabbitMqOutboxPublisher {
    pub fn new(amqp_url: impl Into<String>, exchange: impl Into<String>) -> Self {
        Self {
            amqp_url: amqp_url.into(),
            exchange: exchange.into(),
            state: Arc::new(RwLock::new(None)),
        }
    }

    /// Obtain (or lazily create) an AMQP channel.
    async fn channel(&self) -> Result<Channel, OutboxPublishError> {
        // Fast path: reuse existing open channel.
        {
            let guard = self.state.read().await;
            if let Some(st) = guard.as_ref() {
                if st.channel.status().connected() {
                    return Ok(st.channel.clone());
                }
            }
        }

        // Slow path: (re-)connect.
        let mut guard = self.state.write().await;
        // Double-check after acquiring write lock.
        if let Some(st) = guard.as_ref() {
            if st.channel.status().connected() {
                return Ok(st.channel.clone());
            }
        }

        let conn = Connection::connect(&self.amqp_url, ConnectionProperties::default())
            .await
            .map_err(|e| OutboxPublishError::new(format!("AMQP connect: {e}")))?;

        let ch = conn
            .create_channel()
            .await
            .map_err(|e| OutboxPublishError::new(format!("AMQP channel: {e}")))?;

        let channel = ch.clone();
        *guard = Some(AmqpState {
            _connection: conn,
            channel: ch,
        });
        Ok(channel)
    }

    /// Publish a single outbox event to RabbitMQ.
    pub async fn publish(&self, event: OutboxEventRecord) -> Result<(), OutboxPublishError> {
        let channel = self.channel().await?;
        let routing_key = event_type_to_routing_key(&event.event_type);

        let payload = serde_json::from_str::<serde_json::Value>(&event.payload)
            .unwrap_or_else(|_| serde_json::Value::String(event.payload.clone()));

        let envelope = serde_json::json!({
            "eventId": event.id,
            "aggregateId": event.aggregate_id,
            "eventType": routing_key,
            "eventVersion": 1,
            "payload": payload,
            "createdAt": event.created_at
        });

        let body = serde_json::to_vec(&envelope)
            .map_err(|e| OutboxPublishError::new(format!("JSON serialize: {e}")))?;

        channel
            .basic_publish(
                &self.exchange,
                &routing_key,
                BasicPublishOptions::default(),
                &body,
                BasicProperties::default()
                    .with_content_type("application/json".into())
                    .with_delivery_mode(2), // persistent
            )
            .await
            .map_err(|e| OutboxPublishError::new(format!("AMQP publish: {e}")))?
            .await
            .map_err(|e| OutboxPublishError::new(format!("AMQP confirm: {e}")))?;

        Ok(())
    }

    /// Return a closure suitable for `OutboxScheduler::spawn_polling`.
    pub fn publisher_fn(
        &self,
    ) -> impl Fn(OutboxEventRecord) -> Pin<Box<dyn std::future::Future<Output = Result<(), OutboxPublishError>> + Send>>
           + Clone
           + Send
           + 'static {
        let this = self.clone();
        move |event: OutboxEventRecord| {
            let publisher = this.clone();
            Box::pin(async move { publisher.publish(event).await })
        }
    }
}

/// Map domain-level event types stored in the outbox to RabbitMQ routing keys
/// expected by the notification/order service.
fn event_type_to_routing_key(event_type: &str) -> String {
    match event_type {
        "AuctionEnded" => "auction.ended.v1".to_string(),
        "BidPlaced" => "auction.bid-placed.v1".to_string(),
        "Outbid" => "auction.outbid.v1".to_string(),
        other => other.to_string(),
    }
}
