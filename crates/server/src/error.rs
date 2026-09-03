use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum Error {
    #[snafu(display("Failed to bind server to {address}"))]
    ServerBind {
        address: String,
        source: std::io::Error,
    },

    #[snafu(display("Database error: {source}"))]
    Database { source: rusqlite::Error },

    #[snafu(display("Configuration error: {message}"))]
    Config { message: String },

    #[snafu(display("Provider error: {message}"))]
    Provider { message: String },

    #[snafu(display("Authentication error: {message}"))]
    Auth { message: String },

    #[snafu(display("Torrent error: {message}"))]
    Torrent { message: String },

    #[snafu(display("Transcode error: {message}"))]
    Transcode { message: String },

    #[snafu(display("Storage error: {message}"))]
    Storage { message: String },

    #[snafu(display("Not found: {message}"))]
    NotFound { message: String },

    #[snafu(display("Bad request: {message}"))]
    BadRequest { message: String },

    #[snafu(display("Unauthorized: {message}"))]
    Unauthorized { message: String },

    #[snafu(display("Internal error: {message}"))]
    Internal { message: String },

    #[snafu(display("IO error: {source}"))]
    Io { source: std::io::Error },

    #[snafu(display("Password hashing error: {message}"))]
    PasswordHash { message: String },

    #[snafu(display("JWT error: {source}"))]
    Jwt { source: jsonwebtoken::errors::Error },

    #[snafu(display("Rate limited"))]
    RateLimited,
}

pub type Result<T> = std::result::Result<T, Error>;

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            Error::ServerBind { address, .. } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to bind to {address}"),
            ),
            Error::Database { .. } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error".to_string(),
            ),
            Error::Config { message } => (StatusCode::INTERNAL_SERVER_ERROR, message.clone()),
            Error::Provider { message } => (StatusCode::BAD_GATEWAY, message.clone()),
            Error::Auth { message } => (StatusCode::UNAUTHORIZED, message.clone()),
            Error::Torrent { message } => (StatusCode::INTERNAL_SERVER_ERROR, message.clone()),
            Error::Transcode { message } => (StatusCode::INTERNAL_SERVER_ERROR, message.clone()),
            Error::Storage { message } => (StatusCode::SERVICE_UNAVAILABLE, message.clone()),
            Error::NotFound { message } => (StatusCode::NOT_FOUND, message.clone()),
            Error::BadRequest { message } => (StatusCode::BAD_REQUEST, message.clone()),
            Error::Unauthorized { message } => (StatusCode::UNAUTHORIZED, message.clone()),
            Error::Internal { message } => (StatusCode::INTERNAL_SERVER_ERROR, message.clone()),
            Error::Io { .. } => (StatusCode::INTERNAL_SERVER_ERROR, "IO error".to_string()),
            Error::PasswordHash { .. } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Password processing error".to_string(),
            ),
            Error::Jwt { .. } => (StatusCode::UNAUTHORIZED, "Invalid token".to_string()),
            Error::RateLimited => (
                StatusCode::TOO_MANY_REQUESTS,
                "Too many requests, please try again later".to_string(),
            ),
        };

        let body = serde_json::json!({ "error": message });
        (status, axum::Json(body)).into_response()
    }
}
