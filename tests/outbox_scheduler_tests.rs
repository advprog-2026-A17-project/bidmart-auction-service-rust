use std::sync::{Arc, Mutex};
use std::time::Duration;

use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};

use bidmart_auction_service_rust::persistence::models::NewOutboxEventRecord;
use bidmart_auction_service_rust::persistence::repositories::OutboxRepository;
use bidmart_auction_service_rust::scheduler::outbox_scheduler::{
    OutboxPublishError, OutboxScheduler,
};

async fn setup_test_db() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("connect to in-memory db");

    let sql = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("migrations/20260428000000_init.sql"),
    )
    .expect("read migration");

    for statement in sql.split(';') {
        let trimmed = statement.trim();
        if !trimmed.is_empty() {
            sqlx::query(trimmed)
                .execute(&pool)
                .await
                .expect("execute migration");
        }
    }

    pool
}

#[tokio::test]
async fn publish_pending_marks_successes_and_keeps_failures_pending() {
    let pool = setup_test_db().await;
    let outbox_repo = OutboxRepository::new(pool);
    let scheduler = OutboxScheduler::new(outbox_repo.clone());
    let now = chrono::Utc::now().timestamp();

    let success_event = NewOutboxEventRecord {
        id: "event-success".to_string(),
        aggregate_id: "auction-1".to_string(),
        event_type: "BidPlaced".to_string(),
        payload: r#"{"auction_id":"auction-1"}"#.to_string(),
        published: false,
        published_at: None,
        created_at: now,
        updated_at: now,
    };
    let failed_event = NewOutboxEventRecord {
        id: "event-fail".to_string(),
        aggregate_id: "auction-2".to_string(),
        event_type: "BidPlaced".to_string(),
        payload: r#"{"auction_id":"auction-2"}"#.to_string(),
        published: false,
        published_at: None,
        created_at: now + 1,
        updated_at: now + 1,
    };
    outbox_repo
        .insert(&success_event)
        .await
        .expect("insert success event");
    outbox_repo
        .insert(&failed_event)
        .await
        .expect("insert failed event");

    let published_ids = Arc::new(Mutex::new(Vec::<String>::new()));
    let publisher_ids = published_ids.clone();

    let report = scheduler
        .publish_pending(10, move |event| {
            let publisher_ids = publisher_ids.clone();
            async move {
                publisher_ids
                    .lock()
                    .expect("lock published ids")
                    .push(event.id.clone());

                if event.id == "event-fail" {
                    return Err(OutboxPublishError::new("publisher unavailable"));
                }

                Ok(())
            }
        })
        .await
        .expect("publish pending");

    assert_eq!(report.attempted, 2);
    assert_eq!(report.published, 1);
    assert_eq!(report.failed, 1);
    assert_eq!(
        published_ids.lock().expect("lock published ids").as_slice(),
        ["event-success", "event-fail"]
    );

    let pending = outbox_repo.list_pending(10).await.expect("list pending");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "event-fail");
}

#[tokio::test]
async fn spawned_outbox_scheduler_publishes_pending_events() {
    let pool = setup_test_db().await;
    let outbox_repo = OutboxRepository::new(pool);
    let scheduler = OutboxScheduler::new(outbox_repo.clone());
    let now = chrono::Utc::now().timestamp();

    outbox_repo
        .insert(&NewOutboxEventRecord {
            id: "event-spawned".to_string(),
            aggregate_id: "auction-spawned".to_string(),
            event_type: "BidPlaced".to_string(),
            payload: r#"{"auction_id":"auction-spawned"}"#.to_string(),
            published: false,
            published_at: None,
            created_at: now,
            updated_at: now,
        })
        .await
        .expect("insert event");

    let published_ids = Arc::new(Mutex::new(Vec::<String>::new()));
    let publisher_ids = published_ids.clone();
    let handle = scheduler.spawn_polling(Duration::from_millis(10), 10, move |event| {
        let publisher_ids = publisher_ids.clone();
        async move {
            publisher_ids
                .lock()
                .expect("lock published ids")
                .push(event.id);
            Ok(())
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    handle.abort();

    let pending = outbox_repo.list_pending(10).await.expect("list pending");
    assert!(pending.is_empty());
    assert_eq!(
        published_ids.lock().expect("lock published ids").as_slice(),
        ["event-spawned"]
    );
}
