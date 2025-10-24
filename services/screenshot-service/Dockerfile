# Multi-stage build for high-performance Rust services
FROM rust:1.70-slim as builder

# Install system dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libpq-dev \
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /app

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY proto/ proto/
COPY services/screenshot-service/ services/screenshot-service/

# Build dependencies
RUN cargo build --release --bin screenshot-service

# Copy source code
COPY services/screenshot-service/src/ services/screenshot-service/src/

# Build the application
RUN cargo build --release --bin screenshot-service

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libssl3 \
    libpq5 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create app user and directories
RUN useradd -r -s /bin/false appuser && \
    mkdir -p /app/screenshots && \
    chown -R appuser:appuser /app

# Copy binary from builder
COPY --from=builder /app/target/release/screenshot-service /usr/local/bin/screenshot-service

# Set ownership
RUN chown appuser:appuser /usr/local/bin/screenshot-service

# Switch to non-root user
USER appuser

# Expose port
EXPOSE 50053

# Run the service
CMD ["screenshot-service"]
