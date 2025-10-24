#!/bin/bash

# Build script for Time Tracker Backend
# Ensures all services compile correctly

set -e

echo "🔨 Building Time Tracker Backend Services"
echo "========================================"

# Check if Rust is installed
if ! command -v cargo &> /dev/null; then
    echo "❌ Rust is not installed. Please install Rust first:"
    echo "   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi

echo "✅ Rust is installed: $(cargo --version)"

# Build proto files first
echo ""
echo "🔧 Building Protocol Buffer files..."
cd proto
cargo build
cd ..

# Build all services
echo ""
echo "🔨 Building all microservices..."

echo "  - Building Auth Service..."
cargo build --bin auth-service

echo "  - Building Activity Service..."
cargo build --bin activity-service

echo "  - Building Screenshot Service..."
cargo build --bin screenshot-service

echo "  - Building API Gateway..."
cargo build --bin api-gateway

echo ""
echo "✅ All services built successfully!"
echo ""
echo "🚀 Ready to start services:"
echo "   docker-compose -f docker/docker-compose.yml up -d"
echo "   OR"
echo "   cargo run --bin auth-service &"
echo "   cargo run --bin activity-service &"
echo "   cargo run --bin screenshot-service &"
echo "   cargo run --bin api-gateway &"
echo ""
echo "🧪 Test with: ./test_backend.sh"
