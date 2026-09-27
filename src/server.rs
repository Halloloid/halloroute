use actix_web::{middleware, web, App, HttpServer};
use std::collections::HashMap;
use std::sync::Arc;

use crate::db::DbPool;
use crate::handlers;
use crate::providers::Provider;

pub struct AppState {
    pub db_pool: DbPool,
    pub providers: HashMap<String, Provider>,
}

pub async fn start_server(state: Arc<AppState>) -> std::io::Result<()> {
    let config = crate::config::Config::from_env();
    let bind_addr = format!("{}:{}", config.server.host, config.server.port);

    tracing::info!("Starting HTTP server on {}", bind_addr);

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new((*state).clone()))
            .wrap(middleware::Logger::default())
            .wrap(middleware::NormalizePath::trim())
            // Health check
            .route("/health", web::get().to(handlers::health_check))
            // OpenAI-compatible endpoints
            .route(
                "/v1/chat/completions",
                web::post().to(handlers::chat_completions),
            )
            .route("/v1/models", web::get().to(handlers::list_models))
            // Admin endpoints
            .route("/admin/providers", web::get().to(handlers::get_providers))
            .route("/admin/quota", web::get().to(handlers::get_quota_status))
            .route(
                "/admin/providers/{id}/disable",
                web::post().to(handlers::disable_provider),
            )
            .route(
                "/admin/providers/{id}/enable",
                web::post().to(handlers::enable_provider),
            )
    })
    .workers(config.server.worker_threads)
    .bind(&bind_addr)?
    .run()
    .await
}

// Clone impl for AppState (needed for use with web::Data)
impl Clone for AppState {
    fn clone(&self) -> Self {
        AppState {
            db_pool: self.db_pool.clone(),
            providers: self.providers.clone(),
        }
    }
}
