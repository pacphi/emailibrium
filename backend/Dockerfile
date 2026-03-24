# Stage 1: Build
FROM rust:1.94-slim AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
COPY migrations/ migrations/
RUN cargo build --release

# Stage 2: Runtime
FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/emailibrium /app/emailibrium
COPY --from=builder /app/migrations /app/migrations
COPY entrypoint.sh /app/entrypoint.sh
RUN chmod +x /app/entrypoint.sh
WORKDIR /app
USER 1000:1000
ENTRYPOINT ["/app/entrypoint.sh"]
CMD ["/app/emailibrium"]

# Stage 3: Development (for docker-compose.dev.yml)
FROM rust:1.94-slim AS development
RUN cargo install cargo-watch
WORKDIR /app
CMD ["cargo", "watch", "-x", "run"]
