use actix_web::HttpResponse;
use serde_json::json;

use crate::errors::ApiResult;

pub async fn health_check() -> ApiResult<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!({
        "status": "healthy",
        "service": "RustAgent Gateway",
        "version": env!("CARGO_PKG_VERSION"),
    })))
}
