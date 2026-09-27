mod config;
mod db;
mod errors;
mod handlers;
mod providers;
mod quota;
mod router;
mod server;

use std::sync::Arc;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("rust_agent=debug".parse().unwrap()),
        )
        .init();

    tracing::info!("Starting RustAgent Gateway");

    // Initialize database
    let db_pool = db::init_pool()
        .await
        .expect("Failed to initialize database");
    db::run_migrations(&db_pool)
        .await
        .expect("Failed to run migrations");

    tracing::info!("Database initialized");

    // Initialize providers
    let providers = providers::init_providers();
    tracing::info!("Loaded {} providers", providers.len());

    // Shared state
    let state = Arc::new(server::AppState { db_pool, providers });

    // Start HTTP server
    server::start_server(state).await
}
