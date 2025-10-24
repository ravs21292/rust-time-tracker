#!/bin/bash

# Time Tracker Quick Start Script
# This script sets up the development environment for the Time Tracker project

set -e

echo "🚀 Time Tracker Development Setup"
echo "=================================="

# Check if Rust is installed
if ! command -v cargo &> /dev/null; then
    echo "❌ Rust is not installed. Please install Rust first:"
    echo "   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi

echo "✅ Rust is installed: $(cargo --version)"

# Check if PostgreSQL is installed
if ! command -v psql &> /dev/null; then
    echo "❌ PostgreSQL is not installed. Installing..."
    sudo apt update
    sudo apt install -y postgresql postgresql-contrib
fi

echo "✅ PostgreSQL is installed"

# Start PostgreSQL service
echo "🔄 Starting PostgreSQL service..."
sudo systemctl start postgresql
sudo systemctl enable postgresql

# Create database
echo "🔄 Creating database..."
sudo -u postgres createdb time_tracker 2>/dev/null || echo "Database already exists"

# Load schema
echo "🔄 Loading database schema..."
sudo -u postgres psql time_tracker < database/schema.sql

# Create environment file
echo "🔄 Creating environment configuration..."
if [ ! -f .env ]; then
    cp env.example .env
    echo "✅ Created .env file from template"
else
    echo "✅ .env file already exists"
fi

# Build the project
echo "🔄 Building project..."
cargo build --workspace

echo ""
echo "🎉 Setup complete!"
echo ""
echo "Next steps:"
echo "1. Start the authentication service:"
echo "   cargo run --bin auth-service"
echo ""
echo "2. In another terminal, test the service:"
echo "   grpcurl -plaintext -d '{\"email\": \"john.doe@company.com\", \"password\": \"password123\"}' localhost:50051 auth.AuthService/Login"
echo ""
echo "3. Or use Docker for all services:"
echo "   docker-compose -f docker/docker-compose.yml up -d"
echo ""
echo "📚 For more information, see docs/DEVELOPMENT.md"
