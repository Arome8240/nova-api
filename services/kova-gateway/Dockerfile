# ── Stage 1: cargo-chef planner ───────────────────────────────────────────────
# Generates a minimal "recipe" (Cargo.toml dependency tree) so that the
# dependency-compilation layer can be cached independently of source changes.
FROM rust:1-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

# ── Stage 2: dependency planner ───────────────────────────────────────────────
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ── Stage 3: dependency builder ───────────────────────────────────────────────
# This layer is only invalidated when Cargo.toml / Cargo.lock change.
# Source-only changes skip straight to stage 4.
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Copy source and compile the gateway binary.
COPY . .
RUN cargo build --release --bin kova-gateway

# ── Stage 4: minimal runtime image ────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/kova-gateway /usr/local/bin/kova-gateway

# Render routes traffic to the first EXPOSE port.
EXPOSE 8080
ENV RUST_LOG=info
ENV KOVA_GATEWAY_ADDR=0.0.0.0:8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -sf http://localhost:8080/api/v1/kova/health || exit 1

CMD ["kova-gateway"]
