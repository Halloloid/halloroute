use actix_web::{error::ResponseError, http::StatusCode, HttpResponse};
use serde_json::json;
use std::fmt;
use thiserror::Error;

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

    #[error("Internal server error")]
    InternalError(String),
}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        match self {
            ApiError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::ProviderNotFound(_) => StatusCode::NOT_FOUND,
            ApiError::QuotaExceeded(_) => StatusCode::TOO_MANY_REQUESTS,
            ApiError::AllProvidersUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            ApiError::RequestTimeout => StatusCode::GATEWAY_TIMEOUT,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        let status = self.status_code();
        let body = json!({
            "error": {
                "message": self.to_string(),
                "type": "api_error",
                "code": status.as_u16(),
            }
        });

        HttpResponse::build(status).json(body)
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
