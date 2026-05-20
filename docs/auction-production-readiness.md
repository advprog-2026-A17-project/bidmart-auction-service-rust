# Auction Production Readiness

This document records the production decisions used by the auction service implementation.

## Database-Backed Concurrency Control

Bid placement is serialized per auction with an in-process mutex and persisted in one database transaction. The current highest bid is reloaded inside the critical path before accepting a new bid, so concurrent buyers cannot overwrite each other or skip the minimum increment rule.

## Equal Bid Fairness

Equal bid ordering is deterministic. The service orders bids by amount descending, bid time ascending, then bid id ascending, so two equal bids always resolve to the earliest accepted bid with a stable tie-breaker.

## Gateway REST + Internal gRPC

External clients use the API gateway REST contract. Internal modules can use the service boundary without sharing database state, preserving clear ownership for catalogue, wallet, auction, and order modules.

## RabbitMQ Outbox Relay

Auction events are written to the outbox in the same database transaction as the state change. The relay publishes `AuctionCreated`, `BidPlaced`, `Outbid`, and `AuctionEnded` to RabbitMQ with versioned routing keys so catalogue projections, realtime notifications, and order creation can process them asynchronously.

## Idempotency

The bid path supports retry-safe behavior for duplicate bid details, and consumers deduplicate by event id. This keeps retries safe when a client, gateway, or message broker repeats a request/event after a transient failure.

## Production Migration Strategy

Schema changes are managed through migrations. Runtime deployment should apply migrations before serving traffic, then start the API and schedulers after the schema is compatible.

## Load Testing

Load testing targets the high-contention bid path because it is the critical system path. The expected scenario is many buyers bidding on the same listing while the service preserves ordering, fund holds, anti-sniping extension, and outbox publication.

## Domain Reconstruction

The service reconstructs the domain state from persisted auction and bid records before enforcing rules. This keeps lifecycle decisions deterministic even after process restarts.

## Auction Type Strategy

Auction type handling is isolated behind strategy selection. English Auction is enabled for the current BidMart scope, while unsupported auction modes are rejected explicitly until their rules are implemented.
