mod server;

use rust_agent::{api, providers, quota, state};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("rust_agent=debug".parse().unwrap()),
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

    tracing::info!("Database initialized");

    // Initialize providers with dynamic discovery
    let providers_map = providers::init_providers().await;
    tracing::info!("Loaded {} providers", providers_map.len());

    // Shared state
    let app_state = Arc::new(state::AppState {
        db_pool,
        providers: providers_map,
    });

    // Start HTTP server
    server::start_server(app_state)
        .await
        .expect("Server failed to start");
}
