use tonic::{transport::Server, Request, Response, Status, Code};
use std::env;
use std::sync::Arc;
use dashmap::DashMap;
use redis::Client as RedisClient;
use redis::AsyncCommands;
use chrono::{Utc, Duration};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{info, warn, error};

// Rate limiting structures
#[derive(Debug, Clone)]
struct RateLimitInfo {
    requests: u32,
    window_start: u64,
}

#[derive(Debug)]
pub struct ApiGateway {
    auth_service_url: String,
    activity_service_url: String,
    screenshot_service_url: String,
    redis_client: RedisClient,
    rate_limits: Arc<DashMap<String, RateLimitInfo>>,
    max_requests_per_minute: u32,
}

impl ApiGateway {
    pub fn new(
        auth_service_url: String,
        activity_service_url: String,
        screenshot_service_url: String,
        redis_client: RedisClient,
        max_requests_per_minute: u32,
    ) -> Self {
        Self {
            auth_service_url,
            activity_service_url,
            screenshot_service_url,
            redis_client,
            rate_limits: Arc::new(DashMap::new()),
            max_requests_per_minute,
        }
    }

    async fn check_rate_limit(&self, client_ip: &str) -> Result<(), Status> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let window_start = now - (now % 60); // 1-minute windows
        
        // Try Redis first for distributed rate limiting
        let mut redis_conn = self.redis_client.get_async_connection().await
            .map_err(|e| {
                warn!("Redis connection error: {}", e);
            })
            .unwrap_or_default();

        let redis_key = format!("rate_limit:{}:{}", client_ip, window_start);
        let redis_count: Option<u32> = redis_conn.get(&redis_key).await
            .map_err(|e| warn!("Redis get error: {}", e))
            .unwrap_or(None);

        if let Some(count) = redis_count {
            if count >= self.max_requests_per_minute {
                return Err(Status::new(Code::ResourceExhausted, "Rate limit exceeded"));
            }
            
            // Increment counter
            let _: () = redis_conn.incr(&redis_key, 1).await
                .map_err(|e| warn!("Redis incr error: {}", e))
                .unwrap_or(());
            
            // Set expiration
            let _: () = redis_conn.expire(&redis_key, 60).await
                .map_err(|e| warn!("Redis expire error: {}", e))
                .unwrap_or(());
        } else {
            // Initialize counter
            let _: () = redis_conn.set_ex(&redis_key, 1, 60).await
                .map_err(|e| warn!("Redis set error: {}", e))
                .unwrap_or(());
        }

        // Fallback to local rate limiting
        let mut rate_limit = self.rate_limits.get_mut(client_ip).unwrap_or_else(|| {
            self.rate_limits.insert(client_ip.to_string(), RateLimitInfo {
                requests: 0,
                window_start,
            });
            self.rate_limits.get_mut(client_ip).unwrap()
        });

        if rate_limit.window_start != window_start {
            rate_limit.window_start = window_start;
            rate_limit.requests = 0;
        }

        rate_limit.requests += 1;
        
        if rate_limit.requests > self.max_requests_per_minute {
            return Err(Status::new(Code::ResourceExhausted, "Rate limit exceeded"));
        }

        Ok(())
    }

    async fn validate_session(&self, session_token: &str) -> Result<i32, Status> {
        // Try Redis cache first
        let mut redis_conn = self.redis_client.get_async_connection().await
            .map_err(|e| {
                error!("Redis connection error: {}", e);
                Status::internal("Cache service unavailable")
            })?;

        let cache_key = format!("session:{}", session_token);
        let cached_employee_id: Option<i32> = redis_conn.get(&cache_key).await
            .map_err(|e| {
                warn!("Redis get error: {}", e);
            })
            .unwrap_or(None);

        if let Some(employee_id) = cached_employee_id {
            return Ok(employee_id);
        }

        // Call auth service for validation
        let auth_client = reqwest::Client::new();
        let auth_url = format!("{}/auth.AuthService/ValidateToken", self.auth_service_url);
        
        let request_body = serde_json::json!({
            "session_token": session_token
        });

        let response = auth_client
            .post(&auth_url)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                error!("Failed to call auth service: {}", e);
                Status::internal("Authentication service unavailable")
            })?;

        if !response.status().is_success() {
            return Err(Status::unauthenticated("Invalid session token"));
        }

        let response_body: serde_json::Value = response.json().await
            .map_err(|e| {
                error!("Failed to parse auth response: {}", e);
                Status::internal("Authentication service error")
            })?;

        let valid = response_body.get("valid")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if !valid {
            return Err(Status::unauthenticated("Invalid session token"));
        }

        let employee_id = response_body.get("employee_id")
            .and_then(|id| id.as_i64())
            .unwrap_or(0) as i32;

        // Cache the result
        let _: () = redis_conn.set_ex(&cache_key, employee_id, 300).await
            .map_err(|e| warn!("Redis set error: {}", e))
            .unwrap_or(());

        Ok(employee_id)
    }

    async fn proxy_to_service(&self, service_url: &str, path: &str, body: &[u8]) -> Result<Vec<u8>, Status> {
        let client = reqwest::Client::new();
        let url = format!("{}/{}", service_url, path);
        
        let response = client
            .post(&url)
            .header("Content-Type", "application/json")
            .body(body.to_vec())
            .send()
            .await
            .map_err(|e| {
                error!("Failed to call service: {}", e);
                Status::internal("Service unavailable")
            })?;

        let response_body = response.bytes().await
            .map_err(|e| {
                error!("Failed to read response: {}", e);
                Status::internal("Service error")
            })?;

        Ok(response_body.to_vec())
    }
}

// HTTP server implementation
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request as HyperRequest, Response as HyperResponse, Server, StatusCode};
use std::convert::Infallible;
use std::net::SocketAddr;

async fn handle_request(
    req: HyperRequest<Body>,
    gateway: Arc<ApiGateway>,
) -> Result<HyperResponse<Body>, Infallible> {
    let client_ip = req.headers()
        .get("x-forwarded-for")
        .or_else(|| req.headers().get("x-real-ip"))
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");

    // Check rate limit
    if let Err(_) = gateway.check_rate_limit(client_ip).await {
        let mut response = HyperResponse::new(Body::from("Rate limit exceeded"));
        *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
        return Ok(response);
    }

    let (parts, body) = req.into_parts();
    let body_bytes = hyper::body::to_bytes(body).await.unwrap_or_default();

    // Route requests based on path
    let response = match parts.uri.path() {
        path if path.starts_with("/auth/") => {
            let service_path = path.strip_prefix("/auth/").unwrap_or(path);
            gateway.proxy_to_service(&gateway.auth_service_url, service_path, &body_bytes).await
        }
        path if path.starts_with("/activity/") => {
            // Validate session for activity endpoints
            if let Ok(session_token) = extract_session_token(&parts.headers) {
                if let Err(_) = gateway.validate_session(&session_token).await {
                    return Ok(create_error_response(StatusCode::UNAUTHORIZED, "Invalid session"));
                }
            }
            
            let service_path = path.strip_prefix("/activity/").unwrap_or(path);
            gateway.proxy_to_service(&gateway.activity_service_url, service_path, &body_bytes).await
        }
        path if path.starts_with("/screenshot/") => {
            // Validate session for screenshot endpoints
            if let Ok(session_token) = extract_session_token(&parts.headers) {
                if let Err(_) = gateway.validate_session(&session_token).await {
                    return Ok(create_error_response(StatusCode::UNAUTHORIZED, "Invalid session"));
                }
            }
            
            let service_path = path.strip_prefix("/screenshot/").unwrap_or(path);
            gateway.proxy_to_service(&gateway.screenshot_service_url, service_path, &body_bytes).await
        }
        "/health" => {
            return Ok(create_health_response());
        }
        _ => {
            return Ok(create_error_response(StatusCode::NOT_FOUND, "Not found"));
        }
    };

    match response {
        Ok(body) => {
            let mut response = HyperResponse::new(Body::from(body));
            response.headers_mut().insert("Content-Type", "application/json".parse().unwrap());
            Ok(response)
        }
        Err(_) => {
            Ok(create_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error"))
        }
    }
}

fn extract_session_token(headers: &hyper::HeaderMap) -> Result<String, Status> {
    headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.to_string())
        .ok_or_else(|| Status::unauthenticated("Missing authorization header"))
}

fn create_error_response(status: StatusCode, message: &str) -> HyperResponse<Body> {
    let error_json = serde_json::json!({
        "success": false,
        "message": message
    });
    
    let mut response = HyperResponse::new(Body::from(error_json.to_string()));
    *response.status_mut() = status;
    response.headers_mut().insert("Content-Type", "application/json".parse().unwrap());
    response
}

fn create_health_response() -> HyperResponse<Body> {
    let health_json = serde_json::json!({
        "status": "healthy",
        "timestamp": Utc::now().to_rfc3339(),
        "services": {
            "auth": "active",
            "activity": "active", 
            "screenshot": "active"
        }
    });
    
    let mut response = HyperResponse::new(Body::from(health_json.to_string()));
    response.headers_mut().insert("Content-Type", "application/json".parse().unwrap());
    response
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Load environment variables
    dotenv::dotenv().ok();

    let auth_service_url = env::var("AUTH_SERVICE_URL")
        .unwrap_or_else(|_| "http://localhost:50051".to_string());
    
    let activity_service_url = env::var("ACTIVITY_SERVICE_URL")
        .unwrap_or_else(|_| "http://localhost:50052".to_string());
    
    let screenshot_service_url = env::var("SCREENSHOT_SERVICE_URL")
        .unwrap_or_else(|_| "http://localhost:50053".to_string());

    let redis_url = env::var("REDIS_URL")
        .unwrap_or_else(|_| "redis://localhost:6379".to_string());

    let max_requests_per_minute = env::var("MAX_REQUESTS_PER_MINUTE")
        .unwrap_or_else(|_| "1000".to_string())
        .parse::<u32>()
        .unwrap_or(1000);

    let port = env::var("API_GATEWAY_PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse::<u16>()
        .unwrap_or(8080);

    // Create Redis client
    let redis_client = RedisClient::open(redis_url)?;

    // Create API Gateway
    let gateway = Arc::new(ApiGateway::new(
        auth_service_url,
        activity_service_url,
        screenshot_service_url,
        redis_client,
        max_requests_per_minute,
    ));

    // Create service
    let make_svc = make_service_fn(move |_conn| {
        let gateway = gateway.clone();
        async move {
            Ok::<_, Infallible>(service_fn(move |req| {
                let gateway = gateway.clone();
                handle_request(req, gateway)
            }))
        }
    });

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    
    info!("API Gateway starting on {} with high-performance optimizations", addr);
    info!("Rate limit: {} requests per minute", max_requests_per_minute);

    let server = Server::bind(&addr).serve(make_svc);

    if let Err(e) = server.await {
        error!("Server error: {}", e);
    }

    Ok(())
}
