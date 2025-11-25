# syntax=docker/dockerfile:1
# ENTERPRISE BUILD STRATEGY: Build from workspace root containing all dependencies
# Build: docker build -f BacktestingEngine/Dockerfile -t backtesting-engine:latest .
# Build context MUST be parent directory containing: BacktestingEngine/, databaseschema/

ARG RUST_VERSION=1.82.0
ARG NODE_VERSION=18
ARG APP_NAME=program

################################################################################
# Stage 1: Build the React UI
FROM node:${NODE_VERSION}-alpine AS ui-build

WORKDIR /app/gui
COPY BacktestingEngine/gui/package*.json ./
RUN npm ci --only=production

COPY BacktestingEngine/gui/ ./
RUN npm run build

################################################################################
# Stage 2: Build the Rust backend
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

# Copy config files to /tmp for runtime stage
RUN cp /app/BacktestingEngine/nginx.conf /tmp/nginx.conf && \
    cp /app/BacktestingEngine/supervisord.conf /tmp/supervisord.conf

################################################################################
# Stage 3: Create final runtime image with UI and backend
# Using Chainguard's nginx image - minimal attack surface, continuously patched
FROM cgr.dev/chainguard/nginx:latest AS runtime
ARG APP_NAME=program

# Switch to root to install additional packages
USER root

# Install runtime dependencies including supervisord for multi-process
RUN apk update && apk upgrade --no-cache && apk add --no-cache \
    supervisor \
    ca-certificates \
    postgresql-client \
    redis \
    curl \
    bash && \
    rm -rf /var/cache/apk/*

# Copy React UI build
COPY --from=ui-build /app/gui/build /usr/share/nginx/html

# Copy backend binary to /bin/server (matches supervisord.conf)
COPY --from=backend-build /bin/server /bin/server

# Copy nginx configuration
COPY --from=backend-build /tmp/nginx.conf /etc/nginx/conf.d/default.conf

# Copy supervisord configuration
COPY --from=backend-build /tmp/supervisord.conf /etc/supervisord.conf

# Fix nginx PID path for non-root execution
RUN sed -i 's|pid        /var/run/nginx.pid;|pid        /tmp/nginx.pid;|' /etc/nginx/nginx.conf

# Create nginx cache directories with proper permissions
RUN mkdir -p /var/cache/nginx/client_temp \
             /var/cache/nginx/proxy_temp \
             /var/cache/nginx/fastcgi_temp \
             /var/cache/nginx/uwsgi_temp \
             /var/cache/nginx/scgi_temp && \
    chown -R nginx:nginx /var/cache/nginx && \
    chmod -R 755 /var/cache/nginx

# Expose ports: 80 for UI, 8080 for API
EXPOSE 80 8080

# Environment variable to control mode: "all" (default), "api", "worker"
ENV RUN_MODE=all

# Health check for both services
HEALTHCHECK --interval=30s --timeout=15s --start-period=60s --retries=3 \
  CMD curl -f http://localhost/ || exit 1

# Metadata
LABEL maintainer="Nwagbara-Group-LLC"
LABEL description="BacktestingEngine - Advanced Trading Strategy Backtesting Platform with React UI"
LABEL version="2.0.0"

# Entrypoint script to handle different modes
COPY --from=backend-build /app/BacktestingEngine/docker-entrypoint.sh /docker-entrypoint.sh
RUN chmod +x /docker-entrypoint.sh

ENTRYPOINT ["/docker-entrypoint.sh"]
CMD ["all"]
