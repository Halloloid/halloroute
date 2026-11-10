use askama::Template;
use axum::{
    extract::{Path, Query, State},
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
    /// Added through the dashboard/API (editable and removable)
    pub dynamic: bool,
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
    pub memory_mb: String,
    pub providers: Vec<ProviderView>,
    pub cache_entries: usize,
    pub cache_capacity: usize,
    pub recent_logs: Vec<LogView>,
}

#[derive(Template)]
#[template(path = "tabs/providers.html")]
pub struct ProvidersTabTemplate {
    pub strategy: String,
    pub total_providers: usize,
    pub active_providers: usize,
    pub down_providers: usize,
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
    pub gateway_keys: Vec<crate::gateway_keys::GatewayKeyInfo>,
}

#[derive(Template)]
#[template(path = "tabs/quotas.html")]
pub struct QuotasTabTemplate {
    pub tokens_today: i64,
    pub selected_range: String,
    pub active_providers: usize,
    pub total_providers: usize,
    pub providers: Vec<ProviderView>,
    pub recent_logs: Vec<LogView>,
}

#[derive(Template)]
#[template(path = "tabs/client_setup.html")]
pub struct ClientSetupTabTemplate {
    /// Active (non-revoked) gateway keys. Only prefixes are known; full keys are shown once.
    pub gateway_keys: Vec<crate::gateway_keys::GatewayKeyInfo>,
    /// Model ids served by currently healthy providers (sorted, de-duplicated)
    pub models: Vec<String>,
    pub base_url: String,
}

/// Base URL clients should use to reach this gateway.
pub const GATEWAY_BASE_URL: &str = "http://127.0.0.1:20128/v1";

pub struct PoolMemberView {
    pub provider_id: String,
    pub model: String,
    pub weight: u32,
    /// Provider currently reachable/healthy
    pub healthy: bool,
}

pub struct PoolView {
    pub id: String,
    pub strategy_label: String,
    pub members: Vec<PoolMemberView>,
    pub collective_enabled: bool,
    /// "Parallel" or "GAN" — for display in the pool card
    pub default_mode_label: String,
    /// "parallel" or "gan" — for data attributes in the pool card JS
    pub default_mode_value: String,
}

#[derive(Template)]
#[template(path = "tabs/pools.html")]
pub struct PoolsTabTemplate {
    pub pools: Vec<PoolView>,
    pub candidates: Vec<crate::api::admin::pools::Candidate>,
    /// Pools as JSON for the edit form (already escaped for use inside <script>)
    pub pools_json: String,
}

#[derive(Template)]
#[template(path = "tabs/collective.html")]
pub struct CollectiveTabTemplate {
    pub pools: Vec<String>,
    pub collective_pools: Vec<String>, // pools with collective_enabled = true
}

#[derive(Template)]
#[template(path = "tabs/provider_form.html")]
pub struct ProviderFormTemplate {
    pub editing: bool,
    pub provider_id: String,
    pub name: String,
    pub api_endpoint: String,
    pub auth_type: String,
    pub has_api_key: bool,
    pub rate_limit_rpm: i32,
    pub free_quota_daily: String,
    pub free_quota_monthly: String,
    /// "all", "patterns" or "explicit"
    pub filter_type: String,
    pub filter_args: String,
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
    pub memory_mb: String,
}

#[derive(Template)]
#[template(path = "partials/log_row.html")]
pub struct LogRowTemplate {
    pub log: LogView,
}

async fn build_provider_views(state: &AppState) -> Vec<ProviderView> {
    let dynamic_ids: std::collections::HashSet<String> =
        crate::providers::list_dynamic_providers(&state.db_pool)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|r| r.id)
            .collect();
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
            dynamic: dynamic_ids.contains(id),
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

pub fn get_current_rss_mb() -> f64 {
    #[cfg(windows)]
    {
        use std::mem::MaybeUninit;

        #[repr(C)]
        #[allow(non_snake_case)]
        struct PROCESS_MEMORY_COUNTERS {
            cb: u32,
            PageFaultCount: u32,
            PeakWorkingSetSize: usize,
            WorkingSetSize: usize,
            QuotaPeakPagedPoolUsage: usize,
            QuotaPagedPoolUsage: usize,
            QuotaPeakNonPagedPoolUsage: usize,
            QuotaNonPagedPoolUsage: usize,
            PagefileUsage: usize,
            PeakPagefileUsage: usize,
        }

        #[link(name = "psapi")]
        extern "system" {
            fn GetCurrentProcess() -> isize;
            fn GetProcessMemoryInfo(
                hProcess: isize,
                ppsmemCounters: *mut PROCESS_MEMORY_COUNTERS,
                cb: u32,
            ) -> i32;
        }

        unsafe {
            let mut counters = MaybeUninit::<PROCESS_MEMORY_COUNTERS>::uninit();
            let c = counters.as_mut_ptr();
            (*c).cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;

            if GetProcessMemoryInfo(GetCurrentProcess(), c, (*c).cb) != 0 {
                let info = counters.assume_init();
                // Use PagefileUsage (Commit Size/Private Bytes) instead of WorkingSetSize
                // to match the "Memory" column in Windows Task Manager.
                return info.PagefileUsage as f64 / (1024.0 * 1024.0);
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
            let parts: Vec<&str> = statm.split_whitespace().collect();
            if let Some(rss_str) = parts.get(1) {
                if let Ok(rss_pages) = rss_str.parse::<f64>() {
                    let page_size = 4096.0;
                    return (rss_pages * page_size) / (1024.0 * 1024.0);
                }
            }
        }
    }

    14.2
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
        memory_mb: format!("{:.1}", get_current_rss_mb()),
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
        memory_mb: format!("{:.1}", get_current_rss_mb()),
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
    let active_count = providers.iter().filter(|p| p.enabled).count();
    let down_count = providers.len() - active_count;
    let tmpl = ProvidersTabTemplate {
        strategy: "Auto".to_string(),
        total_providers: providers.len(),
        active_providers: active_count,
        down_providers: down_count,
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

    let gateway_keys: Vec<_> = crate::gateway_keys::GatewayKeyManager::list_keys(&state.db_pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|k| k.is_active)
        .collect();

    let tmpl = LogsTabTemplate {
        available_models,
        recent_logs,
        gateway_keys,
    };
    Html(
        tmpl.render()
            .unwrap_or_else(|e| format!("Render error: {}", e)),
    )
}

#[derive(Debug, Deserialize)]
pub struct QuotaRangeQuery {
    pub range: Option<String>,
}

pub async fn tab_quotas(
    State(state): State<Arc<AppState>>,
    Query(query): Query<QuotaRangeQuery>,
) -> Html<String> {
    let providers = build_provider_views(&state).await;
    let recent_logs = get_recent_log_views(&state).await;
    let token_stats = state.token_usage_stats.get_stats();
    let active_count = providers.iter().filter(|p| p.enabled).count();
    let selected_range = match query.range.as_deref() {
        Some("3d") => "3d",
        Some("7d") => "7d",
        Some("1y") => "1y",
        Some("all") => "all",
        _ => "1d",
    }
    .to_string();
    let days = match selected_range.as_str() {
        "3d" => Some(3),
        "7d" => Some(7),
        "1y" => Some(365),
        "all" => None,
        _ => Some(1),
    };
    let tokens_today = crate::quota::tracker::get_total_tokens_since(&state.db_pool, days)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, "Failed to load quota token total");
            token_stats.total_input_tokens + token_stats.total_output_tokens
        });

    let tmpl = QuotasTabTemplate {
        tokens_today,
        selected_range,
        active_providers: active_count,
        total_providers: providers.len(),
        providers,
        recent_logs,
    };
    Html(
        tmpl.render()
            .unwrap_or_else(|e| format!("Render error: {}", e)),
    )
}

pub async fn tab_client_setup(State(state): State<Arc<AppState>>) -> Html<String> {
    let gateway_keys: Vec<_> = crate::gateway_keys::GatewayKeyManager::list_keys(&state.db_pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|k| k.is_active)
        .collect();

    let mut models: Vec<String> = state
        .providers
        .iter()
        .filter(|e| e.value().is_available())
        .flat_map(|e| e.value().models.clone())
        .collect();
    models.sort();
    models.dedup();

    let tmpl = ClientSetupTabTemplate {
        gateway_keys,
        models,
        base_url: GATEWAY_BASE_URL.to_string(),
    };
    Html(
        tmpl.render()
            .unwrap_or_else(|e| format!("Render error: {}", e)),
    )
}

pub async fn tab_pools(State(state): State<Arc<AppState>>) -> Html<String> {
    let raw = state.pools.list();
    let pools_json = serde_json::to_string(&raw)
        .unwrap_or_else(|_| "[]".to_string())
        .replace("</", "<\\/");

    let pools = raw
        .into_iter()
        .map(|p| PoolView {
            strategy_label: p.strategy.label().to_string(),
            members: p
                .members
                .into_iter()
                .map(|m| PoolMemberView {
                    healthy: state
                        .providers
                        .get(&m.provider_id)
                        .is_some_and(|prov| prov.is_available()),
                    provider_id: m.provider_id,
                    model: m.model,
                    weight: m.weight,
                })
                .collect(),
            collective_enabled: p.collective_enabled,
            default_mode_label: match p.default_mode {
                crate::collective::CollectiveMode::Gan => "GAN".to_string(),
                crate::collective::CollectiveMode::Parallel => "Parallel".to_string(),
            },
            default_mode_value: match p.default_mode {
                crate::collective::CollectiveMode::Gan => "gan".to_string(),
                crate::collective::CollectiveMode::Parallel => "parallel".to_string(),
            },
            id: p.id,
        })
        .collect();

    let tmpl = PoolsTabTemplate {
        pools,
        candidates: crate::api::admin::pools::pool_candidate_list(&state),
        pools_json,
    };
    Html(
        tmpl.render()
            .unwrap_or_else(|e| format!("Render error: {}", e)),
    )
}

pub async fn add_provider_form() -> Html<String> {
    let tmpl = ProviderFormTemplate {
        editing: false,
        provider_id: String::new(),
        name: String::new(),
        api_endpoint: String::new(),
        auth_type: "bearer".to_string(),
        has_api_key: false,
        rate_limit_rpm: 30,
        free_quota_daily: String::new(),
        free_quota_monthly: String::new(),
        filter_type: "all".to_string(),
        filter_args: String::new(),
    };
    Html(
        tmpl.render()
            .unwrap_or_else(|e| format!("Render error: {}", e)),
    )
}

pub async fn edit_provider_form(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Html<String> {
    let Ok(record) = crate::providers::get_dynamic_provider(&state.db_pool, &id).await else {
        return Html("<p class=\"pm-note\">Provider not found</p>".to_string());
    };

    let (filter_type, filter_args) = match record.free_model_filter.split_once(':') {
        Some((t, args)) if t == "patterns" || t == "explicit" => (t.to_string(), args.to_string()),
        _ => ("all".to_string(), String::new()),
    };

    let tmpl = ProviderFormTemplate {
        editing: true,
        provider_id: record.id,
        name: record.name,
        api_endpoint: record.api_endpoint,
        auth_type: record.auth_type,
        has_api_key: record.api_key.as_deref().is_some_and(|k| !k.is_empty()),
        rate_limit_rpm: record.rate_limit_rpm,
        free_quota_daily: record
            .free_quota_daily
            .map(|v| v.to_string())
            .unwrap_or_default(),
        free_quota_monthly: record
            .free_quota_monthly
            .map(|v| v.to_string())
            .unwrap_or_default(),
        filter_type,
        filter_args,
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
    if let Some(mut p) = state.providers.get_mut(&id) {
        p.health_status = if p.is_available() {
            ProviderHealth::Down
        } else {
            ProviderHealth::Healthy
        };
    }
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
    if let Some(mut p) = state.providers.get_mut(&id) {
        p.health_status = ProviderHealth::Healthy;
    }
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

#[derive(Deserialize)]
pub struct CollectivePlaygroundForm {
    pub model: String,
    pub prompt: String,
    // mode
    #[serde(default)]
    pub mode: String, // "parallel" | "gan"
    // shared
    #[serde(default)]
    pub max_rounds: Option<u8>,
    #[serde(default)]
    pub max_tokens_total: Option<i32>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub review: Option<String>, // "on" when checkbox ticked
    // parallel-mode roles (checkboxes → multiple values)
    #[serde(default)]
    pub roles: Vec<String>,
    // gan-mode
    #[serde(default)]
    pub pass_threshold: Option<f32>,
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
            name: None,
            tool_call_id: None,
            tool_calls: None,
            extra_content: None,
        }],
        temperature: Some(0.7),
        max_tokens: Some(500),
        stream: Some(false),
        tools: None,
        tool_choice: None,
        parallel_tool_calls: None,
        collective: None,
        collective_config: None,
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

pub async fn collective_playground_chat(
    State(state): State<Arc<AppState>>,
    Form(form): Form<CollectivePlaygroundForm>,
) -> impl IntoResponse {
    use crate::collective::{CollectiveConfig, CollectiveMode, CollectiveRole};

    // ── Build CollectiveConfig from form fields ───────────────────────────────
    let mode = match form.mode.as_str() {
        "gan" => CollectiveMode::Gan,
        _ => CollectiveMode::Parallel,
    };

    let roles: Vec<CollectiveRole> = if form.roles.is_empty() {
        // default roles
        vec![
            CollectiveRole::Planner,
            CollectiveRole::Explorer,
            CollectiveRole::Reviewer,
        ]
    } else {
        form.roles
            .iter()
            .filter_map(|r| match r.as_str() {
                "planner" => Some(CollectiveRole::Planner),
                "explorer" => Some(CollectiveRole::Explorer),
                "architect" => Some(CollectiveRole::Architect),
                "implementer" => Some(CollectiveRole::Implementer),
                "tester" => Some(CollectiveRole::Tester),
                "reviewer" => Some(CollectiveRole::Reviewer),
                _ => None,
            })
            .collect()
    };

    let config = CollectiveConfig {
        mode,
        roles,
        max_rounds: form.max_rounds.unwrap_or(3).min(5).max(1),
        max_tokens_total: form.max_tokens_total.unwrap_or(16_000).min(32_000).max(1),
        timeout_ms: form.timeout_ms.unwrap_or(120_000).min(300_000).max(1),
        review: form.review.as_deref() == Some("on"),
        tools: false,
        pass_threshold: form.pass_threshold.unwrap_or(7.0).clamp(0.0, 10.0),
        max_parallel: 3,
    };

    let req = ChatCompletionRequest {
        model: form.model,
        messages: vec![Message {
            role: "user".to_string(),
            content: form.prompt,
            name: None,
            tool_call_id: None,
            tool_calls: None,
            extra_content: None,
        }],
        temperature: Some(0.7),
        max_tokens: Some(config.max_tokens_total),
        stream: Some(false),
        tools: None,
        tool_choice: None,
        parallel_tool_calls: None,
        collective: Some(true),
        collective_config: Some(config),
    };

    let start = std::time::Instant::now();
    let result = crate::collective::run_internal(&state, &req).await;
    let elapsed = start.elapsed().as_millis();

    match result {
        Ok(response) => {
            let content = response
                .choices
                .first()
                .map(|c| c.message.content.clone())
                .unwrap_or_else(|| "Collective workflow returned no content.".to_string());
            format!(
                "[200 OK | {}ms | {} tokens]\n\n{}",
                elapsed, response.usage.total_tokens, content
            )
        }
        Err(error) => format!("[Collective workflow error: {}]", error),
    }
}

pub async fn tab_collective(State(state): State<Arc<AppState>>) -> Html<String> {
    let all_pools = state.pools.list();
    let mut pools: Vec<String> = all_pools.iter().map(|p| p.id.clone()).collect();
    pools.sort();
    let mut collective_pools: Vec<String> = all_pools
        .into_iter()
        .filter(|p| p.collective_enabled)
        .map(|p| p.id)
        .collect();
    collective_pools.sort();
    let tmpl = CollectiveTabTemplate {
        pools,
        collective_pools,
    };
    Html(
        tmpl.render()
            .unwrap_or_else(|e| format!("Render error: {}", e)),
    )
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
                    memory_mb: format!("{:.1}", get_current_rss_mb()),
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
        .route("/dashboard/tab/quotas", get(tab_quotas))
        .route("/dashboard/tab/client-setup", get(tab_client_setup))
        .route("/dashboard/tab/pools", get(tab_pools))
        .route("/dashboard/tab/collective", get(tab_collective))
        .route("/dashboard/tabs/add-provider-form", get(add_provider_form))
        .route(
            "/dashboard/tabs/edit-provider-form/:id",
            get(edit_provider_form),
        )
        .route("/admin/events", get(sse_events_handler))
        .route("/admin/providers/:id/toggle", post(toggle_provider))
        .route("/admin/providers/:id/reset", post(reset_provider_breaker))
        .route("/admin/playground/chat", post(playground_chat))
        .route(
            "/admin/playground/collective",
            post(collective_playground_chat),
        )
}
