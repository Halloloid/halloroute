use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// Bearer token authentication error
#[derive(Debug)]
pub enum AuthError {
    /// Missing authorization header
    MissingToken,
    /// Invalid token format
    InvalidToken,
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::MissingToken => write!(f, "Missing authorization header"),
            AuthError::InvalidToken => write!(f, "Invalid token format"),
        }
    }
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AuthError::MissingToken => (StatusCode::UNAUTHORIZED, "Missing authorization header"),
            AuthError::InvalidToken => (StatusCode::UNAUTHORIZED, "Invalid token format"),
        };

        let body = Json(json!({
            "error": {
                "message": message,
                "type": "unauthorized",
                "code": status.as_u16(),
            }
        }));

        (status, body).into_response()
    }
}

/// Bearer token extractor
pub struct BearerAuth(pub String);

#[async_trait]
impl<S> FromRequestParts<S> for BearerAuth
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|h| h.to_str().ok())
            .ok_or(AuthError::MissingToken)?;

        if !auth_header.starts_with("Bearer ") {
            return Err(AuthError::InvalidToken);
        }

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or(AuthError::InvalidToken)?
            .to_string();

        if token.is_empty() {
            return Err(AuthError::InvalidToken);
        }

        Ok(BearerAuth(token))
    }
}

/// Validate API key against configured keys
pub fn validate_api_key(token: &str) -> bool {
    // Get configured API key from environment
    if let Ok(configured_key) = std::env::var("RUSTAGENT_API_KEY") {
        return token == configured_key;
    }

    // If no configured key, allow all requests (for backward compatibility)
    // In production, this should be handled differently
    tracing::warn!("No RUSTAGENT_API_KEY configured, allowing all requests");
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_api_key() {
        // When no env var is set, validation should pass (warning logged)
        std::env::remove_var("RUSTAGENT_API_KEY");
        assert!(validate_api_key("any-token"));
    }
}
