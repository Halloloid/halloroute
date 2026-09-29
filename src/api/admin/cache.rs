use axum::{extract::State, Json};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::errors::ApiResult;
use crate::state::AppState;

pub async fn get_cache_stats(State(state): State<Arc<AppState>>) -> ApiResult<Json<Value>> {
    let metrics = state.request_deduplicator.get_metrics();
    let entries = state.request_deduplicator.entries_count();

    Ok(Json(json!({
        "status": "ok",
        "entries": entries,
        "hits": metrics.hits,
        "misses": metrics.misses,
        "evictions": metrics.evictions,
        "hit_rate_percent": metrics.hit_rate,
    })))
}

pub async fn clear_cache(State(state): State<Arc<AppState>>) -> ApiResult<Json<Value>> {
    state.request_deduplicator.clear();
    Ok(Json(json!({
        "status": "ok",
        "message": "Cache cleared successfully"
    })))
}
