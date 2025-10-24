-- Time Tracker Database Schema
-- PostgreSQL 14+

-- Enable UUID extension
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- Create employees table
CREATE TABLE employees (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    email VARCHAR(255) UNIQUE NOT NULL,
    department VARCHAR(100),
    password_hash VARCHAR(255) NOT NULL,
    is_active BOOLEAN DEFAULT true,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Create activity_logs table
CREATE TABLE activity_logs (
    id SERIAL PRIMARY KEY,
    employee_id INTEGER NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    session_id UUID DEFAULT uuid_generate_v4(),
    start_time TIMESTAMP WITH TIME ZONE NOT NULL,
    end_time TIMESTAMP WITH TIME ZONE,
    idle_time INTEGER DEFAULT 0, -- in seconds
    active_time INTEGER DEFAULT 0, -- in seconds
    task_name VARCHAR(255),
    task_description TEXT,
    urls TEXT[], -- array of captured URLs
    is_active BOOLEAN DEFAULT true,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Create screenshots table
CREATE TABLE screenshots (
    id SERIAL PRIMARY KEY,
    employee_id INTEGER NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    activity_log_id INTEGER REFERENCES activity_logs(id) ON DELETE CASCADE,
    timestamp TIMESTAMP WITH TIME ZONE NOT NULL,
    screenshot_data BYTEA,
    file_path VARCHAR(500),
    file_size INTEGER,
    compression_type VARCHAR(50) DEFAULT 'jpeg',
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Create tasks table for task management
CREATE TABLE tasks (
    id SERIAL PRIMARY KEY,
    employee_id INTEGER NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    project_name VARCHAR(255),
    is_active BOOLEAN DEFAULT true,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Create user_sessions table for authentication
CREATE TABLE user_sessions (
    id SERIAL PRIMARY KEY,
    employee_id INTEGER NOT NULL REFERENCES employees(id) ON DELETE CASCADE,
    session_token VARCHAR(500) UNIQUE NOT NULL,
    expires_at TIMESTAMP WITH TIME ZONE NOT NULL,
    is_active BOOLEAN DEFAULT true,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Create indexes for better performance (optimized for 1000+ concurrent users)
CREATE INDEX CONCURRENTLY idx_employees_email ON employees(email);
CREATE INDEX CONCURRENTLY idx_employees_department ON employees(department);
CREATE INDEX CONCURRENTLY idx_employees_active ON employees(is_active) WHERE is_active = true;

-- Activity logs indexes for high-performance queries
CREATE INDEX CONCURRENTLY idx_activity_logs_employee_id ON activity_logs(employee_id);
CREATE INDEX CONCURRENTLY idx_activity_logs_start_time ON activity_logs(start_time);
CREATE INDEX CONCURRENTLY idx_activity_logs_session_id ON activity_logs(session_id);
CREATE INDEX CONCURRENTLY idx_activity_logs_active ON activity_logs(is_active) WHERE is_active = true;
CREATE INDEX CONCURRENTLY idx_activity_logs_employee_time ON activity_logs(employee_id, start_time);
CREATE INDEX CONCURRENTLY idx_activity_logs_session_active ON activity_logs(session_id, is_active);

-- Screenshots indexes for efficient storage and retrieval
CREATE INDEX CONCURRENTLY idx_screenshots_employee_id ON screenshots(employee_id);
CREATE INDEX CONCURRENTLY idx_screenshots_timestamp ON screenshots(timestamp);
CREATE INDEX CONCURRENTLY idx_screenshots_activity_log ON screenshots(activity_log_id);
CREATE INDEX CONCURRENTLY idx_screenshots_employee_time ON screenshots(employee_id, timestamp);

-- Tasks indexes
CREATE INDEX CONCURRENTLY idx_tasks_employee_id ON tasks(employee_id);
CREATE INDEX CONCURRENTLY idx_tasks_active ON tasks(is_active) WHERE is_active = true;

-- User sessions indexes for fast authentication
CREATE INDEX CONCURRENTLY idx_user_sessions_token ON user_sessions(session_token);
CREATE INDEX CONCURRENTLY idx_user_sessions_employee_id ON user_sessions(employee_id);
CREATE INDEX CONCURRENTLY idx_user_sessions_expires ON user_sessions(expires_at);
CREATE INDEX CONCURRENTLY idx_user_sessions_active ON user_sessions(is_active) WHERE is_active = true;

-- Partial indexes for better performance
CREATE INDEX CONCURRENTLY idx_activity_logs_recent ON activity_logs(start_time) WHERE start_time > NOW() - INTERVAL '30 days';
CREATE INDEX CONCURRENTLY idx_screenshots_recent ON screenshots(timestamp) WHERE timestamp > NOW() - INTERVAL '7 days';

-- Create updated_at trigger function
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ language 'plpgsql';

-- Create triggers for updated_at
CREATE TRIGGER update_employees_updated_at BEFORE UPDATE ON employees
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_activity_logs_updated_at BEFORE UPDATE ON activity_logs
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_tasks_updated_at BEFORE UPDATE ON tasks
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- Insert sample data for testing
INSERT INTO employees (name, email, department, password_hash) VALUES
('John Doe', 'john.doe@company.com', 'Engineering', '$2b$12$LQv3c1yqBWVHxkd0LHAkCOYz6TtxMQJqhN8/LewdBPj4J/8Kz8KzK'),
('Jane Smith', 'jane.smith@company.com', 'Marketing', '$2b$12$LQv3c1yqBWVHxkd0LHAkCOYz6TtxMQJqhN8/LewdBPj4J/8Kz8KzK'),
('Bob Johnson', 'bob.johnson@company.com', 'Sales', '$2b$12$LQv3c1yqBWVHxkd0LHAkCOYz6TtxMQJqhN8/LewdBPj4J/8Kz8KzK');

-- Insert sample tasks
INSERT INTO tasks (employee_id, name, description, project_name) VALUES
(1, 'Bug Fix', 'Fix authentication issue in login module', 'Auth System'),
(1, 'Feature Development', 'Implement new dashboard UI', 'Dashboard'),
(2, 'Marketing Campaign', 'Create social media content', 'Q4 Campaign'),
(3, 'Client Meeting', 'Prepare presentation for client', 'Project Alpha');
