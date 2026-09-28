use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::api;
use crate::state::AppState;

pub async fn start_server(state: Arc<AppState>) -> Result<(), std::io::Error> {
    let app = api::routes()
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:20128").await?;

    tracing::info!("HTTP server listening on 127.0.0.1:20128");

    axum::serve(listener, app).await?;

    Ok(())
}
