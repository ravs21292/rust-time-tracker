#!/bin/bash

echo "🧪 Testing Time Tracker Backend Services"
echo "======================================"

# Check if services are running
echo "🔍 Checking running services..."
ps aux | grep -E "(auth-service|activity-service|screenshot-service|api-gateway)" | grep -v grep

echo ""
echo "🌐 Checking ports..."
netstat -tlnp | grep -E "(50051|50052|50053|8080)" || echo "No services listening on expected ports"

echo ""
echo "📊 Database Status:"
sudo -u postgres psql time_tracker -c "SELECT COUNT(*) as employee_count FROM employees;" 2>/dev/null || echo "Database not accessible"

echo ""
echo "🔴 Redis Status:"
redis-cli ping 2>/dev/null || echo "Redis not running"

echo ""
echo "📋 How Services Communicate:"
echo "1. User Login → Auth Service (50051) → Returns session_token + employee_id"
echo "2. Activity Tracking → Activity Service (50052) → Stores data with employee_id"
echo "3. Screenshot Capture → Screenshot Service (50053) → Stores images with employee_id"
echo "4. API Gateway (8080) → Routes requests to appropriate services"
echo ""
echo "💾 All data is linked by employee_id in PostgreSQL:"
echo "   - employees table: user info"
echo "   - activity_logs table: time tracking data"
echo "   - screenshots table: captured images"
echo "   - user_sessions table: active sessions"
