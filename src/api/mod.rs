pub mod admin;
pub mod v1;

pub use admin::{
    clear_cache, disable_provider, enable_provider, get_cache_stats, get_providers,
    get_quota_status,
};
pub use v1::{
    chat_completions, chat_completions_streaming, create_embeddings, health_check, list_models,
};

use crate::state::AppState;
use axum::routing::{get, post};
use axum::Router;
use std::sync::Arc;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        // Health check
        .route("/health", get(v1::health_check))
        // OpenAI-compatible endpoints
        .route("/v1/chat/completions", post(v1::chat_completions))
        .route("/v1/models", get(v1::list_models))
        .route("/v1/embeddings", post(v1::create_embeddings))
        // Admin endpoints
        .route("/admin/providers", get(admin::get_providers))
        .route("/admin/quota", get(admin::get_quota_status))
        .route("/admin/cache", get(admin::get_cache_stats))
        .route("/admin/cache/clear", post(admin::clear_cache))
        .route(
            "/admin/providers/:id/disable",
            post(admin::disable_provider),
        )
        .route("/admin/providers/:id/enable", post(admin::enable_provider))
}
