//! Executes a chat request against a pool: pick an order, try members until one succeeds.
use super::PoolEntry;
use crate::api::v1::chat::{ChatCompletionRequest, ChatCompletionResponse};
use crate::errors::{ApiError, ApiResult};
use crate::providers::client::ProviderClient;
use crate::quota::QuotaManager;
use crate::state::AppState;
use std::sync::Arc;

/// Send `req` to the pool's members in strategy order, falling back to the next member
/// when one is unavailable, over quota, or returns an error. The response's `model`
/// is the member model that actually answered.
pub async fn execute_pool_chat(
    state: &AppState,
    entry: &PoolEntry,
    req: &ChatCompletionRequest,
    estimated_tokens: i64,
) -> ApiResult<ChatCompletionResponse> {
    let client = Arc::new(reqwest::Client::new());
    let mut attempted = 0usize;
    let mut last_error: Option<ApiError> = None;

    for idx in entry.order() {
        let member = &entry.pool.members[idx];

        let provider = state
            .providers
            .get(&member.provider_id)
            .map(|p| p.value().clone());
        let Some(provider) = provider else {
            tracing::debug!(pool = %entry.pool.id, provider_id = %member.provider_id, "Pool member skipped: provider missing");
            continue;
        };
        if !provider.is_available() || !provider.models.contains(&member.model) {
            tracing::debug!(pool = %entry.pool.id, provider_id = %member.provider_id, model = %member.model, "Pool member skipped: unavailable");
            continue;
        }
        match QuotaManager::check_quota(&state.db_pool, &provider, estimated_tokens).await {
            Ok(true) => {}
            Ok(false) => {
                tracing::debug!(pool = %entry.pool.id, provider_id = %provider.id, "Pool member skipped: quota exhausted");
                continue;
            }
            Err(e) => {
                last_error = Some(e);
                continue;
            }
        }

        entry.mark_used(idx);
        attempted += 1;

        let mut member_req = req.clone();
        member_req.model = member.model.clone();
        let provider_client = ProviderClient::new(client.clone(), provider.clone());
        let start_time = std::time::Instant::now();

        match provider_client.chat_completions(&member_req).await {
            Ok(response) => {
                entry.mark_ok(idx);
                let latency_ms = start_time.elapsed().as_millis() as i64;

                let prompt = response.usage.prompt_tokens as i64;
                let completion = response.usage.completion_tokens as i64;
                let total = if prompt + completion > 0 {
                    prompt + completion
                } else {
                    estimated_tokens
                };
                if let Err(e) =
                    QuotaManager::record_usage(&state.db_pool, &provider.id, total).await
                {
                    tracing::warn!(error = %e, "Failed to record pool usage");
                }
                state.token_usage_stats.record_usage(prompt, completion);

                let _ = crate::quota::tracker::log_request(
                    &state.db_pool,
                    &provider.id,
                    &member.model,
                    prompt,
                    completion,
                    total,
                    latency_ms,
                    200,
                )
                .await;

                tracing::info!(
                    pool = %entry.pool.id,
                    provider_id = %provider.id,
                    model = %member.model,
                    attempt = attempted,
                    "Pool request served"
                );
                return Ok(response);
            }
            Err(e) => {
                tracing::warn!(
                    pool = %entry.pool.id,
                    provider_id = %provider.id,
                    model = %member.model,
                    error = %e,
                    "Pool member failed, falling back"
                );
                entry.mark_failed(idx);
                last_error = Some(e);
            }
        }
    }

    match last_error {
        Some(e) => Err(ApiError::ProviderError(format!(
            "All {} attempted member(s) of pool '{}' failed. Last error: {}",
            attempted, entry.pool.id, e
        ))),
        None => Err(ApiError::AllProvidersUnavailable),
    }
}
