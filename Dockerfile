# syntax=docker/dockerfile:1

FROM rust:1.96-bookworm AS builder
WORKDIR /app
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release && cp target/release/atlas-payload-provider /usr/local/bin/atlas-payload-provider

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /usr/local/bin/atlas-payload-provider /usr/local/bin/atlas-payload-provider
RUN mkdir -p /data/payloads

ENV LISTEN_HOST=0.0.0.0 \
    LISTEN_PORT=28883 \
    HTML_TITLE="Atlas Payload Provider" \
    PAYLOAD_DIR=/data/payloads \
    MAX_PAYLOAD_BYTES=1048576 \
    SIGNER_PRIVATE_KEY=""

EXPOSE 28883
ENTRYPOINT ["atlas-payload-provider"]
