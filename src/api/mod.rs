pub mod admin;
pub mod v1;

pub use admin::{
    clear_cache, create_provider, delete_provider, disable_provider, enable_provider,
    get_cache_stats, get_provider, get_providers, get_quota_status, list_dynamic_providers,
    ping_provider, update_provider,
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
        .route("/v1/chat/completions", post(v1::chat_completions_entry))
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
        // Gateway API key management
        .route(
            "/admin/gateway-keys",
            post(admin::create_gateway_key).get(admin::list_gateway_keys),
        )
        .route(
            "/admin/gateway-keys/:id",
            axum::routing::delete(admin::revoke_gateway_key),
        )
        // Model pools (virtual load-balanced models)
        .route(
            "/admin/pools",
            post(admin::create_model_pool).get(admin::list_model_pools),
        )
        .route("/admin/pools/candidates", get(admin::pool_candidates))
        .route(
            "/admin/pools/:id",
            axum::routing::put(admin::update_model_pool).delete(admin::delete_model_pool),
        )
        .route(
            "/admin/pools/:id/collective",
            axum::routing::patch(admin::toggle_collective),
        )
        // Dynamic provider management
        .route("/admin/providers/ping", post(admin::ping_provider))
        .route("/admin/providers/dynamic", post(admin::create_provider))
        .route(
            "/admin/providers/dynamic",
            get(admin::list_dynamic_providers),
        )
        .route("/admin/providers/dynamic/:id", get(admin::get_provider))
        .route(
            "/admin/providers/dynamic/:id",
            axum::routing::put(admin::update_provider),
        )
        .route(
            "/admin/providers/dynamic/:id",
            axum::routing::delete(admin::delete_provider),
        )
}
