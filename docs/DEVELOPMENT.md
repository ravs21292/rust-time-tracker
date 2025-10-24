# Time Tracker Development Guide

## Quick Start for Local Development

### Prerequisites
- Rust 1.70+ installed
- PostgreSQL 14+ running locally
- Docker & Docker Compose (optional)

### Option 1: Docker Development (Recommended)

1. **Start all services**:
   ```bash
   cd /var/www/html/RUST/time-tracker
   docker-compose -f docker/docker-compose.yml up -d
   ```

2. **Check service status**:
   ```bash
   docker-compose -f docker/docker-compose.yml ps
   ```

3. **View logs**:
   ```bash
   docker-compose -f docker/docker-compose.yml logs -f auth-service
   ```

### Option 2: Local Development

1. **Setup PostgreSQL**:
   ```bash
   # Install PostgreSQL (Ubuntu/Debian)
   sudo apt update
   sudo apt install postgresql postgresql-contrib
   
   # Start PostgreSQL
   sudo systemctl start postgresql
   sudo systemctl enable postgresql
   
   # Create database
   sudo -u postgres createdb time_tracker
   
   # Run schema
   sudo -u postgres psql time_tracker < database/schema.sql
   ```

2. **Setup environment**:
   ```bash
   cd /var/www/html/RUST/time-tracker
   
   # Create .env file
   cat > .env << EOF
   DATABASE_URL=postgresql://postgres:postgres@localhost:5432/time_tracker
   JWT_SECRET=your-secret-key-change-in-production
   AUTH_SERVICE_PORT=50051
   ACTIVITY_SERVICE_PORT=50052
   SCREENSHOT_SERVICE_PORT=50053
   API_GATEWAY_PORT=8080
   EOF
   ```

3. **Build and run services**:
   ```bash
   # Build all services
   cargo build --workspace
   
   # Run services in separate terminals
   cargo run --bin auth-service
   cargo run --bin activity-service
   cargo run --bin screenshot-service
   cargo run --bin api-gateway
   ```

## Service Architecture

### Authentication Service (Port 50051)
- **Purpose**: User login/logout and session management
- **Endpoints**: Login, Logout, ValidateToken, RefreshToken, GetUserProfile
- **Database**: employees, user_sessions tables

### Activity Service (Port 50052)
- **Purpose**: Time tracking and task management
- **Endpoints**: StartTracking, StopTracking, UpdateActivity, GetActivityLogs
- **Database**: activity_logs, tasks tables

### Screenshot Service (Port 50053)
- **Purpose**: Screenshot capture and storage
- **Endpoints**: CaptureScreenshot, GetScreenshots, DeleteScreenshot
- **Database**: screenshots table

### API Gateway (Port 8080)
- **Purpose**: Service orchestration and load balancing
- **Features**: Request routing, authentication, rate limiting

## Database Schema Details

### Core Tables

#### employees
- Stores employee information and authentication data
- Fields: id, name, email, department, password_hash, is_active

#### activity_logs
- Tracks work sessions and time data
- Fields: id, employee_id, session_id, start_time, end_time, idle_time, active_time, task_name, urls

#### screenshots
- Stores captured screenshots
- Fields: id, employee_id, activity_log_id, timestamp, screenshot_data, file_path

#### tasks
- Manages user tasks and projects
- Fields: id, employee_id, name, description, project_name, is_active

#### user_sessions
- Manages authentication sessions
- Fields: id, employee_id, session_token, expires_at, is_active

## API Testing

### Using grpcurl (Install: `go install github.com/fullstorydev/grpcurl/cmd/grpcurl@latest`)

1. **Test Authentication Service**:
   ```bash
   # Login
   grpcurl -plaintext -d '{"email": "john.doe@company.com", "password": "password123"}' \
     localhost:50051 auth.AuthService/Login
   
   # Validate Token
   grpcurl -plaintext -d '{"session_token": "your-token-here"}' \
     localhost:50051 auth.AuthService/ValidateToken
   ```

2. **Test Activity Service**:
   ```bash
   # Start Tracking
   grpcurl -plaintext -d '{"employee_id": 1, "session_token": "your-token"}' \
     localhost:50052 activity.ActivityService/StartTracking
   ```

## Development Workflow

### 1. Database Changes
- Update `database/schema.sql`
- Run migration: `psql time_tracker < database/schema.sql`

### 2. gRPC Changes
- Update `.proto` files in `proto/` directory
- Regenerate code: `cargo build` in proto directory

### 3. Service Development
- Implement service logic in respective service directories
- Add tests in `tests/` directories
- Update Docker configurations if needed

### 4. Client Development
- OS client will be developed in `client/os-client/`
- Use generated gRPC clients for service communication

## Testing Strategy

### Unit Tests
- Each service has comprehensive unit tests
- Database operations are mocked for isolated testing

### Integration Tests
- Test service-to-service communication
- Database integration tests with test containers

### End-to-End Tests
- Full workflow testing from OS client to database
- Performance testing with multiple concurrent users

## Security Considerations

### Authentication
- JWT tokens with expiration
- Password hashing with bcrypt
- Session management with secure tokens

### Data Protection
- Screenshot data encryption
- Secure gRPC communication
- Input validation and sanitization

### Network Security
- Service-to-service authentication
- Rate limiting and DDoS protection
- CORS configuration for web clients

## Performance Optimization

### Database
- Proper indexing on frequently queried columns
- Connection pooling with SQLx
- Query optimization and caching

### Services
- Async/await for non-blocking operations
- Connection pooling for external services
- Efficient serialization with Prost

### Client
- Minimal resource usage
- Efficient screenshot compression
- Background processing for non-critical operations

## Monitoring and Logging

### Logging
- Structured logging with tracing
- Different log levels for different environments
- Request/response logging for debugging

### Metrics
- Service health checks
- Performance metrics collection
- Error rate monitoring

### Alerting
- Service downtime alerts
- High error rate notifications
- Performance degradation warnings

## Deployment

### Local Development
- Docker Compose for easy local setup
- Hot reloading for development
- Local database with sample data

### Production
- Kubernetes deployment manifests
- Database migration strategies
- Service discovery and load balancing
- Monitoring and alerting setup

## Troubleshooting

### Common Issues

1. **Database Connection Errors**:
   - Check PostgreSQL is running
   - Verify DATABASE_URL in .env
   - Check database exists and schema is loaded

2. **gRPC Connection Errors**:
   - Verify services are running on correct ports
   - Check firewall settings
   - Ensure proto files are compiled

3. **Permission Errors**:
   - Check file permissions for screenshot storage
   - Verify database user permissions
   - Check Docker volume permissions

### Debug Commands

```bash
# Check service logs
docker-compose logs -f [service-name]

# Check database connection
psql $DATABASE_URL -c "SELECT version();"

# Test gRPC service
grpcurl -plaintext localhost:50051 list

# Check Rust compilation
cargo check --workspace
```

## Next Steps

1. Complete remaining microservices (activity, screenshot, api-gateway)
2. Implement OS client application
3. Add comprehensive testing suite
4. Set up CI/CD pipeline
5. Performance testing and optimization
6. Security audit and hardening
