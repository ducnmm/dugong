use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum BackendError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Redis error: {0}")]
    Redis(#[from] redis::RedisError),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Webhook validation failed: {0}")]
    WebhookValidation(String),

    #[error("Event already processed: {0}")]
    DuplicateEvent(String),

    #[error("Twitter API error: {0}")]
    TwitterApi(String),

    #[error("Enclave error: {0}")]
    Enclave(String),

    #[error("Sui network error: {0}")]
    SuiNetwork(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl IntoResponse for BackendError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            BackendError::Database(ref e) => {
                tracing::error!("Database error: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Database error")
            }
            BackendError::Redis(ref e) => {
                tracing::error!("Redis error: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Cache error")
            }
            BackendError::Config(ref e) => {
                tracing::error!("Configuration error: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Configuration error")
            }
            BackendError::WebhookValidation(ref e) => {
                tracing::warn!("Webhook validation failed: {}", e);
                (StatusCode::BAD_REQUEST, "Invalid webhook")
            }
            BackendError::DuplicateEvent(ref e) => {
                tracing::info!("Duplicate event: {}", e);
                (StatusCode::OK, "Already processed")
            }
            BackendError::TwitterApi(ref e) => {
                tracing::error!("Twitter API error: {}", e);
                (StatusCode::BAD_GATEWAY, "Twitter API error")
            }
            BackendError::Enclave(ref e) => {
                tracing::error!("Enclave error: {}", e);
                (StatusCode::BAD_GATEWAY, "Enclave error")
            }
            BackendError::SuiNetwork(ref e) => {
                tracing::error!("Sui network error: {}", e);
                (StatusCode::BAD_GATEWAY, "Blockchain error")
            }
            BackendError::Internal(ref e) => {
                tracing::error!("Internal error: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal error")
            }
        };

        let body = Json(json!({
            "error": error_message,
            "details": self.to_string(),
        }));

        (status, body).into_response()
    }
}

// Convenience type alias
pub type Result<T> = std::result::Result<T, BackendError>;

// Helper for converting anyhow errors
impl From<anyhow::Error> for BackendError {
    fn from(err: anyhow::Error) -> Self {
        BackendError::Internal(err.to_string())
    }
}
