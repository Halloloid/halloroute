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

    // Load dynamic providers from database
    match providers::list_dynamic_providers(&db_pool).await {
        Ok(dynamic_providers) => {
            for record in dynamic_providers {
                if record.is_active {
                    let provider = providers::record_to_provider(&record);
                    providers.insert(provider.id.clone(), provider);
                    tracing::info!(
                        provider_id = %record.id,
                        name = %record.name,
                        "Loaded dynamic provider from database"
                    );
                }
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to load dynamic providers from database");
        }
    }

    tracing::info!("Loaded {} total providers", providers.len());

    // Initialize request deduplication / response cache (1-hour TTL, 2000 items max)
    let request_deduplicator = Arc::new(RequestDeduplicator::new(Duration::from_secs(3600), 2000));

    // Initialize context window manager & token usage stats
    let context_window_manager = ContextWindowManager::new();
    let token_usage_stats = Arc::new(TokenUsageStats::new());

    // Load model pools (virtual load-balanced models) from database
    let pools = Arc::new(halloroute::pools::PoolRegistry::default());
    match halloroute::pools::store::list_pools(&db_pool).await {
        Ok(stored) => {
            for pool in stored {
                tracing::info!(pool = %pool.id, members = pool.members.len(), "Loaded model pool");
                pools.insert(pool);
            }
        }
        Err(e) => tracing::warn!(error = %e, "Failed to load model pools from database"),
    }

    // Shared state
    let app_state = Arc::new(state::AppState {
        db_pool,
        providers,
        pools,
        request_deduplicator,
        context_window_manager,
        token_usage_stats,
    });

    // Start HTTP server with graceful shutdown
    server::start_server(app_state)
        .await
        .expect("Server failed to start");
}
