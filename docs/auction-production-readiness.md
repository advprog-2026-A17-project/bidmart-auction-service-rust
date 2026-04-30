# Auction Production Readiness

This document records the implementation choices and remaining operational expectations for the Rust auction service.

## Database-Backed Concurrency Control

The service now has two layers of bid concurrency control.

The first layer is the existing in-process per-auction critical section. It serializes high-contention bid placement inside one service process.

The second layer is database-backed monotonic persistence. Auction highest-bid updates use a guarded database update that only writes when the persisted `current_highest_bid_cents` is empty or lower than the accepted bid. This prevents a stale lower bid from overwriting a higher bid when two service instances process the same auction concurrently.

For a horizontally scaled production deployment, PostgreSQL is preferred over SQLite. PostgreSQL should use a transaction around bid insertion, wallet hold recording, outbox insertion, and auction update. If the team keeps SQLite for local development, it should be treated as local-only storage.

## Equal Bid Fairness

Winning bid selection is deterministic:

1. Highest `bid_amount_cents` wins.
2. Earlier `bid_time` wins when amounts are equal.
3. Lower bid `id` wins when amount and time are equal.

The third rule handles equal-time edge cases deterministically. The domain still rejects bids that do not meet the configured increment, so equal accepted bids are mainly a persistence safety rule for imported data, retries, and edge cases.

## HTTP Outbox Relay

The selected transport for this service is an HTTP outbox relay. `HttpOutboxPublisher` posts an event envelope to a configured relay endpoint. The relay can be implemented by catalogue, a gateway event endpoint, or a small event-router service.

The envelope contains:

- `id`
- `aggregate_id`
- `event_type`
- `payload`
- `created_at`

This keeps the scheduler generic while giving the deployment a real transport. RabbitMQ can replace the HTTP relay later without changing domain or repository code.

## Idempotency

Bid placement retries are idempotent for the current API contract when the retry uses the same auction ID, bidder ID, amount, and bid time. The service returns the existing bid and avoids duplicate outbox events.

Auction close is idempotent for already terminal auctions. Re-closing an auction with `WON` or `UNSOLD` returns the persisted auction and does not emit another `AuctionEnded` event.

Outbox publishing is idempotent through stable outbox event IDs. The relay should treat event `id` as the idempotency key and ignore already processed events.

## Production Migration Strategy

SQLite remains acceptable for local development and automated tests. For staging or production, the recommended strategy is PostgreSQL with sqlx migrations.

Required PostgreSQL follow-up:

- Add PostgreSQL migrations mirroring the current schema.
- Add a monotonically increasing auction version if stricter optimistic locking is needed.
- Wrap bid placement in a database transaction.
- Add indexes for `bids(auction_id, bid_amount_cents DESC, bid_time ASC, id ASC)` and `outbox_events(published, created_at)`.
- Run migration checks in CI.

## Load Testing

Sustained high-volume bidding should be validated outside the unit and integration suite.

Recommended scenario:

- Create one active auction.
- Run 100 to 500 concurrent bid attempts over 30 to 60 seconds.
- Mix valid higher bids, duplicate retry bids, stale low bids, and near-end anti-sniping bids.
- Assert the final highest bid is monotonic, no duplicate retry bids are persisted, and outbox lag remains bounded.

Recommended tools are k6 for HTTP-level tests and a Tokio-based internal harness for service-level stress tests.

## Domain Reconstruction

The service reconstructs domain state from persisted auction rows plus the current winning bid. This is sufficient for the current lifecycle tests, but production should persist and restore every domain field that affects future decisions.

Recommended follow-up:

- Persist extension count.
- Persist final outcome state explicitly.
- Persist anti-sniping configuration per auction.
- Reconstruct the full bid history if future auction strategies need more than the current winning bid.

## Auction Type Strategy

Future auction variants from `BidMart - The Future` should use a strategy boundary instead of conditional logic in `AuctionService`.

Recommended shape:

- `AuctionTypeStrategy` for bid validation and winner selection.
- English auction strategy as the default implementation.
- Scholarship evaluation strategy for non-price winner selection.
- Multi-slot regional strategy for multiple winners.
- Enterprise strategy where wallet fund holding can be optional or disabled.

The service layer should select a strategy from persisted auction type metadata and keep wallet/outbox orchestration outside the strategy.
