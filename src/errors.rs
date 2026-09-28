use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum ApiError {
    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Provider error: {0}")]
    ProviderError(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Provider not found: {0}")]
    ProviderNotFound(String),

    #[error("Quota exceeded: {0}")]
    QuotaExceeded(String),

    #[error("All providers unavailable")]
    AllProvidersUnavailable,

    #[error("Request timeout")]
    RequestTimeout,

    #[error("Internal server error: {0}")]
    InternalError(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, error_type, message) = match &self {
            ApiError::InvalidRequest(_) => {
                (StatusCode::BAD_REQUEST, "invalid_request", self.to_string())
            }
            ApiError::ProviderNotFound(_) => (
                StatusCode::NOT_FOUND,
                "provider_not_found",
                self.to_string(),
            ),
            ApiError::QuotaExceeded(_) => (
                StatusCode::TOO_MANY_REQUESTS,
                "quota_exceeded",
                self.to_string(),
            ),
            ApiError::AllProvidersUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "providers_unavailable",
                self.to_string(),
            ),
            ApiError::RequestTimeout => (StatusCode::GATEWAY_TIMEOUT, "timeout", self.to_string()),
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                self.to_string(),
            ),
        };

        let body = Json(json!({
            "error": {
                "message": message,
                "type": error_type,
                "code": status.as_u16(),
            }
        }));

        (status, body).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
