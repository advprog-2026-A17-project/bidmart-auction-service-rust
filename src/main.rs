use bidmart_auction_service_rust::persistence::repositories::OutboxRepository;
use bidmart_auction_service_rust::scheduler::auction_closure_scheduler::AuctionClosureScheduler;
use bidmart_auction_service_rust::scheduler::outbox_scheduler::OutboxScheduler;
use bidmart_auction_service_rust::scheduler::rabbitmq_outbox_publisher::RabbitMqOutboxPublisher;
use bidmart_auction_service_rust::config::{
    resolve_auction_closure_interval_ms, resolve_bind_address, resolve_database_url,
    resolve_events_exchange, resolve_outbox_interval_ms, resolve_rabbitmq_url,
};
use bidmart_auction_service_rust::server::{
    build_router, connect_pool, run_migrations,
};
use dotenvy::from_path;
use std::time::Duration;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = from_path(".env");
    let _ = dotenvy::from_path_override("../bidmart-infrastructure/.env");

    let database_url = resolve_database_url();
    let bind_address = resolve_bind_address();
    let scheduler_interval_ms = resolve_auction_closure_interval_ms();

    let pool = connect_pool(&database_url).await?;
    run_migrations(&pool).await?;

    let (app, auction_service) = build_router(pool.clone());

    // Auction closure scheduler (closes expired auctions)
    let scheduler = AuctionClosureScheduler::new(auction_service);
    let _scheduler_handle = scheduler.spawn_polling(Duration::from_millis(scheduler_interval_ms));

    // Outbox scheduler → RabbitMQ (publishes domain events to the notification service)
    let rabbitmq_url = resolve_rabbitmq_url();
    let exchange = resolve_events_exchange();
    let outbox_interval_ms = resolve_outbox_interval_ms();

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
