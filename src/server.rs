use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::db::DbPool;
use crate::handlers;
use crate::providers::Provider;
use std::collections::HashMap;

#[derive(Clone)]
pub struct AppState {
    pub db_pool: DbPool,
    pub providers: HashMap<String, Provider>,
}

pub async fn start_server(state: Arc<AppState>) -> Result<(), std::io::Error> {
    let app = Router::new()
        // Health check
        .route("/health", get(handlers::health_check))
        // OpenAI-compatible endpoints
        .route("/v1/chat/completions", post(handlers::chat_completions))
        .route("/v1/models", get(handlers::list_models))
        // Admin endpoints
        .route("/admin/providers", get(handlers::get_providers))
        .route("/admin/quota", get(handlers::get_quota_status))
        .route(
            "/admin/providers/:id/disable",
            post(handlers::disable_provider),
        )
        .route(
            "/admin/providers/:id/enable",
            post(handlers::enable_provider),
        )
        // Layers
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:20128").await?;

    tracing::info!("HTTP server listening on 127.0.0.1:20128");

    axum::serve(listener, app).await?;

    Ok(())
}
