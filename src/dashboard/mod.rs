use askama::Template;
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Sse},
    routing::{get, post},
    Form, Router,
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio_stream::StreamExt;

use crate::{
    api::v1::chat::{ChatCompletionRequest, Message},
    providers::types::ProviderHealth,
    state::AppState,
};

#[derive(Serialize, Clone)]
pub struct ProviderView {
    pub id: String,
    pub name: String,
    pub endpoint: String,
    pub auth_type: String,
    pub status: String,
    pub enabled: bool,
    pub rate_limit_rpm: i32,
    pub used_tokens: i64,
    pub limit_tokens: i64,
    pub quota_pct: u32,
    pub models: Vec<String>,
}

#[derive(Serialize, Clone)]
pub struct LogView {
    pub timestamp: String,
    pub method: String,
    pub endpoint: String,
    pub model: String,
    pub provider: String,
    pub status: u16,
    pub latency_ms: i64,
    pub cache_hit: bool,
}

#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate {
    pub port: u16,
    pub uptime: String,
    pub memory_mb: String,
    pub active_tab: String,
    pub total_requests: i64,
    pub cache_hit_rate: u32,
    pub cache_hits: u64,
    pub cache_lookups: u64,
    pub active_providers: usize,
    pub total_providers: usize,
    pub tokens_today: i64,
    pub providers: Vec<ProviderView>,
    pub cache_entries: usize,
    pub cache_capacity: usize,
    pub recent_logs: Vec<LogView>,
}

#[derive(Template)]
#[template(path = "tabs/overview.html")]
pub struct OverviewTabTemplate {
    pub total_requests: i64,
    pub cache_hit_rate: u32,
    pub cache_hits: u64,
    pub cache_lookups: u64,
    pub active_providers: usize,
    pub total_providers: usize,
    pub tokens_today: i64,
    pub providers: Vec<ProviderView>,
    pub cache_entries: usize,
    pub cache_capacity: usize,
    pub recent_logs: Vec<LogView>,
}

#[derive(Template)]
#[template(path = "tabs/providers.html")]
pub struct ProvidersTabTemplate {
    pub strategy: String,
    pub providers: Vec<ProviderView>,
}

#[derive(Template)]
#[template(path = "partials/provider_item.html")]
pub struct ProviderItemTemplate {
    pub p: ProviderView,
}

#[derive(Template)]
#[template(path = "tabs/logs.html")]
pub struct LogsTabTemplate {
    pub available_models: Vec<String>,
    pub recent_logs: Vec<LogView>,
}

#[derive(Template)]
#[template(path = "partials/metrics_strip.html")]
pub struct MetricsStripTemplate {
    pub total_requests: i64,
    pub cache_hit_rate: u32,
    pub cache_hits: u64,
    pub cache_lookups: u64,
    pub active_providers: usize,
    pub total_providers: usize,
    pub tokens_today: i64,
}

#[derive(Template)]
#[template(path = "partials/log_row.html")]
pub struct LogRowTemplate {
    pub log: LogView,
}

async fn build_provider_views(state: &AppState) -> Vec<ProviderView> {
    let mut views = Vec::new();
    for entry in state.providers.iter() {
        let id = entry.key();
        let p = entry.value();
        let is_healthy = p.is_available();
        let status = match p.health_status {
            ProviderHealth::Healthy => "Healthy".to_string(),
            ProviderHealth::Degraded => "Degraded".to_string(),
            ProviderHealth::Down => "Down".to_string(),
        };

        let used = crate::quota::tracker::get_today_quota_usage(&state.db_pool, id)
            .await
            .unwrap_or(0);
        let limit = p.free_quota_daily.unwrap_or(100_000);
        let pct = if limit > 0 {
            ((used as f64 / limit as f64) * 100.0).min(100.0) as u32
        } else {
            0
        };

        views.push(ProviderView {
            id: id.clone(),
            name: p.name.clone(),
            endpoint: p.api_endpoint.clone(),
            auth_type: p.auth_type.clone(),
            status,
            enabled: is_healthy,
            rate_limit_rpm: p.rate_limit_rpm,
            used_tokens: used,
            limit_tokens: limit,
            quota_pct: pct,
            models: p.models.clone(),
        });
    }
    views.sort_by(|a, b| a.name.cmp(&b.name));
    views
}

async fn get_recent_log_views(state: &AppState) -> Vec<LogView> {
    let logs = crate::quota::tracker::get_recent_requests(&state.db_pool, 20)
        .await
        .unwrap_or_default();
    logs.into_iter()
        .map(|entry| {
            let ts = if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&entry.created_at) {
                dt.format("%H:%M:%S").to_string()
            } else {
                entry.created_at.chars().take(8).collect()
            };
            LogView {
                timestamp: ts,
                method: "POST".to_string(),
                endpoint: "/v1/chat/completions".to_string(),
                model: entry.model,
                provider: entry.provider_id,
                status: entry.status_code,
                latency_ms: entry.latency_ms,
                cache_hit: false,
            }
        })
        .collect()
}

pub async fn dashboard_index(State(state): State<Arc<AppState>>) -> Html<String> {
    let providers = build_provider_views(&state).await;
    let recent_logs = get_recent_log_views(&state).await;
    let cache_stats = state.request_deduplicator.get_metrics();
    let hit_rate = (cache_stats.hit_rate * 100.0) as u32;
    let token_stats = state.token_usage_stats.get_stats();

    let active_count = providers.iter().filter(|p| p.enabled).count();
    let total_lookups = cache_stats.hits + cache_stats.misses;

    let tmpl = IndexTemplate {
        port: 20128,
        uptime: "Active".to_string(),
        memory_mb: "14.2".to_string(),
        active_tab: "overview".to_string(),
        total_requests: token_stats.total_requests,
        cache_hit_rate: hit_rate,
        cache_hits: cache_stats.hits,
        cache_lookups: total_lookups,
        active_providers: active_count,
        total_providers: providers.len(),
        tokens_today: token_stats.total_input_tokens + token_stats.total_output_tokens,
        providers,
        cache_entries: state.request_deduplicator.entries_count(),
        cache_capacity: 1000,
        recent_logs,
    };

    Html(
        tmpl.render()
            .unwrap_or_else(|e| format!("Render error: {}", e)),
    )
}

pub async fn tab_overview(State(state): State<Arc<AppState>>) -> Html<String> {
    let providers = build_provider_views(&state).await;
    let recent_logs = get_recent_log_views(&state).await;
    let cache_stats = state.request_deduplicator.get_metrics();
    let hit_rate = (cache_stats.hit_rate * 100.0) as u32;
    let token_stats = state.token_usage_stats.get_stats();
    let active_count = providers.iter().filter(|p| p.enabled).count();
    let total_lookups = cache_stats.hits + cache_stats.misses;

    let tmpl = OverviewTabTemplate {
        total_requests: token_stats.total_requests,
        cache_hit_rate: hit_rate,
        cache_hits: cache_stats.hits,
        cache_lookups: total_lookups,
        active_providers: active_count,
        total_providers: providers.len(),
        tokens_today: token_stats.total_input_tokens + token_stats.total_output_tokens,
        providers,
        cache_entries: state.request_deduplicator.entries_count(),
        cache_capacity: 1000,
        recent_logs,
    };

    Html(
        tmpl.render()
            .unwrap_or_else(|e| format!("Render error: {}", e)),
    )
}

pub async fn tab_providers(State(state): State<Arc<AppState>>) -> Html<String> {
    let providers = build_provider_views(&state).await;
    let tmpl = ProvidersTabTemplate {
        strategy: "Auto".to_string(),
        providers,
    };
    Html(
        tmpl.render()
            .unwrap_or_else(|e| format!("Render error: {}", e)),
    )
}

pub async fn tab_logs(State(state): State<Arc<AppState>>) -> Html<String> {
    let mut available_models = Vec::new();
    for entry in state.providers.iter() {
        let p = entry.value();
        for m in &p.models {
            if !available_models.contains(m) {
                available_models.push(m.clone());
            }
        }
    }
    available_models.sort();

    let recent_logs = get_recent_log_views(&state).await;
    let tmpl = LogsTabTemplate {
        available_models,
        recent_logs,
    };
    Html(
        tmpl.render()
            .unwrap_or_else(|e| format!("Render error: {}", e)),
    )
}

pub async fn toggle_provider(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let views = build_provider_views(&state).await;
    if let Some(p) = views.into_iter().find(|v| v.id == id) {
        let tmpl = ProviderItemTemplate { p };
        Html(tmpl.render().unwrap_or_default())
    } else {
        Html("".to_string())
    }
}

pub async fn reset_provider_breaker(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let views = build_provider_views(&state).await;
    if let Some(p) = views.into_iter().find(|v| v.id == id) {
        let tmpl = ProviderItemTemplate { p };
        Html(tmpl.render().unwrap_or_default())
    } else {
        Html("".to_string())
    }
}

#[derive(Deserialize)]
pub struct PlaygroundForm {
    pub model: String,
    pub prompt: String,
}

pub async fn playground_chat(
    State(state): State<Arc<AppState>>,
    Form(form): Form<PlaygroundForm>,
) -> impl IntoResponse {
    let req = ChatCompletionRequest {
        model: form.model.clone(),
        messages: vec![Message {
            role: "user".to_string(),
            content: form.prompt.clone(),
        }],
        temperature: Some(0.7),
        max_tokens: Some(500),
        stream: Some(false),
    };

    let start = std::time::Instant::now();

    // Internal call - bypass auth by implementing logic inline
    let registry = crate::providers::ProviderRegistry::new(state.providers.clone());
    let router = crate::routing::Router::new(registry, crate::routing::RoutingStrategy::Auto);

    let result = async {
        let provider = router
            .select_provider(&state.db_pool, Some(&req.model))
            .await?;
        let client = std::sync::Arc::new(reqwest::Client::new());
        let provider_client =
            crate::providers::client::ProviderClient::new(client, provider.clone());
        provider_client.chat_completions(&req).await
    }
    .await;

    let elapsed = start.elapsed().as_millis();

    match result {
        Ok(res) => {
            let content = res
                .choices
                .first()
                .map(|c| c.message.content.as_str())
                .unwrap_or("No response");
            format!(
                "[200 OK | Latency: {}ms | Total Tokens: {}]\n\n{}",
                elapsed, res.usage.total_tokens, content
            )
        }
        Err(err) => {
            format!("[Gateway Error: {}]", err)
        }
    }
}

pub async fn sse_events_handler(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<axum::response::sse::Event, Infallible>>> {
    let stream =
        tokio_stream::wrappers::IntervalStream::new(tokio::time::interval(Duration::from_secs(3)))
            .map(move |_| {
                let cache_stats = state.request_deduplicator.get_metrics();
                let hit_rate = (cache_stats.hit_rate * 100.0) as u32;
                let token_stats = state.token_usage_stats.get_stats();
                let active_count = state
                    .providers
                    .iter()
                    .filter(|entry| entry.value().is_available())
                    .count();
                let total_lookups = cache_stats.hits + cache_stats.misses;

                let metrics_tmpl = MetricsStripTemplate {
                    total_requests: token_stats.total_requests,
                    cache_hit_rate: hit_rate,
                    cache_hits: cache_stats.hits,
                    cache_lookups: total_lookups,
                    active_providers: active_count,
                    total_providers: state.providers.len(),
                    tokens_today: token_stats.total_input_tokens + token_stats.total_output_tokens,
                };

                let rendered = metrics_tmpl
                    .render()
                    .unwrap_or_default()
                    .lines()
                    .map(|l| l.trim())
                    .collect::<Vec<_>>()
                    .join("");
                Ok(axum::response::sse::Event::default()
                    .event("metrics")
                    .data(rendered))
            });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(dashboard_index))
        .route("/dashboard", get(dashboard_index))
        .route("/dashboard/tab/overview", get(tab_overview))
        .route("/dashboard/tab/providers", get(tab_providers))
        .route("/dashboard/tab/logs", get(tab_logs))
        .route("/admin/events", get(sse_events_handler))
        .route("/admin/providers/:id/toggle", post(toggle_provider))
        .route("/admin/providers/:id/reset", post(reset_provider_breaker))
        .route("/admin/playground/chat", post(playground_chat))
}
