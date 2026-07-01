# Root Dockerfile — builds kova-gateway (the public-facing service).
#
# This file exists so Render can find a Dockerfile at the repo root when
# the service is configured with default settings.
#
# For a full multi-service deployment (auth, account, ledger, gateway as
# separate Render services) use the render.yaml Blueprint instead:
#   Render dashboard → New → Blueprint → select this repo.
#
# Per-service Dockerfiles live at:
#   services/kova-gateway/Dockerfile    ← this file delegates to here
#   services/kova-auth/Dockerfile
#   services/kova-account/Dockerfile
#   services/kova-ledger/Dockerfile

# ── Stage 1: cargo-chef planner ───────────────────────────────────────────────
FROM rust:1-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

# ── Stage 2: dependency planner ───────────────────────────────────────────────
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ── Stage 3: dependency builder ───────────────────────────────────────────────
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

COPY . .
RUN cargo build --release --bin kova-gateway

# ── Stage 4: minimal runtime image ────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        libssl3 \
        curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/kova-gateway /usr/local/bin/kova-gateway

EXPOSE 8080
ENV RUST_LOG=info
ENV KOVA_GATEWAY_ADDR=0.0.0.0:8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD curl -sf http://localhost:8080/api/v1/kova/health || exit 1

CMD ["kova-gateway"]
