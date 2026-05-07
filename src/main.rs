use bidmart_auction_service_rust::server::{
    build_router, connect_pool, default_database_url, run_migrations,
};
use bidmart_auction_service_rust::persistence::repositories::OutboxRepository;
use bidmart_auction_service_rust::scheduler::auction_closure_scheduler::AuctionClosureScheduler;
use bidmart_auction_service_rust::scheduler::outbox_scheduler::OutboxScheduler;
use bidmart_auction_service_rust::scheduler::rabbitmq_outbox_publisher::RabbitMqOutboxPublisher;
use dotenvy::from_path;
use std::env;
use std::time::Duration;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = from_path(".env");
    let _ = dotenvy::from_path_override("../bidmart-infrastructure/.env");

    let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| default_database_url());
    let bind_address = env::var("BIND_ADDRESS").unwrap_or_else(|_| "0.0.0.0:3000".to_string());
    let scheduler_interval_ms = env::var("AUCTION_CLOSURE_SCHEDULER_INTERVAL_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000);

    let pool = connect_pool(&database_url).await?;
    run_migrations(&pool).await?;

    let (app, auction_service) = build_router(pool.clone());

    // Auction closure scheduler (closes expired auctions)
    let scheduler = AuctionClosureScheduler::new(auction_service);
    let _scheduler_handle = scheduler.spawn_polling(Duration::from_millis(scheduler_interval_ms));

    // Outbox scheduler → RabbitMQ (publishes domain events to the notification service)
    let rabbitmq_url =
        env::var("RABBITMQ_URL").unwrap_or_else(|_| "amqp://guest:guest@localhost:5672/%2f".to_string());
    let exchange =
        env::var("BIDMART_EVENTS_EXCHANGE").unwrap_or_else(|_| "bidmart.events".to_string());
    let outbox_interval_ms: u64 = env::var("OUTBOX_SCHEDULER_INTERVAL_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000);

    let outbox_repo = OutboxRepository::new(pool);
    let outbox_scheduler = OutboxScheduler::new(outbox_repo);
    let rmq_publisher = RabbitMqOutboxPublisher::new(rabbitmq_url, exchange);
    let _outbox_handle = outbox_scheduler.spawn_polling(
        Duration::from_millis(outbox_interval_ms),
        50,
        rmq_publisher.publisher_fn(),
    );

    let listener = TcpListener::bind(&bind_address).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

