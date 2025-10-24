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
COPY services/api-gateway/ services/api-gateway/

# Build dependencies
RUN cargo build --release --bin api-gateway

# Copy source code
COPY services/api-gateway/src/ services/api-gateway/src/

# Build the application
RUN cargo build --release --bin api-gateway

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libssl3 \
    libpq5 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create app user
RUN useradd -r -s /bin/false appuser

# Copy binary from builder
COPY --from=builder /app/target/release/api-gateway /usr/local/bin/api-gateway

# Set ownership
RUN chown appuser:appuser /usr/local/bin/api-gateway

# Switch to non-root user
USER appuser

# Expose port
EXPOSE 8080

# Run the service
CMD ["api-gateway"]
