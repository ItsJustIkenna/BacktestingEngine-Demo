# syntax=docker/dockerfile:1
# API-ONLY BUILD: Lean container for the Rust API server
# UI is served separately via nginx/CDN in production (see k8s/ for orchestration)
# Build: docker build -f deployment/Dockerfile -t backtesting-engine:latest .
# Build context MUST be parent directory containing: BacktestingEngine/, databaseschema/, LoggingEngine/

ARG RUST_VERSION=1.82.0
ARG APP_NAME=program

################################################################################
# Stage 1: Build the Rust backend
FROM rust:${RUST_VERSION}-slim-bookworm AS backend-build
ARG APP_NAME

# Install necessary build dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    pkg-config \
    libssl-dev \
    libpq-dev \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

# Set performance-optimized environment variables
ENV RUSTFLAGS="-C target-cpu=native -C opt-level=3 -C codegen-units=1 -C panic=abort"
ENV RUST_BACKTRACE=0

# Set up workspace structure
WORKDIR /app

# Copy dependencies
COPY databaseschema/ ./databaseschema/
COPY LoggingEngine/ ./LoggingEngine/

# Copy BacktestingEngine
COPY BacktestingEngine/ ./BacktestingEngine/

# Build from BacktestingEngine directory
WORKDIR /app/BacktestingEngine

# Build the application with postgres feature
RUN cargo build --locked --release --features program/postgres && \
    cp target/release/$APP_NAME /bin/server

################################################################################
# Stage 2: Create minimal runtime image
FROM debian:bookworm-slim AS runtime
ARG APP_NAME=program

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libpq5 \
    libssl3 \
    curl \
    postgresql-client \
    && rm -rf /var/lib/apt/lists/*

# Copy backend binary
COPY --from=backend-build /bin/server /bin/server

# Create non-root user
RUN useradd -m -u 1000 appuser && \
    chown appuser:appuser /bin/server

USER appuser
WORKDIR /home/appuser

# Expose API port
EXPOSE 8080

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=30s --retries=3 \
  CMD curl -f http://localhost:8080/health || exit 1

# Metadata
LABEL maintainer="Nwagbara-Group-LLC"
LABEL description="BacktestingEngine API - Trading Strategy Backtesting Platform"
LABEL version="2.0.0"

# Run the API server
CMD ["/bin/server", "serve"]
