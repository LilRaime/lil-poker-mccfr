FROM rust:1-slim AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release --bin play_live

FROM gcr.io/distroless/cc-debian12

WORKDIR /app

COPY --from=builder /app/target/release/play_live /usr/local/bin/play_live
COPY models ./models

ENTRYPOINT ["/usr/local/bin/play_live"]
