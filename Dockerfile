FROM rust:1.88-bookworm AS builder

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY migrations migrations/
COPY src src/

# Plain RUN (no BuildKit cache mounts) so Heroku container builds succeed without BuildKit.
RUN cargo build --release --locked \
    && cp target/release/bidmart-auction-service-rust ./bidmart-auction-service-rust

# Reuse the builder base (glibc bookworm) so Compose does not pull debian:bookworm-slim separately.
FROM rust:1.88-bookworm AS runtime

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /data

COPY --from=builder /app/bidmart-auction-service-rust /usr/local/bin/bidmart-auction-service-rust

EXPOSE 8082

ENTRYPOINT ["bidmart-auction-service-rust"]
