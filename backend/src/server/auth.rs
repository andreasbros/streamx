use crate::error::{Error, Result};
use axum::extract::{ConnectInfo, FromRef, FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::warn;

use super::AppState;

const BCRYPT_COST: u32 = 12;
const RATE_LIMIT_WINDOW_SECS: i64 = 60;
const RATE_LIMIT_MAX_REQUESTS: usize = 10;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    User,
    Guest,
}

fn default_role() -> Role {
    Role::User
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub user_id: String,
    pub username: String,
    #[serde(default = "default_role")]
    pub role: Role,
    /// Only set for Guest tokens - restricts access to this stream
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_id: Option<String>,
    pub exp: usize,
}

/// Extractor: any valid token (user, admin, or guest)
impl<S> FromRequestParts<S> for Claims
where
    S: Send + Sync,
    AppState: axum::extract::FromRef<S>,
{
    type Rejection = Error;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);
        let token = extract_token(parts)?;
        validate_jwt(&token, &app_state.jwt_secret)
    }
}

/// Extractor: rejects Guest tokens. Use for browse, search, settings, admin, etc.
pub struct AuthenticatedUser(pub Claims);

impl<S> FromRequestParts<S> for AuthenticatedUser
where
    S: Send + Sync,
    AppState: axum::extract::FromRef<S>,
{
    type Rejection = Error;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);
        let token = extract_token(parts)?;
        let claims = validate_jwt(&token, &app_state.jwt_secret)?;
        if claims.role == Role::Guest {
            return Err(Error::Unauthorized {
                message: "Guest access not allowed for this endpoint".to_string(),
            });
        }
        Ok(AuthenticatedUser(claims))
    }
}

fn extract_token(parts: &Parts) -> std::result::Result<String, Error> {
    parts
        .headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| t.to_string())
        .or_else(|| {
            parts
                .uri
                .query()
                .and_then(|q| {
                    q.split('&')
                        .find_map(|pair| pair.strip_prefix("token="))
                })
                .map(|t| t.to_string())
        })
        .or_else(|| {
            // Also check for ?guest= query param
            parts
                .uri
                .query()
                .and_then(|q| {
                    q.split('&')
                        .find_map(|pair| pair.strip_prefix("guest="))
                })
                .map(|t| t.to_string())
        })
        .ok_or_else(|| Error::Unauthorized {
            message: "Missing authorization (header or ?token= query param)".to_string(),
        })
}

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct MeResponse {
    pub id: String,
    pub username: String,
    pub is_admin: bool,
    pub created_at: String,
}

#[derive(Clone)]
pub struct RateLimiter {
    requests: Arc<Mutex<HashMap<String, Vec<i64>>>>,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            requests: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn check(&self, ip: &str) -> Result<()> {
        let now = Utc::now().timestamp();
        let cutoff = now - RATE_LIMIT_WINDOW_SECS;
        let mut map = self.requests.lock().await;

        let timestamps = map.entry(ip.to_string()).or_default();
        timestamps.retain(|&t| t > cutoff);

        if timestamps.len() >= RATE_LIMIT_MAX_REQUESTS {
            warn!(ip = ip, "Rate limit exceeded");
            return Err(Error::RateLimited);
        }

        timestamps.push(now);
        Ok(())
    }
}

pub fn create_jwt(
    user_id: &str,
    username: &str,
    is_admin: bool,
    secret: &str,
    duration_hours: i64,
) -> Result<String> {
    let expiration = Utc::now()
        .checked_add_signed(Duration::hours(duration_hours))
        .ok_or_else(|| Error::Internal {
            message: "Failed to compute token expiration".to_string(),
        })?;

    let role = if is_admin { Role::Admin } else { Role::User };

    let claims = Claims {
        user_id: user_id.to_string(),
        username: username.to_string(),
        role,
        stream_id: None,
        exp: expiration.timestamp() as usize,
    };

    let header = Header::default();
    let key = EncodingKey::from_secret(secret.as_bytes());
    encode(&header, &claims, &key).map_err(|source| Error::Jwt { source })
}

pub fn create_guest_token(
    stream_id: &str,
    secret: &str,
    duration_hours: i64,
) -> Result<String> {
    let expiration = Utc::now()
        .checked_add_signed(Duration::hours(duration_hours))
        .ok_or_else(|| Error::Internal {
            message: "Failed to compute token expiration".to_string(),
        })?;

    let claims = Claims {
        user_id: "guest".to_string(),
        username: "guest".to_string(),
        role: Role::Guest,
        stream_id: Some(stream_id.to_string()),
        exp: expiration.timestamp() as usize,
    };

    let header = Header::default();
    let key = EncodingKey::from_secret(secret.as_bytes());
    encode(&header, &claims, &key).map_err(|source| Error::Jwt { source })
}

pub fn validate_jwt(token: &str, secret: &str) -> Result<Claims> {
    let key = DecodingKey::from_secret(secret.as_bytes());
    let validation = Validation::default();

    let token_data =
        decode::<Claims>(token, &key, &validation).map_err(|source| Error::Jwt { source })?;

    Ok(token_data.claims)
}

pub fn hash_password(password: &str) -> Result<String> {
    bcrypt::hash(password, BCRYPT_COST).map_err(|e| Error::PasswordHash {
        message: e.to_string(),
    })
}

fn verify_password(password: &str, hash: &str) -> Result<bool> {
    bcrypt::verify(password, hash).map_err(|e| Error::PasswordHash {
        message: e.to_string(),
    })
}

fn validate_username(username: &str) -> Result<()> {
    let username = username.trim();
    if username.len() < 3 || username.len() > 32 {
        return Err(Error::BadRequest {
            message: "Username must be between 3 and 32 characters".to_string(),
        });
    }
    if !username.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err(Error::BadRequest {
            message: "Username may only contain letters, numbers, and underscores".to_string(),
        });
    }
    Ok(())
}

fn validate_password(password: &str) -> Result<()> {
    if password.len() < 8 || password.len() > 128 {
        return Err(Error::BadRequest {
            message: "Password must be between 8 and 128 characters".to_string(),
        });
    }
    Ok(())
}

fn parse_session_duration(duration_str: &str) -> Result<i64> {
    let trimmed = duration_str.trim();
    if let Some(days) = trimmed.strip_suffix('d') {
        let d: u64 = days.parse().map_err(|_| Error::Config {
            message: format!("Invalid session duration: {trimmed}"),
        })?;
        Ok(d as i64 * 24)
    } else if let Some(hours) = trimmed.strip_suffix('h') {
        let h: u64 = hours.parse().map_err(|_| Error::Config {
            message: format!("Invalid session duration: {trimmed}"),
        })?;
        Ok(h as i64)
    } else {
        Ok(168)
    }
}

pub async fn register(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<RegisterRequest>,
) -> std::result::Result<impl IntoResponse, Error> {
    state.rate_limiter.check(&addr.ip().to_string()).await?;

    let username = body.username.trim().to_lowercase();
    let password = &body.password;

    validate_username(&username)?;
    validate_password(password)?;

    let existing = state.db.find_user_by_username(&username).await?;
    if existing.is_some() {
        return Err(Error::BadRequest {
            message: "Username already taken".to_string(),
        });
    }

    let password_hash = hash_password(password)?;
    let user = state.db.create_user(&username, &password_hash).await?;

    let duration_hours = parse_session_duration(&state.config.auth.session_duration)?;
    let token = create_jwt(&user.id, &user.username, user.is_admin, &state.jwt_secret, duration_hours)?;

    Ok((StatusCode::CREATED, Json(AuthResponse { token })))
}

pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<LoginRequest>,
) -> std::result::Result<impl IntoResponse, Error> {
    state.rate_limiter.check(&addr.ip().to_string()).await?;

    let username = body.username.trim().to_lowercase();
    let password = &body.password;

    let user = state
        .db
        .find_user_by_username(&username)
        .await?
        .ok_or_else(|| Error::Auth {
            message: "Invalid username or password".to_string(),
        })?;

    let valid = verify_password(password, &user.password_hash)?;
    if !valid {
        return Err(Error::Auth {
            message: "Invalid username or password".to_string(),
        });
    }

    let duration_hours = parse_session_duration(&state.config.auth.session_duration)?;
    let token = create_jwt(&user.id, &user.username, user.is_admin, &state.jwt_secret, duration_hours)?;

    Ok(Json(AuthResponse { token }))
}

pub async fn me(
    State(state): State<AppState>,
    claims: Claims,
) -> std::result::Result<impl IntoResponse, Error> {
    // Guest tokens return minimal info
    if claims.role == Role::Guest {
        return Ok(Json(MeResponse {
            id: "guest".to_string(),
            username: "guest".to_string(),
            is_admin: false,
            created_at: String::new(),
        }));
    }

    let user = state
        .db
        .find_user_by_id(&claims.user_id)
        .await?
        .ok_or_else(|| Error::NotFound {
            message: "User not found".to_string(),
        })?;

    Ok(Json(MeResponse {
        id: user.id,
        username: user.username,
        is_admin: user.is_admin,
        created_at: user.created_at,
    }))
}
