use tonic::{transport::Server, Request, Response, Status};
use activity_service::activity_service_server::{ActivityService, ActivityServiceServer};
use activity_service::{
    StartTrackingRequest, StartTrackingResponse, StopTrackingRequest, StopTrackingResponse,
    UpdateActivityRequest, UpdateActivityResponse, GetActivityLogsRequest, GetActivityLogsResponse,
    GetCurrentActivityRequest, GetCurrentActivityResponse, AssignTaskRequest, AssignTaskResponse,
    GetTasksRequest, GetTasksResponse, CreateTaskRequest, CreateTaskResponse,
    ActivityLog, Task
};
use sqlx::{PgPool, Row};
use uuid::Uuid;
use chrono::{Utc, Duration};
use std::env;
use std::sync::Arc;
use dashmap::DashMap;
use redis::Client as RedisClient;
use redis::AsyncCommands;
use std::collections::HashMap;

pub mod activity_service {
    tonic::include_proto!("activity");
}

#[derive(Debug)]
pub struct ActivityServiceImpl {
    db_pool: PgPool,
    redis_client: RedisClient,
    active_sessions: Arc<DashMap<String, ActivitySession>>,
}

#[derive(Debug, Clone)]
struct ActivitySession {
    employee_id: i32,
    session_id: String,
    start_time: chrono::DateTime<Utc>,
    last_activity: chrono::DateTime<Utc>,
    idle_time: i32,
    urls: Vec<String>,
    task_name: Option<String>,
}

impl ActivityServiceImpl {
    pub fn new(db_pool: PgPool, redis_client: RedisClient) -> Self {
        Self {
            db_pool,
            redis_client,
            active_sessions: Arc::new(DashMap::new()),
        }
    }

    async fn validate_session(&self, session_token: &str) -> Result<i32, Status> {
        // Try Redis cache first for high performance
        let mut redis_conn = self.redis_client.get_async_connection().await
            .map_err(|e| {
                tracing::error!("Redis connection error: {}", e);
                Status::internal("Cache service unavailable")
            })?;

        let cache_key = format!("session:{}", session_token);
        let cached_employee_id: Option<i32> = redis_conn.get(&cache_key).await
            .map_err(|e| {
                tracing::warn!("Redis get error: {}", e);
            })
            .unwrap_or(None);

        if let Some(employee_id) = cached_employee_id {
            return Ok(employee_id);
        }

        // Fallback to database
        let row = sqlx::query!(
            "SELECT employee_id FROM user_sessions WHERE session_token = $1 AND expires_at > $2 AND is_active = true",
            session_token,
            Utc::now()
        )
        .fetch_optional(&self.db_pool)
        .await
        .map_err(|e| {
            tracing::error!("Database error during session validation: {}", e);
            Status::internal("Database error")
        })?;

        match row {
            Some(row) => {
                // Cache the result for future requests
                let _: () = redis_conn.set_ex(&cache_key, row.employee_id, 300).await
                    .map_err(|e| tracing::warn!("Redis set error: {}", e))
                    .unwrap_or(());
                
                Ok(row.employee_id)
            }
            None => Err(Status::unauthenticated("Invalid or expired session token"))
        }
    }

    async fn calculate_idle_time(&self, last_activity: chrono::DateTime<Utc>) -> i32 {
        let now = Utc::now();
        let duration = now.signed_duration_since(last_activity);
        duration.num_seconds() as i32
    }

    async fn update_session_cache(&self, session_id: &str, session: &ActivitySession) -> Result<(), Status> {
        let mut redis_conn = self.redis_client.get_async_connection().await
            .map_err(|e| {
                tracing::error!("Redis connection error: {}", e);
                Status::internal("Cache service unavailable")
            })?;

        let cache_key = format!("activity_session:{}", session_id);
        let session_data = serde_json::to_string(session)
            .map_err(|_| Status::internal("Serialization error"))?;

        let _: () = redis_conn.set_ex(&cache_key, session_data, 3600).await
            .map_err(|e| {
                tracing::warn!("Redis set error: {}", e);
            })
            .unwrap_or(());

        Ok(())
    }

    async fn get_session_from_cache(&self, session_id: &str) -> Option<ActivitySession> {
        let mut redis_conn = match self.redis_client.get_async_connection().await {
            Ok(conn) => conn,
            Err(e) => {
                tracing::warn!("Redis connection error: {}", e);
                return None;
            }
        };

        let cache_key = format!("activity_session:{}", session_id);
        let session_data: Option<String> = redis_conn.get(&cache_key).await
            .map_err(|e| tracing::warn!("Redis get error: {}", e))
            .unwrap_or(None);

        session_data.and_then(|data| serde_json::from_str(&data).ok())
    }
}

#[tonic::async_trait]
impl ActivityService for ActivityServiceImpl {
    async fn start_tracking(&self, request: Request<StartTrackingRequest>) -> Result<Response<StartTrackingResponse>, Status> {
        let req = request.into_inner();
        
        tracing::info!("Start tracking request for employee: {}", req.employee_id);

        // Validate session
        let employee_id = self.validate_session(&req.session_token).await?;
        if employee_id != req.employee_id {
            return Err(Status::permission_denied("Session does not match employee ID"));
        }

        // Check if user already has an active session
        let existing_session = self.active_sessions.iter()
            .find(|entry| entry.value().employee_id == employee_id && entry.value().session_id.len() > 0);

        if let Some(existing) = existing_session {
            return Ok(Response::new(StartTrackingResponse {
                success: false,
                message: format!("User already has an active session: {}", existing.value().session_id),
                session_id: existing.value().session_id.clone(),
                activity_log_id: 0,
            }));
        }

        // Generate new session ID
        let session_id = Uuid::new_v4().to_string();
        let start_time = Utc::now();

        // Create activity session in memory cache
        let activity_session = ActivitySession {
            employee_id,
            session_id: session_id.clone(),
            start_time,
            last_activity: start_time,
            idle_time: 0,
            urls: Vec::new(),
            task_name: req.task_name,
        };

        // Store in database
        let activity_log_id = sqlx::query!(
            "INSERT INTO activity_logs (employee_id, session_id, start_time, task_name, task_description, is_active) 
             VALUES ($1, $2, $3, $4, $5, true) RETURNING id",
            employee_id,
            session_id,
            start_time,
            req.task_name,
            req.task_description
        )
        .fetch_one(&self.db_pool)
        .await
        .map_err(|e| {
            tracing::error!("Database error during activity start: {}", e);
            Status::internal("Failed to start activity tracking")
        })?
        .id;

        // Store in memory cache
        self.active_sessions.insert(session_id.clone(), activity_session.clone());

        // Cache in Redis
        self.update_session_cache(&session_id, &activity_session).await?;

        tracing::info!("Activity tracking started for employee {} with session {}", employee_id, session_id);

        Ok(Response::new(StartTrackingResponse {
            success: true,
            message: "Activity tracking started successfully".to_string(),
            session_id,
            activity_log_id: activity_log_id as i64,
        }))
    }

    async fn stop_tracking(&self, request: Request<StopTrackingRequest>) -> Result<Response<StopTrackingResponse>, Status> {
        let req = request.into_inner();
        
        tracing::info!("Stop tracking request for session: {}", req.session_id);

        // Validate session
        let employee_id = self.validate_session(&req.session_token).await?;
        if employee_id != req.employee_id {
            return Err(Status::permission_denied("Session does not match employee ID"));
        }

        // Get session from cache or database
        let session = if let Some(cached_session) = self.get_session_from_cache(&req.session_id).await {
            cached_session
        } else if let Some(memory_session) = self.active_sessions.get(&req.session_id) {
            memory_session.value().clone()
        } else {
            return Err(Status::not_found("Activity session not found"));
        };

        let end_time = Utc::now();
        let total_time = end_time.signed_duration_since(session.start_time).num_seconds() as i64;
        let final_idle_time = self.calculate_idle_time(session.last_activity).await;
        let active_time = total_time - final_idle_time as i64;

        // Update database
        sqlx::query!(
            "UPDATE activity_logs SET end_time = $1, idle_time = $2, active_time = $3, urls = $4, is_active = false 
             WHERE session_id = $5 AND employee_id = $6",
            end_time,
            final_idle_time,
            active_time,
            &session.urls,
            req.session_id,
            employee_id
        )
        .execute(&self.db_pool)
        .await
        .map_err(|e| {
            tracing::error!("Database error during activity stop: {}", e);
            Status::internal("Failed to stop activity tracking")
        })?;

        // Remove from memory cache
        self.active_sessions.remove(&req.session_id);

        // Remove from Redis cache
        let mut redis_conn = self.redis_client.get_async_connection().await
            .map_err(|e| {
                tracing::warn!("Redis connection error: {}", e);
            })
            .unwrap_or_default();

        let cache_key = format!("activity_session:{}", req.session_id);
        let _: () = redis_conn.del(&cache_key).await
            .map_err(|e| tracing::warn!("Redis del error: {}", e))
            .unwrap_or(());

        tracing::info!("Activity tracking stopped for session {} - Total: {}s, Idle: {}s", 
                      req.session_id, total_time, final_idle_time);

        Ok(Response::new(StopTrackingResponse {
            success: true,
            message: "Activity tracking stopped successfully".to_string(),
            total_time_seconds: total_time,
            idle_time_seconds: final_idle_time as i64,
        }))
    }

    async fn update_activity(&self, request: Request<UpdateActivityRequest>) -> Result<Response<UpdateActivityResponse>, Status> {
        let req = request.into_inner();
        
        // Validate session
        let employee_id = self.validate_session(&req.session_token).await?;
        if employee_id != req.employee_id {
            return Err(Status::permission_denied("Session does not match employee ID"));
        }

        // Update session in memory cache
        if let Some(mut session) = self.active_sessions.get_mut(&req.session_id) {
            session.last_activity = Utc::now();
            session.idle_time = req.idle_time_seconds;
            session.urls = req.urls;

            // Update Redis cache
            self.update_session_cache(&req.session_id, &session).await?;

            tracing::debug!("Activity updated for session {} - Idle: {}s, URLs: {}", 
                           req.session_id, req.idle_time_seconds, req.urls.len());
        }

        Ok(Response::new(UpdateActivityResponse {
            success: true,
            message: "Activity updated successfully".to_string(),
        }))
    }

    async fn get_activity_logs(&self, request: Request<GetActivityLogsRequest>) -> Result<Response<GetActivityLogsResponse>, Status> {
        let req = request.into_inner();
        
        // Validate session
        let employee_id = self.validate_session(&req.session_token).await?;
        if employee_id != req.employee_id {
            return Err(Status::permission_denied("Session does not match employee ID"));
        }

        let limit = req.limit.unwrap_or(50).min(100) as i64;
        let offset = req.offset.unwrap_or(0) as i64;

        // Parse date filters
        let start_date = req.start_date.and_then(|d| chrono::DateTime::parse_from_rfc3339(&d).ok());
        let end_date = req.end_date.and_then(|d| chrono::DateTime::parse_from_rfc3339(&d).ok());

        let mut query = "SELECT id, employee_id, session_id, start_time, end_time, idle_time, active_time, 
                        task_name, task_description, urls, is_active FROM activity_logs WHERE employee_id = $1".to_string();
        let mut param_count = 1;

        if let Some(start) = start_date {
            param_count += 1;
            query.push_str(&format!(" AND start_time >= ${}", param_count));
        }
        if let Some(end) = end_date {
            param_count += 1;
            query.push_str(&format!(" AND start_time <= ${}", param_count));
        }

        query.push_str(&format!(" ORDER BY start_time DESC LIMIT ${} OFFSET ${}", param_count + 1, param_count + 2));

        let mut db_query = sqlx::query(&query).bind(employee_id);
        
        if let Some(start) = start_date {
            db_query = db_query.bind(start);
        }
        if let Some(end) = end_date {
            db_query = db_query.bind(end);
        }
        
        db_query = db_query.bind(limit).bind(offset);

        let rows = db_query.fetch_all(&self.db_pool).await
            .map_err(|e| {
                tracing::error!("Database error during activity logs fetch: {}", e);
                Status::internal("Failed to fetch activity logs")
            })?;

        let logs: Vec<ActivityLog> = rows.into_iter().map(|row| {
            ActivityLog {
                id: row.get::<i64, _>("id"),
                employee_id: row.get::<i32, _>("employee_id"),
                session_id: row.get::<String, _>("session_id"),
                start_time: row.get::<chrono::DateTime<Utc>, _>("start_time").to_rfc3339(),
                end_time: row.get::<Option<chrono::DateTime<Utc>>, _>("end_time").map(|t| t.to_rfc3339()),
                idle_time: row.get::<i32, _>("idle_time"),
                active_time: row.get::<i32, _>("active_time"),
                task_name: row.get::<Option<String>, _>("task_name"),
                task_description: row.get::<Option<String>, _>("task_description"),
                urls: row.get::<Vec<String>, _>("urls"),
                is_active: row.get::<bool, _>("is_active"),
            }
        }).collect();

        // Get total count
        let count_query = "SELECT COUNT(*) FROM activity_logs WHERE employee_id = $1";
        let total_count = sqlx::query_scalar::<_, i64>(count_query)
            .bind(employee_id)
            .fetch_one(&self.db_pool)
            .await
            .map_err(|e| {
                tracing::error!("Database error during count: {}", e);
                Status::internal("Failed to get total count")
            })?;

        Ok(Response::new(GetActivityLogsResponse {
            success: true,
            message: "Activity logs retrieved successfully".to_string(),
            logs,
            total_count: total_count as i32,
        }))
    }

    async fn get_current_activity(&self, request: Request<GetCurrentActivityRequest>) -> Result<Response<GetCurrentActivityResponse>, Status> {
        let req = request.into_inner();
        
        // Validate session
        let employee_id = self.validate_session(&req.session_token).await?;
        if employee_id != req.employee_id {
            return Err(Status::permission_denied("Session does not match employee ID"));
        }

        // Find active session
        let active_session = self.active_sessions.iter()
            .find(|entry| entry.value().employee_id == employee_id)
            .map(|entry| entry.value().clone());

        if let Some(session) = active_session {
            let activity_log = ActivityLog {
                id: 0, // Will be filled from database if needed
                employee_id: session.employee_id,
                session_id: session.session_id,
                start_time: session.start_time.to_rfc3339(),
                end_time: None,
                idle_time: session.idle_time,
                active_time: Utc::now().signed_duration_since(session.start_time).num_seconds() as i32 - session.idle_time,
                task_name: session.task_name,
                task_description: None,
                urls: session.urls,
                is_active: true,
            };

            Ok(Response::new(GetCurrentActivityResponse {
                success: true,
                message: "Current activity retrieved successfully".to_string(),
                current_activity: Some(activity_log),
            }))
        } else {
            Ok(Response::new(GetCurrentActivityResponse {
                success: true,
                message: "No active activity found".to_string(),
                current_activity: None,
            }))
        }
    }

    async fn assign_task(&self, request: Request<AssignTaskRequest>) -> Result<Response<AssignTaskResponse>, Status> {
        let req = request.into_inner();
        
        // Validate session
        let employee_id = self.validate_session(&req.session_token).await?;
        if employee_id != req.employee_id {
            return Err(Status::permission_denied("Session does not match employee ID"));
        }

        // Update session in memory cache
        if let Some(mut session) = self.active_sessions.get_mut(&req.session_id) {
            session.task_name = Some(req.task_name.clone());
            session.urls = req.urls;

            // Update Redis cache
            self.update_session_cache(&req.session_id, &session).await?;
        }

        // Update database
        sqlx::query!(
            "UPDATE activity_logs SET task_name = $1, task_description = $2, urls = $3 
             WHERE session_id = $4 AND employee_id = $5",
            req.task_name,
            req.task_description,
            &req.urls,
            req.session_id,
            employee_id
        )
        .execute(&self.db_pool)
        .await
        .map_err(|e| {
            tracing::error!("Database error during task assignment: {}", e);
            Status::internal("Failed to assign task")
        })?;

        Ok(Response::new(AssignTaskResponse {
            success: true,
            message: "Task assigned successfully".to_string(),
        }))
    }

    async fn get_tasks(&self, request: Request<GetTasksRequest>) -> Result<Response<GetTasksResponse>, Status> {
        let req = request.into_inner();
        
        // Validate session
        let employee_id = self.validate_session(&req.session_token).await?;
        if employee_id != req.employee_id {
            return Err(Status::permission_denied("Session does not match employee ID"));
        }

        let rows = sqlx::query!(
            "SELECT id, employee_id, name, description, project_name, is_active, created_at 
             FROM tasks WHERE employee_id = $1 AND is_active = true ORDER BY created_at DESC",
            employee_id
        )
        .fetch_all(&self.db_pool)
        .await
        .map_err(|e| {
            tracing::error!("Database error during tasks fetch: {}", e);
            Status::internal("Failed to fetch tasks")
        })?;

        let tasks: Vec<Task> = rows.into_iter().map(|row| {
            Task {
                id: row.id as i64,
                employee_id: row.employee_id,
                name: row.name,
                description: row.description,
                project_name: row.project_name,
                is_active: row.is_active,
                created_at: row.created_at.to_rfc3339(),
            }
        }).collect();

        Ok(Response::new(GetTasksResponse {
            success: true,
            message: "Tasks retrieved successfully".to_string(),
            tasks,
        }))
    }

    async fn create_task(&self, request: Request<CreateTaskRequest>) -> Result<Response<CreateTaskResponse>, Status> {
        let req = request.into_inner();
        
        // Validate session
        let employee_id = self.validate_session(&req.session_token).await?;
        if employee_id != req.employee_id {
            return Err(Status::permission_denied("Session does not match employee ID"));
        }

        let task_id = sqlx::query!(
            "INSERT INTO tasks (employee_id, name, description, project_name) VALUES ($1, $2, $3, $4) RETURNING id",
            employee_id,
            req.name,
            req.description,
            req.project_name
        )
        .fetch_one(&self.db_pool)
        .await
        .map_err(|e| {
            tracing::error!("Database error during task creation: {}", e);
            Status::internal("Failed to create task")
        })?
        .id;

        let task = Task {
            id: task_id as i64,
            employee_id,
            name: req.name,
            description: req.description,
            project_name: req.project_name,
            is_active: true,
            created_at: Utc::now().to_rfc3339(),
        };

        Ok(Response::new(CreateTaskResponse {
            success: true,
            message: "Task created successfully".to_string(),
            task,
        }))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Load environment variables
    dotenv::dotenv().ok();

    let database_url = env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://localhost/time_tracker".to_string());
    
    let redis_url = env::var("REDIS_URL")
        .unwrap_or_else(|_| "redis://localhost:6379".to_string());

    let port = env::var("ACTIVITY_SERVICE_PORT")
        .unwrap_or_else(|_| "50052".to_string())
        .parse::<u16>()
        .unwrap_or(50052);

    // Create database connection pool with optimized settings for high concurrency
    let db_pool = PgPool::builder()
        .max_connections(100)
        .min_connections(10)
        .acquire_timeout(std::time::Duration::from_secs(30))
        .build(&database_url)
        .await?;

    // Create Redis client
    let redis_client = RedisClient::open(redis_url)?;

    let activity_service = ActivityServiceImpl::new(db_pool, redis_client);
    let activity_service = ActivityServiceServer::new(activity_service);

    let addr = format!("0.0.0.0:{}", port).parse()?;
    
    tracing::info!("Activity service starting on {} with high-performance optimizations", addr);

    Server::builder()
        .add_service(activity_service)
        .serve(addr)
        .await?;

    Ok(())
}
