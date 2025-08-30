# syntax=docker/dockerfile:1

ARG RUST_VERSION=1.82.0
ARG NODE_VERSION=18
ARG APP_NAME=program

################################################################################
# Stage 1: Build the React UI
FROM node:${NODE_VERSION}-alpine AS ui-build

WORKDIR /app/gui
COPY gui/package*.json ./
RUN npm ci --only=production

COPY gui/ ./
RUN npm run build

################################################################################
# Stage 2: Build the Rust backend
FROM rust:${RUST_VERSION}-slim-bullseye AS backend-build
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

# Copy the BacktestingEngine source code
COPY . .

# Build the application
RUN cargo test --locked --release && \
    cargo build --locked --release && \
    cp target/release/$APP_NAME /bin/server

################################################################################
# Stage 3: Create final runtime image with UI and backend
FROM nginx:1.25-alpine AS runtime

# Install runtime dependencies including supervisord for multi-process
RUN apk add --no-cache \
    supervisor \
    ca-certificates \
    postgresql-client \
    redis \
    && rm -rf /var/cache/apk/*

# Copy React UI build
COPY --from=ui-build /app/gui/build /usr/share/nginx/html

# Copy backend binary
COPY --from=backend-build /bin/server /usr/local/bin/backtesting-server

# Copy nginx configuration
COPY nginx.conf /etc/nginx/conf.d/default.conf

# Copy supervisord configuration  
COPY supervisord.conf /etc/supervisord.conf

# Expose ports: 80 for UI, 8080 for API
EXPOSE 80 8080

# Health check for both services
HEALTHCHECK --interval=30s --timeout=15s --start-period=60s --retries=3 \
  CMD wget --no-verbose --tries=1 --spider http://localhost/ || exit 1

# Metadata
LABEL maintainer="Nwagbara-Group-LLC"
LABEL description="BacktestingEngine - Advanced Trading Strategy Backtesting Platform with React UI"
LABEL version="2.0.0"

# Start supervisord to manage both nginx and backend
CMD ["/usr/bin/supervisord", "-c", "/etc/supervisord.conf"]
