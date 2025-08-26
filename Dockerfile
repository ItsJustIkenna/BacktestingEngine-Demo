# syntax=docker/dockerfile:1

ARG RUST_VERSION=1.82.0
ARG APP_NAME=program

################################################################################
# Stage 1: Build the application with optimizations
FROM rust:${RUST_VERSION}-slim-bullseye AS build
ARG APP_NAME

# Install necessary build dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

# Set performance-optimized environment variables
ENV RUSTFLAGS="-C target-cpu=native -C opt-level=3 -C codegen-units=1 -C panic=abort"
ENV RUST_BACKTRACE=0

# Set the working directory inside the container
WORKDIR /app

# Copy the source code into the container
COPY . /app/BacktestingEngine

COPY databaseschema /app/databaseschema

COPY redisutils /app/redisutils

# Ensure the program builds correctly from the workspace
WORKDIR /app/BacktestingEngine

RUN cargo test --locked --release && \
    cargo build --locked --release && \
    cp target/release/$APP_NAME /bin/server

################################################################################
# Stage 2: Create a smaller runtime image
FROM debian:bullseye-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libc6 \
    net-tools \
    procps \
    libssl-dev \
    ca-certificates \
    postgresql-client \
    redis-tools \
    && rm -rf /var/lib/apt/lists/*

# Create the health check script
RUN echo '#!/bin/sh' > /usr/local/bin/health_check.sh \
&& echo 'if ! pgrep "server"; then exit 1; fi' >> /usr/local/bin/health_check.sh \
&& echo 'if ! redis-cli -h $REDIS_HOST -p $REDIS_PORT -a $REDIS_PASSWORD ping | grep -q "PONG"; then exit 1; fi' >> /usr/local/bin/health_check.sh \
&& echo 'if ! pg_isready -h $POSTGRES_HOST -p $POSTGRES_PORT -U $POSTGRES_USERNAME -d $POSTGRES_DB; then exit 1; fi' >> /usr/local/bin/health_check.sh \
&& chmod +x /usr/local/bin/health_check.sh

# Create the liveness probe script
RUN echo '#!/bin/sh' > /usr/local/bin/liveness_check.sh \
&& echo 'if ! pgrep "server"; then exit 1; fi' >> /usr/local/bin/liveness_check.sh \
&& echo 'if ! redis-cli -h $REDIS_HOST -p $REDIS_PORT -a $REDIS_PASSWORD ping | grep -q "PONG"; then exit 1; fi' >> /usr/local/bin/liveness_check.sh \
&& echo 'if ! pg_isready -h $POSTGRES_HOST -p $POSTGRES_PORT -U $POSTGRES_USERNAME -d $POSTGRES_DB; then exit 1; fi' >> /usr/local/bin/liveness_check.sh \
&& chmod +x /usr/local/bin/liveness_check.sh

# Create a non-privileged user to run the app
ARG UID=10001
RUN adduser --disabled-password --gecos "" --home "/nonexistent" --shell "/sbin/nologin" --no-create-home --uid "${UID}" appuser

# Copy the built application from the build stage
COPY --from=build /bin/server /bin/server

# Ensure the binary is executable
RUN chmod +x /bin/server

EXPOSE 443

# Switch to non-privileged user
USER appuser

# Set the command to run the application
CMD ["/bin/server"]
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
