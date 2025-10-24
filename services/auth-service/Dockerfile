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
COPY services/auth-service/ services/auth-service/

# Build dependencies
RUN cargo build --release --bin auth-service

# Copy source code
COPY services/auth-service/src/ services/auth-service/src/

# Build the application
RUN cargo build --release --bin auth-service

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
COPY --from=builder /app/target/release/auth-service /usr/local/bin/auth-service

# Set ownership
RUN chown appuser:appuser /usr/local/bin/auth-service

# Switch to non-root user
USER appuser

# Expose port
EXPOSE 50051

# Run the service
CMD ["auth-service"]
