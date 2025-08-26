# Multi-stage build for BacktestingEngine
ARG RUST_VERSION=1.75

# Build stage
FROM rust:${RUST_VERSION}-slim-bullseye as builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    ca-certificates \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace manifests
COPY Cargo.toml Cargo.lock ./
COPY config/Cargo.toml ./config/
COPY dataloader/Cargo.toml ./dataloader/
COPY orderbook/Cargo.toml ./orderbook/
COPY portfoliomanager/Cargo.toml ./portfoliomanager/
COPY program/Cargo.toml ./program/
COPY riskmanager/Cargo.toml ./riskmanager/
COPY strategy/Cargo.toml ./strategy/

# Build dependencies (cache layer)
RUN cargo build --release --workspace && rm -rf src/ target/release/deps/backtesting_engine*

# Copy source code
COPY . .

# Build application with optimizations
ENV RUSTFLAGS="-C target-cpu=native"
RUN cargo build --release --workspace

# Runtime stage - distroless for security and minimal attack surface
FROM gcr.io/distroless/cc-debian11:nonroot

WORKDIR /app

# Copy built applications
COPY --from=builder /app/target/release/backtesting-engine /usr/local/bin/backtesting-engine
COPY --from=builder /app/target/release/strategy-runner /usr/local/bin/strategy-runner
COPY --from=builder /app/target/release/portfolio-manager /usr/local/bin/portfolio-manager

# Copy configuration files
COPY --from=builder /app/config/ /etc/backtesting-engine/config/

# Copy sample data and strategies
COPY --from=builder /app/examples/ /app/examples/

# Health check binary
COPY --from=builder /app/target/release/health-check /usr/local/bin/health-check

# Create data directory
USER root
RUN mkdir -p /data /var/log/backtesting-engine && \
    chown -R 65534:65534 /data /var/log/backtesting-engine
USER nonroot

# Metadata
LABEL maintainer="Nwagbara-Group-LLC"
LABEL description="BacktestingEngine - Advanced Trading Strategy Backtesting Platform"
LABEL version="1.0.0"

ARG BUILD_DATE
ARG VCS_REF
LABEL org.label-schema.build-date=$BUILD_DATE
LABEL org.label-schema.vcs-ref=$VCS_REF
LABEL org.label-schema.schema-version="1.0"

# Security: Use non-root user
USER nonroot:nonroot

# Expose ports
EXPOSE 8080 9090 8081

# Health check
HEALTHCHECK --interval=30s --timeout=15s --start-period=120s --retries=3 \
  CMD ["/usr/local/bin/health-check"]

# Volumes for persistent data
VOLUME ["/data", "/var/log/backtesting-engine"]

# Default command
ENTRYPOINT ["/usr/local/bin/backtesting-engine"]
CMD ["--config", "/etc/backtesting-engine/config/production.yaml", "--data-dir", "/data"]
