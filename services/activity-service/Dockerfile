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
COPY services/activity-service/ services/activity-service/

# Build dependencies
RUN cargo build --release --bin activity-service

# Copy source code
COPY services/activity-service/src/ services/activity-service/src/

# Build the application
RUN cargo build --release --bin activity-service

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
COPY --from=builder /app/target/release/activity-service /usr/local/bin/activity-service

# Set ownership
RUN chown appuser:appuser /usr/local/bin/activity-service

# Switch to non-root user
USER appuser

# Expose port
EXPOSE 50052

# Run the service
CMD ["activity-service"]
