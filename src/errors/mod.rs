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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[tokio::test]
    async fn test_error_status_codes_and_payloads() {
        let test_cases = vec![
            (
                ApiError::InvalidRequest("bad param".into()),
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "Invalid request: bad param",
            ),
            (
                ApiError::ProviderNotFound("groq".into()),
                StatusCode::NOT_FOUND,
                "provider_not_found",
                "Provider not found: groq",
            ),
            (
                ApiError::QuotaExceeded("together".into()),
                StatusCode::TOO_MANY_REQUESTS,
                "quota_exceeded",
                "Quota exceeded: together",
            ),
            (
                ApiError::AllProvidersUnavailable,
                StatusCode::SERVICE_UNAVAILABLE,
                "providers_unavailable",
                "All providers unavailable",
            ),
            (
                ApiError::RequestTimeout,
                StatusCode::GATEWAY_TIMEOUT,
                "timeout",
                "Request timeout",
            ),
            (
                ApiError::DatabaseError("disk full".into()),
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Database error: disk full",
            ),
            (
                ApiError::ProviderError("upstream 502".into()),
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Provider error: upstream 502",
            ),
            (
                ApiError::InternalError("state broken".into()),
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Internal server error: state broken",
            ),
        ];

        for (err, expected_status, expected_type, expected_msg) in test_cases {
            let response = err.into_response();
            assert_eq!(response.status(), expected_status);

            let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

            assert_eq!(body_json["error"]["type"], expected_type);
            assert_eq!(body_json["error"]["message"], expected_msg);
            assert_eq!(body_json["error"]["code"], expected_status.as_u16());
        }
    }
}
