use dashmap::DashMap;
use halloroute::{
    cache::RequestDeduplicator,
    providers, quota, server, state,
    token::{ContextWindowManager, TokenUsageStats},
};
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("halloroute=debug".parse().unwrap()),
        )
        .init();

    tracing::info!("Starting RustAgent Gateway");

    // Initialize database
    let db_pool = quota::init_pool()
        .await
        .expect("Failed to initialize database");
    quota::run_migrations(&db_pool)
        .await
        .expect("Failed to run migrations");

    // Start background quota reset jobs
    quota::start_quota_reset_tasks(db_pool.clone());
    tracing::info!("Database & quota background reset jobs initialized");

    // Initialize providers with dynamic discovery
    let providers_map = providers::init_providers().await;
    let providers = Arc::new(DashMap::new());
    for (id, provider) in providers_map {
        providers.insert(id, provider);
    }
    tracing::info!("Loaded {} providers", providers.len());

    // Initialize request deduplication / response cache (1-hour TTL, 2000 items max)
    let request_deduplicator = Arc::new(RequestDeduplicator::new(Duration::from_secs(3600), 2000));

    // Initialize context window manager & token usage stats
    let context_window_manager = ContextWindowManager::new();
    let token_usage_stats = Arc::new(TokenUsageStats::new());

    // Shared state
    let app_state = Arc::new(state::AppState {
        db_pool,
        providers,
        request_deduplicator,
        context_window_manager,
        token_usage_stats,
    });

    // Start HTTP server with graceful shutdown
    server::start_server(app_state)
        .await
        .expect("Server failed to start");
}
