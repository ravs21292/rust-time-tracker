# Time Tracker Software

A comprehensive employee time tracking system built with Rust, gRPC, PostgreSQL, and microservice architecture.

## Project Overview

This project consists of:
- **Backend**: Rust-based microservices with gRPC communication
- **Database**: PostgreSQL with proper schema design
- **OS Client**: Linux desktop application for employee activity tracking
- **Architecture**: Scalable microservice design for multiple concurrent users

## Features

### OS Software Features
1. **User Authentication**: Secure login system
2. **Activity Tracking**: Start/stop tracking with task assignment
3. **Screenshot Capture**: Automatic periodic screenshots
4. **Browser URL Monitoring**: Track active browser URLs
5. **Idle Time Detection**: Monitor and log idle periods
6. **Time Calculation**: Total time + idle time reporting
7. **Logout**: Secure session termination

### Backend Services
- **Authentication Service**: User login/logout management
- **Activity Tracking Service**: Time tracking and task management
- **Screenshot Service**: Image capture and storage
- **API Gateway**: Service orchestration and load balancing

## Architecture

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   OS Client     │    │   API Gateway   │    │   Auth Service  │
│   (Linux App)   │◄──►│   (gRPC)        │◄──►│   (gRPC)        │
└─────────────────┘    └─────────────────┘    └─────────────────┘
                                │
                       ┌────────┴────────┐
                       │                 │
                ┌─────────────┐  ┌─────────────┐
                │   Activity  │  │ Screenshot  │
                │   Service   │  │   Service   │
                │   (gRPC)    │  │   (gRPC)    │
                └─────────────┘  └─────────────┘
                       │                 │
                       └────────┬────────┘
                                │
                       ┌─────────────┐
                       │ PostgreSQL  │
                       │  Database   │
                       └─────────────┘
```

## Database Schema

### Tables

#### employees
```sql
CREATE TABLE employees (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    email VARCHAR(255) UNIQUE NOT NULL,
    department VARCHAR(100),
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

#### activity_logs
```sql
CREATE TABLE activity_logs (
    id SERIAL PRIMARY KEY,
    employee_id INTEGER REFERENCES employees(id),
    start_time TIMESTAMP NOT NULL,
    end_time TIMESTAMP,
    idle_time INTEGER DEFAULT 0, -- in seconds
    task_name VARCHAR(255),
    urls TEXT[], -- array of captured URLs
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

#### screenshots
```sql
CREATE TABLE screenshots (
    id SERIAL PRIMARY KEY,
    employee_id INTEGER REFERENCES employees(id),
    timestamp TIMESTAMP NOT NULL,
    screenshot_data BYTEA NOT NULL,
    file_path VARCHAR(500),
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

## Project Structure

```
time-tracker/
├── services/
│   ├── auth-service/
│   ├── activity-service/
│   ├── screenshot-service/
│   └── api-gateway/
├── client/
│   └── os-client/
├── proto/
│   └── *.proto files
├── database/
│   ├── migrations/
│   └── schema.sql
├── docker/
│   └── docker-compose.yml
└── docs/
    └── API.md
```

## Getting Started

### Prerequisites
- Rust 1.70+
- PostgreSQL 14+
- Docker & Docker Compose (optional)
- Linux development environment

### Local Development Setup

1. **Clone and setup**:
   ```bash
   cd /var/www/html/RUST/time-tracker
   ```

2. **Database setup**:
   ```bash
   # Start PostgreSQL
   sudo systemctl start postgresql
   
   # Create database
   createdb time_tracker
   
   # Run migrations
   psql time_tracker < database/schema.sql
   ```

3. **Build services**:
   ```bash
   # Build all services
   cargo build --workspace
   
   # Run services individually
   cargo run --bin auth-service
   cargo run --bin activity-service
   cargo run --bin screenshot-service
   cargo run --bin api-gateway
   ```

4. **Build OS client**:
   ```bash
   cd client/os-client
   cargo build --release
   ```

## Development Phases

### Phase 1: Core Infrastructure
- [x] Project structure setup
- [ ] Database schema and migrations
- [ ] gRPC service definitions
- [ ] Basic microservice framework

### Phase 2: Authentication & Core Services
- [ ] Authentication service implementation
- [ ] Activity tracking service
- [ ] Screenshot capture service
- [ ] API Gateway

### Phase 3: OS Client Development
- [ ] Linux desktop application
- [ ] Screenshot capture functionality
- [ ] Browser URL monitoring
- [ ] Idle time detection

### Phase 4: Integration & Testing
- [ ] Service integration
- [ ] End-to-end testing
- [ ] Performance optimization
- [ ] Security hardening

## Technology Stack

- **Backend**: Rust, Tokio, Tonic (gRPC)
- **Database**: PostgreSQL, SQLx
- **OS Client**: Rust, GTK4/egui
- **Screenshots**: X11/Wayland capture
- **Browser Monitoring**: Chrome DevTools Protocol
- **Containerization**: Docker, Docker Compose

## Security Considerations

- JWT-based authentication
- Encrypted screenshot storage
- Secure gRPC communication
- Input validation and sanitization
- Rate limiting and DDoS protection

## Performance Targets

- Support 100+ concurrent users
- < 100ms API response times
- Efficient screenshot compression
- Minimal system resource usage

## Contributing

1. Follow Rust coding standards
2. Write comprehensive tests
3. Document all public APIs
4. Use conventional commit messages

## License

MIT License - see LICENSE file for details
