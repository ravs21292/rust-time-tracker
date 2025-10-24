use tonic::{transport::Server, Request, Response, Status};
use auth_service::auth_service_server::{AuthService, AuthServiceServer};
use auth_service::{LoginRequest, LoginResponse, LogoutRequest, LogoutResponse, 
                  ValidateTokenRequest, ValidateTokenResponse, RefreshTokenRequest, 
                  RefreshTokenResponse, GetUserProfileRequest, GetUserProfileResponse, UserProfile};
use sqlx::{PgPool, Row};
use uuid::Uuid;
use chrono::{Duration, Utc};
use bcrypt::{hash, verify, DEFAULT_COST};
use std::env;

pub mod auth_service {
    tonic::include_proto!("auth");
}

#[derive(Debug)]
pub struct AuthServiceImpl {
    db_pool: PgPool,
    jwt_secret: String,
}

impl AuthServiceImpl {
    pub fn new(db_pool: PgPool, jwt_secret: String) -> Self {
        Self { db_pool, jwt_secret }
    }

    async fn hash_password(&self, password: &str) -> Result<String, Status> {
        hash(password, DEFAULT_COST)
            .map_err(|_| Status::internal("Failed to hash password"))
    }

    async fn verify_password(&self, password: &str, hash: &str) -> Result<bool, Status> {
        verify(password, hash)
            .map_err(|_| Status::internal("Failed to verify password"))
    }

    async fn generate_session_token(&self, employee_id: i32) -> Result<String, Status> {
        let token = Uuid::new_v4().to_string();
        let expires_at = Utc::now() + Duration::hours(24);
        
        sqlx::query!(
            "INSERT INTO user_sessions (employee_id, session_token, expires_at) VALUES ($1, $2, $3)",
            employee_id,
            token,
            expires_at
        )
        .execute(&self.db_pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to create session: {}", e);
            Status::internal("Failed to create session")
        })?;

        Ok(token)
    }

    async fn validate_session_token(&self, token: &str) -> Result<i32, Status> {
        let row = sqlx::query!(
            "SELECT employee_id FROM user_sessions WHERE session_token = $1 AND expires_at > $2 AND is_active = true",
            token,
            Utc::now()
        )
        .fetch_optional(&self.db_pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to validate session: {}", e);
            Status::internal("Failed to validate session")
        })?;

        match row {
            Some(row) => Ok(row.employee_id),
            None => Err(Status::unauthenticated("Invalid or expired session token"))
        }
    }
}

#[tonic::async_trait]
impl AuthService for AuthServiceImpl {
    async fn login(&self, request: Request<LoginRequest>) -> Result<Response<LoginResponse>, Status> {
        let req = request.into_inner();
        
        tracing::info!("Login attempt for email: {}", req.email);

        // Get user from database
        let user_row = sqlx::query!(
            "SELECT id, name, email, department, password_hash, is_active FROM employees WHERE email = $1",
            req.email
        )
        .fetch_optional(&self.db_pool)
        .await
        .map_err(|e| {
            tracing::error!("Database error during login: {}", e);
            Status::internal("Database error")
        })?;

        let user_row = match user_row {
            Some(user) => user,
            None => {
                return Ok(Response::new(LoginResponse {
                    success: false,
                    message: "Invalid email or password".to_string(),
                    session_token: String::new(),
                    user: None,
                }));
            }
        };

        // Check if user is active
        if !user_row.is_active.unwrap_or(false) {
            return Ok(Response::new(LoginResponse {
                success: false,
                message: "Account is deactivated".to_string(),
                session_token: String::new(),
                user: None,
            }));
        }

        // Verify password
        let password_valid = self.verify_password(&req.password, &user_row.password_hash).await?;
        if !password_valid {
            return Ok(Response::new(LoginResponse {
                success: false,
                message: "Invalid email or password".to_string(),
                session_token: String::new(),
                user: None,
            }));
        }

        // Generate session token
        let session_token = self.generate_session_token(user_row.id).await?;

        let user_profile = Some(UserProfile {
            id: user_row.id,
            name: user_row.name.clone(),
            email: user_row.email.clone(),
            department: user_row.department.unwrap_or_default(),
            is_active: user_row.is_active.unwrap_or(false),
        });

        tracing::info!("Successful login for user: {}", user_row.email);

        Ok(Response::new(LoginResponse {
            success: true,
            message: "Login successful".to_string(),
            session_token,
            user: user_profile,
        }))
    }

    async fn logout(&self, request: Request<LogoutRequest>) -> Result<Response<LogoutResponse>, Status> {
        let req = request.into_inner();
        
        tracing::info!("Logout request for session token");

        // Deactivate session
        sqlx::query!(
            "UPDATE user_sessions SET is_active = false WHERE session_token = $1",
            req.session_token
        )
        .execute(&self.db_pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to logout: {}", e);
            Status::internal("Failed to logout")
        })?;

        Ok(Response::new(LogoutResponse {
            success: true,
            message: "Logout successful".to_string(),
        }))
    }

    async fn validate_token(&self, request: Request<ValidateTokenRequest>) -> Result<Response<ValidateTokenResponse>, Status> {
        let req = request.into_inner();
        
        match self.validate_session_token(&req.session_token).await {
            Ok(employee_id) => Ok(Response::new(ValidateTokenResponse {
                valid: true,
                employee_id,
                message: "Token is valid".to_string(),
            })),
            Err(status) => Ok(Response::new(ValidateTokenResponse {
                valid: false,
                employee_id: 0,
                message: status.message().to_string(),
            })),
        }
    }

    async fn refresh_token(&self, request: Request<RefreshTokenRequest>) -> Result<Response<RefreshTokenResponse>, Status> {
        let req = request.into_inner();
        
        // Validate current token
        let employee_id = self.validate_session_token(&req.session_token).await?;
        
        // Deactivate old session
        sqlx::query!(
            "UPDATE user_sessions SET is_active = false WHERE session_token = $1",
            req.session_token
        )
        .execute(&self.db_pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to deactivate old session: {}", e);
            Status::internal("Failed to refresh token")
        })?;

        // Generate new session token
        let new_session_token = self.generate_session_token(employee_id).await?;

        Ok(Response::new(RefreshTokenResponse {
            success: true,
            new_session_token,
            message: "Token refreshed successfully".to_string(),
        }))
    }

    async fn get_user_profile(&self, request: Request<GetUserProfileRequest>) -> Result<Response<GetUserProfileResponse>, Status> {
        let req = request.into_inner();
        
        let user_row = sqlx::query!(
            "SELECT id, name, email, department, is_active FROM employees WHERE id = $1",
            req.employee_id
        )
        .fetch_optional(&self.db_pool)
        .await
        .map_err(|e| {
            tracing::error!("Database error during profile fetch: {}", e);
            Status::internal("Database error")
        })?;

        match user_row {
            Some(user) => {
                let user_profile = UserProfile {
                    id: user.id,
                    name: user.name,
                    email: user.email,
                    department: user.department.unwrap_or_default(),
                    is_active: user.is_active.unwrap_or(false),
                };

                Ok(Response::new(GetUserProfileResponse {
                    success: true,
                    user: Some(user_profile),
                    message: "Profile retrieved successfully".to_string(),
                }))
            }
            None => Ok(Response::new(GetUserProfileResponse {
                success: false,
                user: None,
                message: "User not found".to_string(),
            })),
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
    
    let jwt_secret = env::var("JWT_SECRET")
        .unwrap_or_else(|_| "your-secret-key".to_string());

    let port = env::var("AUTH_SERVICE_PORT")
        .unwrap_or_else(|_| "50051".to_string())
        .parse::<u16>()
        .unwrap_or(50051);

    // Create database connection pool
    let db_pool = PgPool::connect(&database_url).await?;

    let auth_service = AuthServiceImpl::new(db_pool, jwt_secret);
    let auth_service = AuthServiceServer::new(auth_service);

    let addr = format!("0.0.0.0:{}", port).parse()?;
    
    tracing::info!("Auth service starting on {}", addr);

    Server::builder()
        .add_service(auth_service)
        .serve(addr)
        .await?;

    Ok(())
}
