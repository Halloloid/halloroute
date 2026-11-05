//! Opt-in collective coding orchestration above the normal model-pool path.
//!
//! ## Two modes
//!
//! | Mode       | Description |
//! |------------|-------------|
//! | `parallel` | Original Phase 5.5 behaviour: fan-out to specialist roles → aggregate → critique → synthesize. |
//! | `gan`      | GAN-style loop (inspired by ECC's `gan-generator` / `gan-evaluator` agents): Planner produces a spec, Generator implements, Evaluator scores — iterating until the pass threshold is met or `max_rounds` is exhausted. |
//!
//! ## Usage
//!
//! ```json
//! // Parallel mode (default)
//! { "model": "first-hallo", "collective": true,
//!   "collective_config": { "roles": ["planner","explorer","reviewer"] } }
//!
//! // GAN mode
//! { "model": "first-hallo", "collective": true,
//!   "collective_config": { "mode": "gan", "max_rounds": 3, "pass_threshold": 7.5 } }
//! ```

pub mod agents;
pub mod gan;
pub mod parallel;
pub mod tools;
pub mod types;

// Re-export the public surface used by the rest of the codebase.
pub use tools::validate_tool_call;
pub use types::{CollectiveConfig, CollectiveMode, CollectiveRole, ToolWorkflowState};

use crate::api::v1::chat::{ChatCompletionRequest, ChatCompletionResponse};
use crate::errors::{ApiError, ApiResult};
use crate::middleware::BearerAuth;
use crate::state::AppState;
use std::sync::Arc;

// ── Public entry points ───────────────────────────────────────────────────────

/// Authenticated collective entry point (called from the public chat completions route).
pub async fn run(
    state: &Arc<AppState>,
    auth: &BearerAuth,
    req: &ChatCompletionRequest,
) -> ApiResult<ChatCompletionResponse> {
    if !crate::middleware::validate_api_key_async(&auth.0, &state.db_pool).await? {
        return Err(ApiError::InvalidRequest(
            "Invalid or revoked API key".into(),
        ));
    }
    run_workflow(state, req).await
}

/// Unauthenticated collective entry point (called from internal dashboard routes).
pub async fn run_internal(
    state: &Arc<AppState>,
    req: &ChatCompletionRequest,
) -> ApiResult<ChatCompletionResponse> {
    run_workflow(state, req).await
}

// ── Core orchestration ────────────────────────────────────────────────────────

async fn run_workflow(
    state: &Arc<AppState>,
    req: &ChatCompletionRequest,
) -> ApiResult<ChatCompletionResponse> {
    let mut config = req.collective_config.clone().unwrap_or_default();

    // Pool must exist and have collective intelligence enabled.
    let pool = state.pools.get(&req.model).ok_or_else(|| {
        ApiError::InvalidRequest(format!(
            "collective model '{}' must reference an existing pool",
            req.model
        ))
    })?;

    if !pool.pool.collective_enabled {
        return Err(ApiError::InvalidRequest(format!(
            "collective intelligence is disabled for pool '{}'",
            req.model
        )));
    }

    // If the caller did not explicitly choose a mode (i.e. the request carried
    // the default `Parallel`), use the pool's stored default instead.
    // This means the pool card's default_mode setting is authoritative for
    // API callers that omit `collective_config.mode`.
    if req
        .collective_config
        .as_ref()
        .map(|c| c.mode == CollectiveMode::Parallel)
        .unwrap_or(true)
    {
        config.mode = pool.pool.default_mode;
    }

    // Capability preflight: at least one eligible member must exist.
    let requires_tools = req.tools.as_ref().is_some_and(|t| !t.is_empty());
    let requires_parallel_tools = req.parallel_tool_calls.unwrap_or(false);
    let request_tokens = crate::token::TokenCounter::count_request(req);

    let eligible = pool.pool.members.iter().any(|member| {
        state
            .providers
            .get(&member.provider_id)
            .map(|p| {
                p.is_available()
                    && p.models.iter().any(|m| m == &member.model)
                    && (!requires_tools || p.capabilities.supports_tools)
                    && (!requires_parallel_tools || p.capabilities.supports_parallel_tool_calls)
                    && p.capabilities.context_window >= request_tokens
            })
            .unwrap_or(false)
    });

    drop(pool); // release the Arc<PoolEntry> before any await

    if !eligible {
        return Err(ApiError::InvalidRequest(
            "collective pool has no eligible member for the requested capabilities".into(),
        ));
    }

    config.validate()?;

    let workflow_id = uuid::Uuid::new_v4().to_string();

    tracing::info!(
        workflow_id = %workflow_id,
        model = %req.model,
        mode = ?config.mode,
        "collective_started"
    );

    let result = match config.mode {
        CollectiveMode::Parallel => parallel::run(state, req, &config, &workflow_id).await,
        CollectiveMode::Gan => gan::run(state, req, &config, &workflow_id).await,
    };

    match &result {
        Ok(_) => tracing::info!(workflow_id = %workflow_id, "collective_finished"),
        Err(e) => tracing::error!(workflow_id = %workflow_id, error = %e, "collective_failed"),
    }

    result
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use types::{CollectiveMode, CollectiveRole};

    #[test]
    fn defaults_are_bounded_and_tool_execution_is_opt_in() {
        let cfg = CollectiveConfig::default();
        assert!(cfg.roles.len() <= types::MAX_ROLES);
        assert!(cfg.max_parallel <= types::MAX_PARALLEL);
        assert!(cfg.max_rounds <= types::MAX_ROUNDS);
        assert!(cfg.max_tokens_total <= types::MAX_TOKENS);
        assert!(cfg.timeout_ms <= types::MAX_TIMEOUT_MS);
        assert!(!cfg.tools, "tool execution must be opt-in");
        assert_eq!(cfg.mode, CollectiveMode::Parallel);
    }

    #[test]
    fn excessive_parallelism_is_rejected() {
        let cfg = CollectiveConfig {
            max_parallel: 99,
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn tool_execution_is_blocked() {
        let cfg = CollectiveConfig {
            tools: true,
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn gan_mode_defaults_are_valid() {
        let cfg = CollectiveConfig {
            mode: CollectiveMode::Gan,
            max_rounds: 3,
            pass_threshold: 7.0,
            ..Default::default()
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn invalid_pass_threshold_is_rejected() {
        let cfg = CollectiveConfig {
            mode: CollectiveMode::Gan,
            pass_threshold: 11.0,
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn tool_calls_require_the_expected_state_and_function_name() {
        use serde_json::json;
        use types::ToolWorkflowState;

        let valid = json!({ "function": { "name": "read_file" } });
        assert!(validate_tool_call(ToolWorkflowState::AwaitingCall, &valid).is_ok());

        let wrong_state = validate_tool_call(ToolWorkflowState::CallValidated, &valid);
        assert!(wrong_state.is_err());

        let missing_name = json!({ "function": {} });
        assert!(validate_tool_call(ToolWorkflowState::AwaitingCall, &missing_name).is_err());
    }

    #[test]
    fn all_collective_roles_have_names_and_prompts() {
        let roles = [
            CollectiveRole::Planner,
            CollectiveRole::Explorer,
            CollectiveRole::Architect,
            CollectiveRole::Implementer,
            CollectiveRole::Tester,
            CollectiveRole::Reviewer,
        ];
        for role in &roles {
            assert!(!role.name().is_empty(), "role has no name");
            assert!(
                !role.system_prompt().is_empty(),
                "role '{}' has no system prompt",
                role.name()
            );
        }
    }
}
