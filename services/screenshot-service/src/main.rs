use tonic::{transport::Server, Request, Response, Status};
use screenshot_service::screenshot_service_server::{ScreenshotService, ScreenshotServiceServer};
use screenshot_service::{
    CaptureScreenshotRequest, CaptureScreenshotResponse, GetScreenshotsRequest, GetScreenshotsResponse,
    DeleteScreenshotRequest, DeleteScreenshotResponse, GetScreenshotRequest, GetScreenshotResponse,
    ScreenshotInfo, ScreenshotData
};
use sqlx::{PgPool, Row};
use uuid::Uuid;
use chrono::Utc;
use std::env;
use std::path::Path;
use std::fs;
use image::{ImageFormat, DynamicImage};
use redis::Client as RedisClient;
use redis::AsyncCommands;
use base64::{Engine as _, engine::general_purpose};

pub mod screenshot_service {
    tonic::include_proto!("screenshot");
}

#[derive(Debug)]
pub struct ScreenshotServiceImpl {
    db_pool: PgPool,
    redis_client: RedisClient,
    storage_path: String,
    compression_quality: u8,
}

impl ScreenshotServiceImpl {
    pub fn new(db_pool: PgPool, redis_client: RedisClient, storage_path: String, compression_quality: u8) -> Self {
        Self {
            db_pool,
            redis_client,
            storage_path,
            compression_quality,
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

    async fn compress_image(&self, image_data: &[u8]) -> Result<Vec<u8>, Status> {
        // Decode the image
        let img = image::load_from_memory(image_data)
            .map_err(|e| {
                tracing::error!("Failed to decode image: {}", e);
                Status::invalid_argument("Invalid image data")
            })?;

        // Resize image for better performance (max 1920x1080)
        let img = if img.width() > 1920 || img.height() > 1080 {
            let (width, height) = img.dimensions();
            let scale = (1920.0 / width as f32).min(1080.0 / height as f32);
            let new_width = (width as f32 * scale) as u32;
            let new_height = (height as f32 * scale) as u32;
            
            img.resize(new_width, new_height, image::imageops::FilterType::Lanczos3)
        } else {
            img
        };

        // Convert to RGB8 for JPEG compression
        let rgb_img = img.to_rgb8();

        // Compress to JPEG
        let mut compressed_data = Vec::new();
        {
            let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut compressed_data, self.compression_quality);
            encoder.encode_image(&rgb_img)
                .map_err(|e| {
                    tracing::error!("Failed to compress image: {}", e);
                    Status::internal("Failed to compress image")
                })?;
        }

        Ok(compressed_data)
    }

    async fn save_screenshot_to_disk(&self, screenshot_id: i64, compressed_data: &[u8]) -> Result<String, Status> {
        // Create directory structure: storage_path/year/month/day/
        let now = Utc::now();
        let year = now.format("%Y").to_string();
        let month = now.format("%m").to_string();
        let day = now.format("%d").to_string();
        
        let dir_path = Path::new(&self.storage_path)
            .join(&year)
            .join(&month)
            .join(&day);

        // Create directory if it doesn't exist
        fs::create_dir_all(&dir_path)
            .map_err(|e| {
                tracing::error!("Failed to create directory: {}", e);
                Status::internal("Failed to create storage directory")
            })?;

        // Generate filename
        let filename = format!("screenshot_{}.jpg", screenshot_id);
        let file_path = dir_path.join(&filename);
        let relative_path = format!("{}/{}/{}/{}", year, month, day, filename);

        // Save file
        fs::write(&file_path, compressed_data)
            .map_err(|e| {
                tracing::error!("Failed to save screenshot: {}", e);
                Status::internal("Failed to save screenshot")
            })?;

        Ok(relative_path)
    }

    async fn get_screenshot_from_disk(&self, file_path: &str) -> Result<Vec<u8>, Status> {
        let full_path = Path::new(&self.storage_path).join(file_path);
        
        fs::read(&full_path)
            .map_err(|e| {
                tracing::error!("Failed to read screenshot file: {}", e);
                Status::not_found("Screenshot file not found")
            })
    }

    async fn cache_screenshot(&self, screenshot_id: i64, screenshot_data: &[u8]) -> Result<(), Status> {
        let mut redis_conn = self.redis_client.get_async_connection().await
            .map_err(|e| {
                tracing::warn!("Redis connection error: {}", e);
            })
            .unwrap_or_default();

        let cache_key = format!("screenshot:{}", screenshot_id);
        let _: () = redis_conn.set_ex(&cache_key, screenshot_data, 3600).await
            .map_err(|e| tracing::warn!("Redis set error: {}", e))
            .unwrap_or(());

        Ok(())
    }

    async fn get_screenshot_from_cache(&self, screenshot_id: i64) -> Option<Vec<u8>> {
        let mut redis_conn = match self.redis_client.get_async_connection().await {
            Ok(conn) => conn,
            Err(e) => {
                tracing::warn!("Redis connection error: {}", e);
                return None;
            }
        };

        let cache_key = format!("screenshot:{}", screenshot_id);
        redis_conn.get(&cache_key).await
            .map_err(|e| tracing::warn!("Redis get error: {}", e))
            .unwrap_or(None)
    }
}

#[tonic::async_trait]
impl ScreenshotService for ScreenshotServiceImpl {
    async fn capture_screenshot(&self, request: Request<CaptureScreenshotRequest>) -> Result<Response<CaptureScreenshotResponse>, Status> {
        let req = request.into_inner();
        
        tracing::info!("Screenshot capture request for employee: {}", req.employee_id);

        // Validate session
        let employee_id = self.validate_session(&req.session_token).await?;
        if employee_id != req.employee_id {
            return Err(Status::permission_denied("Session does not match employee ID"));
        }

        // Decode base64 image data
        let image_data = general_purpose::STANDARD.decode(&req.screenshot_data)
            .map_err(|e| {
                tracing::error!("Failed to decode base64 image data: {}", e);
                Status::invalid_argument("Invalid base64 image data")
            })?;

        // Compress image
        let compressed_data = self.compress_image(&image_data).await?;
        let file_size = compressed_data.len() as i32;

        // Get activity log ID if session_id is provided
        let activity_log_id = if let Some(session_id) = req.session_id {
            let row = sqlx::query!(
                "SELECT id FROM activity_logs WHERE session_id = $1 AND employee_id = $2",
                session_id,
                employee_id
            )
            .fetch_optional(&self.db_pool)
            .await
            .map_err(|e| {
                tracing::error!("Database error during activity log lookup: {}", e);
                Status::internal("Database error")
            })?;

            row.map(|r| r.id as i64)
        } else {
            None
        };

        // Insert screenshot record into database
        let screenshot_id = sqlx::query!(
            "INSERT INTO screenshots (employee_id, activity_log_id, timestamp, screenshot_data, file_size, compression_type) 
             VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
            employee_id,
            activity_log_id,
            Utc::now(),
            &compressed_data,
            file_size,
            req.compression_type.unwrap_or_else(|| "jpeg".to_string())
        )
        .fetch_one(&self.db_pool)
        .await
        .map_err(|e| {
            tracing::error!("Database error during screenshot insert: {}", e);
            Status::internal("Failed to save screenshot")
        })?
        .id;

        // Save to disk
        let file_path = self.save_screenshot_to_disk(screenshot_id as i64, &compressed_data).await?;

        // Update database with file path
        sqlx::query!(
            "UPDATE screenshots SET file_path = $1 WHERE id = $2",
            file_path,
            screenshot_id
        )
        .execute(&self.db_pool)
        .await
        .map_err(|e| {
            tracing::error!("Database error during file path update: {}", e);
            Status::internal("Failed to update file path")
        })?;

        // Cache screenshot for quick access
        self.cache_screenshot(screenshot_id as i64, &compressed_data).await?;

        tracing::info!("Screenshot captured successfully - ID: {}, Size: {} bytes", screenshot_id, file_size);

        Ok(Response::new(CaptureScreenshotResponse {
            success: true,
            message: "Screenshot captured successfully".to_string(),
            screenshot_id: screenshot_id as i64,
            file_path,
        }))
    }

    async fn get_screenshots(&self, request: Request<GetScreenshotsRequest>) -> Result<Response<GetScreenshotsResponse>, Status> {
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

        let mut query = "SELECT id, employee_id, activity_log_id, timestamp, file_path, file_size, compression_type 
                        FROM screenshots WHERE employee_id = $1".to_string();
        let mut param_count = 1;

        if let Some(session_id) = req.session_id {
            param_count += 1;
            query.push_str(&format!(" AND activity_log_id IN (SELECT id FROM activity_logs WHERE session_id = ${})", param_count));
        }
        if let Some(start) = start_date {
            param_count += 1;
            query.push_str(&format!(" AND timestamp >= ${}", param_count));
        }
        if let Some(end) = end_date {
            param_count += 1;
            query.push_str(&format!(" AND timestamp <= ${}", param_count));
        }

        query.push_str(&format!(" ORDER BY timestamp DESC LIMIT ${} OFFSET ${}", param_count + 1, param_count + 2));

        let mut db_query = sqlx::query(&query).bind(employee_id);
        
        if let Some(session_id) = req.session_id {
            db_query = db_query.bind(session_id);
        }
        if let Some(start) = start_date {
            db_query = db_query.bind(start);
        }
        if let Some(end) = end_date {
            db_query = db_query.bind(end);
        }
        
        db_query = db_query.bind(limit).bind(offset);

        let rows = db_query.fetch_all(&self.db_pool).await
            .map_err(|e| {
                tracing::error!("Database error during screenshots fetch: {}", e);
                Status::internal("Failed to fetch screenshots")
            })?;

        let screenshots: Vec<ScreenshotInfo> = rows.into_iter().map(|row| {
            ScreenshotInfo {
                id: row.get::<i64, _>("id"),
                employee_id: row.get::<i32, _>("employee_id"),
                activity_log_id: row.get::<Option<i64>, _>("activity_log_id"),
                timestamp: row.get::<chrono::DateTime<Utc>, _>("timestamp").to_rfc3339(),
                file_path: row.get::<Option<String>, _>("file_path").unwrap_or_default(),
                file_size: row.get::<i32, _>("file_size"),
                compression_type: row.get::<String, _>("compression_type"),
            }
        }).collect();

        // Get total count
        let count_query = "SELECT COUNT(*) FROM screenshots WHERE employee_id = $1";
        let total_count = sqlx::query_scalar::<_, i64>(count_query)
            .bind(employee_id)
            .fetch_one(&self.db_pool)
            .await
            .map_err(|e| {
                tracing::error!("Database error during count: {}", e);
                Status::internal("Failed to get total count")
            })?;

        Ok(Response::new(GetScreenshotsResponse {
            success: true,
            message: "Screenshots retrieved successfully".to_string(),
            screenshots,
            total_count: total_count as i32,
        }))
    }

    async fn delete_screenshot(&self, request: Request<DeleteScreenshotRequest>) -> Result<Response<DeleteScreenshotResponse>, Status> {
        let req = request.into_inner();
        
        // Validate session
        let employee_id = self.validate_session(&req.session_token).await?;
        if employee_id != req.employee_id {
            return Err(Status::permission_denied("Session does not match employee ID"));
        }

        // Get screenshot info before deletion
        let screenshot_row = sqlx::query!(
            "SELECT file_path FROM screenshots WHERE id = $1 AND employee_id = $2",
            req.screenshot_id,
            employee_id
        )
        .fetch_optional(&self.db_pool)
        .await
        .map_err(|e| {
            tracing::error!("Database error during screenshot lookup: {}", e);
            Status::internal("Database error")
        })?;

        match screenshot_row {
            Some(row) => {
                // Delete from database
                sqlx::query!(
                    "DELETE FROM screenshots WHERE id = $1 AND employee_id = $2",
                    req.screenshot_id,
                    employee_id
                )
                .execute(&self.db_pool)
                .await
                .map_err(|e| {
                    tracing::error!("Database error during screenshot deletion: {}", e);
                    Status::internal("Failed to delete screenshot")
                })?;

                // Delete file from disk
                if let Some(file_path) = row.file_path {
                    let full_path = Path::new(&self.storage_path).join(&file_path);
                    if let Err(e) = fs::remove_file(&full_path) {
                        tracing::warn!("Failed to delete screenshot file: {}", e);
                    }
                }

                // Remove from cache
                let mut redis_conn = self.redis_client.get_async_connection().await
                    .map_err(|e| {
                        tracing::warn!("Redis connection error: {}", e);
                    })
                    .unwrap_or_default();

                let cache_key = format!("screenshot:{}", req.screenshot_id);
                let _: () = redis_conn.del(&cache_key).await
                    .map_err(|e| tracing::warn!("Redis del error: {}", e))
                    .unwrap_or(());

                tracing::info!("Screenshot deleted successfully - ID: {}", req.screenshot_id);

                Ok(Response::new(DeleteScreenshotResponse {
                    success: true,
                    message: "Screenshot deleted successfully".to_string(),
                }))
            }
            None => Ok(Response::new(DeleteScreenshotResponse {
                success: false,
                message: "Screenshot not found".to_string(),
            }))
        }
    }

    async fn get_screenshot(&self, request: Request<GetScreenshotRequest>) -> Result<Response<GetScreenshotResponse>, Status> {
        let req = request.into_inner();
        
        // Validate session
        let employee_id = self.validate_session(&req.session_token).await?;
        if employee_id != req.employee_id {
            return Err(Status::permission_denied("Session does not match employee ID"));
        }

        // Try cache first
        if let Some(cached_data) = self.get_screenshot_from_cache(req.screenshot_id).await {
            let screenshot_data = ScreenshotData {
                id: req.screenshot_id,
                employee_id,
                activity_log_id: None,
                timestamp: Utc::now().to_rfc3339(),
                screenshot_data: cached_data,
                file_path: String::new(),
                file_size: 0,
                compression_type: "jpeg".to_string(),
            };

            return Ok(Response::new(GetScreenshotResponse {
                success: true,
                message: "Screenshot retrieved from cache".to_string(),
                screenshot: Some(screenshot_data),
            }));
        }

        // Get from database
        let screenshot_row = sqlx::query!(
            "SELECT id, employee_id, activity_log_id, timestamp, screenshot_data, file_path, file_size, compression_type 
             FROM screenshots WHERE id = $1 AND employee_id = $2",
            req.screenshot_id,
            employee_id
        )
        .fetch_optional(&self.db_pool)
        .await
        .map_err(|e| {
            tracing::error!("Database error during screenshot fetch: {}", e);
            Status::internal("Database error")
        })?;

        match screenshot_row {
            Some(row) => {
                let screenshot_data = ScreenshotData {
                    id: row.id as i64,
                    employee_id: row.employee_id,
                    activity_log_id: row.activity_log_id.map(|id| id as i64),
                    timestamp: row.timestamp.to_rfc3339(),
                    screenshot_data: row.screenshot_data,
                    file_path: row.file_path.unwrap_or_default(),
                    file_size: row.file_size,
                    compression_type: row.compression_type,
                };

                // Cache for future requests
                self.cache_screenshot(req.screenshot_id, &screenshot_data.screenshot_data).await?;

                Ok(Response::new(GetScreenshotResponse {
                    success: true,
                    message: "Screenshot retrieved successfully".to_string(),
                    screenshot: Some(screenshot_data),
                }))
            }
            None => Ok(Response::new(GetScreenshotResponse {
                success: false,
                message: "Screenshot not found".to_string(),
                screenshot: None,
            }))
        }
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

    let storage_path = env::var("SCREENSHOT_STORAGE_PATH")
        .unwrap_or_else(|_| "/tmp/screenshots".to_string());

    let compression_quality = env::var("SCREENSHOT_COMPRESSION_QUALITY")
        .unwrap_or_else(|_| "80".to_string())
        .parse::<u8>()
        .unwrap_or(80);

    let port = env::var("SCREENSHOT_SERVICE_PORT")
        .unwrap_or_else(|_| "50053".to_string())
        .parse::<u16>()
        .unwrap_or(50053);

    // Create storage directory
    fs::create_dir_all(&storage_path)?;

    // Create database connection pool with optimized settings for high concurrency
    let db_pool = PgPool::builder()
        .max_connections(100)
        .min_connections(10)
        .acquire_timeout(std::time::Duration::from_secs(30))
        .build(&database_url)
        .await?;

    // Create Redis client
    let redis_client = RedisClient::open(redis_url)?;

    let screenshot_service = ScreenshotServiceImpl::new(db_pool, redis_client, storage_path, compression_quality);
    let screenshot_service = ScreenshotServiceServer::new(screenshot_service);

    let addr = format!("0.0.0.0:{}", port).parse()?;
    
    tracing::info!("Screenshot service starting on {} with high-performance optimizations", addr);

    Server::builder()
        .add_service(screenshot_service)
        .serve(addr)
        .await?;

    Ok(())
}
