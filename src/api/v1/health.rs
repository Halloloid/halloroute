use axum::Json;
use serde_json::json;

use crate::errors::ApiResult;

pub async fn health_check() -> ApiResult<Json<serde_json::Value>> {
    Ok(Json(json!({
        "status": "healthy",
        "service": "RustAgent Gateway",
        "version": env!("CARGO_PKG_VERSION"),
        "timestamp": chrono::Utc::now().to_rfc3339(),
    })))
}
