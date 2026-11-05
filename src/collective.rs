//! Opt-in collective coding orchestration above the normal model-pool path.

use crate::api::v1::chat::{ChatCompletionRequest, ChatCompletionResponse, Message};
use crate::errors::{ApiError, ApiResult};
use crate::middleware::BearerAuth;
use crate::pools::execute_pool_chat;
use crate::state::AppState;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

const MAX_ROLES: usize = 6;
const MAX_PARALLEL: usize = 4;
const MAX_ROUNDS: u8 = 3;
const MAX_TOKENS: i32 = 24_000;
const MAX_TIMEOUT_MS: u64 = 180_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectiveRole {
    Planner,
    Explorer,
    Architect,
    Implementer,
    Tester,
    Reviewer,
}

impl CollectiveRole {
    fn prompt(&self) -> &'static str {
        match self {
            Self::Planner => "Break the coding task into requirements, constraints, acceptance criteria, and a safe execution plan.",
            Self::Explorer => "Inspect the supplied codebase context conceptually. Identify relevant files, symbols, existing patterns, and likely integration points.",
            Self::Architect => "Propose an implementation design and explain important trade-offs, compatibility concerns, and edge cases.",
            Self::Implementer => "Describe the concrete implementation changes required. Do not claim files were changed; provide an evidence-based proposal.",
            Self::Tester => "Design focused tests and failure scenarios that would validate or disprove the proposed solution.",
            Self::Reviewer => "Act as an adversarial reliability and security reviewer. Identify missing requirements, contradictions, unsafe assumptions, and uncertainty.",
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Planner => "planner",
            Self::Explorer => "explorer",
            Self::Architect => "architect",
            Self::Implementer => "implementer",
            Self::Tester => "tester",
            Self::Reviewer => "reviewer",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectiveConfig {
    #[serde(default = "default_roles")]
    pub roles: Vec<CollectiveRole>,
    #[serde(default = "default_max_parallel")]
    pub max_parallel: usize,
    #[serde(default = "default_max_rounds")]
    pub max_rounds: u8,
    #[serde(default = "default_max_tokens")]
    pub max_tokens_total: i32,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_true")]
    pub review: bool,
    #[serde(default)]
    pub tools: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolWorkflowState {
    AwaitingCall,
    CallValidated,
    AwaitingResult,
    Completed,
    Failed,
}

/// Validates a proposed tool call without executing workspace mutations.
pub fn validate_tool_call(
    state: ToolWorkflowState,
    tool_call: &serde_json::Value,
) -> ApiResult<ToolWorkflowState> {
    if state != ToolWorkflowState::AwaitingCall {
        return Err(ApiError::InvalidRequest(
            "tool call is not valid in the current collective workflow state".into(),
        ));
    }
    let name = tool_call
        .get("function")
        .and_then(|function| function.get("name"))
        .and_then(serde_json::Value::as_str)
        .filter(|name| !name.is_empty());
    if name.is_none() {
        return Err(ApiError::InvalidRequest(
            "collective tool call must include function.name".into(),
        ));
    }
    Ok(ToolWorkflowState::CallValidated)
}

fn default_roles() -> Vec<CollectiveRole> {
    vec![
        CollectiveRole::Planner,
        CollectiveRole::Explorer,
        CollectiveRole::Reviewer,
    ]
}
fn default_max_parallel() -> usize {
    3
}
fn default_max_rounds() -> u8 {
    3
}
fn default_max_tokens() -> i32 {
    12_000
}
fn default_timeout_ms() -> u64 {
    120_000
}
fn default_true() -> bool {
    true
}

impl Default for CollectiveConfig {
    fn default() -> Self {
        Self {
            roles: default_roles(),
            max_parallel: default_max_parallel(),
            max_rounds: default_max_rounds(),
            max_tokens_total: default_max_tokens(),
            timeout_ms: default_timeout_ms(),
            review: true,
            tools: false,
        }
    }
}

impl CollectiveConfig {
    fn validate(&self) -> ApiResult<()> {
        if self.roles.is_empty() || self.roles.len() > MAX_ROLES {
            return Err(ApiError::InvalidRequest(format!(
                "collective roles must contain 1-{} entries",
                MAX_ROLES
            )));
        }
        if self.max_parallel == 0 || self.max_parallel > MAX_PARALLEL {
            return Err(ApiError::InvalidRequest(format!(
                "collective max_parallel must be between 1 and {}",
                MAX_PARALLEL
            )));
        }
        if self.max_rounds == 0 || self.max_rounds > MAX_ROUNDS {
            return Err(ApiError::InvalidRequest(format!(
                "collective max_rounds must be between 1 and {}",
                MAX_ROUNDS
            )));
        }
        if self.max_tokens_total <= 0 || self.max_tokens_total > MAX_TOKENS {
            return Err(ApiError::InvalidRequest(format!(
                "collective max_tokens_total must be between 1 and {}",
                MAX_TOKENS
            )));
        }
        if self.timeout_ms == 0 || self.timeout_ms > MAX_TIMEOUT_MS {
            return Err(ApiError::InvalidRequest(format!(
                "collective timeout_ms must be between 1 and {}",
                MAX_TIMEOUT_MS
            )));
        }
        if self.tools {
            return Err(ApiError::InvalidRequest(
                "collective tool execution is not enabled yet; use tools=false".into(),
            ));
        }
        Ok(())
    }
}

struct RoleResult {
    role: String,
    content: String,
}

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

/// Runs an explicitly requested collective workflow from an authenticated
/// internal dashboard route.
pub async fn run_internal(
    state: &Arc<AppState>,
    req: &ChatCompletionRequest,
) -> ApiResult<ChatCompletionResponse> {
    run_workflow(state, req).await
}

async fn run_workflow(
    state: &Arc<AppState>,
    req: &ChatCompletionRequest,
) -> ApiResult<ChatCompletionResponse> {
    let config = req.collective_config.clone().unwrap_or_default();
    config.validate()?;
    if state.pools.get(&req.model).is_none() {
        return Err(ApiError::InvalidRequest(format!(
            "collective model '{}' must reference an existing pool",
            req.model
        )));
    }
    let pool = state.pools.get(&req.model).unwrap();
    let requires_tools = req.tools.as_ref().is_some_and(|tools| !tools.is_empty());
    let requires_parallel_tools = req.parallel_tool_calls.unwrap_or(false);
    let eligible = pool.pool.members.iter().any(|member| {
        state
            .providers
            .get(&member.provider_id)
            .map(|provider| {
                provider.is_available()
                    && provider.models.iter().any(|model| model == &member.model)
                    && (!requires_tools || provider.capabilities.supports_tools)
                    && (!requires_parallel_tools
                        || provider.capabilities.supports_parallel_tool_calls)
                    && provider.capabilities.context_window
                        >= crate::token::TokenCounter::count_request(req)
            })
            .unwrap_or(false)
    });
    if !eligible {
        return Err(ApiError::InvalidRequest(
            "collective pool has no eligible member for the requested capabilities".into(),
        ));
    }

    let workflow_id = uuid::Uuid::new_v4().to_string();
    tracing::info!(
        workflow_id = %workflow_id,
        model = %req.model,
        roles = config.roles.len(),
        "collective_started"
    );

    let role_budget = (config.max_tokens_total / (config.roles.len() as i32 + 1)).max(256);
    let mut role_requests = Vec::with_capacity(config.roles.len());
    for role in &config.roles {
        let mut messages = vec![Message {
            role: "system".into(),
            content: format!(
                "You are the {} specialist in a bounded collective coding workflow. \
                 Produce concise, evidence-based analysis. {} \
                 Do not execute tools or claim that changes were made.",
                role.name(),
                role.prompt()
            ),
            name: Some(role.name().into()),
            tool_call_id: None,
            tool_calls: None,
            extra_content: None,
        }];
        messages.extend(req.messages.clone());
        role_requests.push((
            role.name().to_string(),
            ChatCompletionRequest {
                model: req.model.clone(),
                messages,
                temperature: req.temperature,
                max_tokens: Some(role_budget),
                stream: Some(false),
                tools: None,
                tool_choice: None,
                parallel_tool_calls: None,
                collective: None,
                collective_config: None,
            },
        ));
    }

    let state_for_roles = Arc::clone(state);
    let deadline = Duration::from_millis(config.timeout_ms);
    let role_work = async move {
        let mut results = Vec::new();
        let mut pending =
            futures::stream::iter(role_requests.into_iter().map(|(role, request)| {
                let state = Arc::clone(&state_for_roles);
                async move {
                    let Some(entry) = state.pools.get(&request.model) else {
                        return Err(ApiError::AllProvidersUnavailable);
                    };
                    let response = execute_pool_chat(
                        &state,
                        &entry,
                        &request,
                        crate::token::TokenCounter::count_request(&request),
                    )
                    .await?;
                    let content = response
                        .choices
                        .first()
                        .map(|choice| choice.message.content.clone())
                        .unwrap_or_default();
                    Ok(RoleResult { role, content })
                }
            }))
            .buffer_unordered(config.max_parallel);
        while let Some(result) = pending.next().await {
            match result {
                Ok(result) => results.push(result),
                Err(error) => tracing::warn!(error = %error, "collective_role_failed"),
            }
        }
        Ok::<Vec<RoleResult>, ApiError>(results)
    };

    let results = tokio::time::timeout(deadline, role_work)
        .await
        .map_err(|_| ApiError::RequestTimeout)??;
    if results.is_empty() {
        return Err(ApiError::AllProvidersUnavailable);
    }

    let aggregate = results
        .iter()
        .map(|result| format!("[{}]\n{}", result.role, result.content))
        .collect::<Vec<_>>()
        .join("\n\n");
    tracing::info!(
        workflow_id = %workflow_id,
        completed_roles = results.len(),
        "collective_aggregate_created"
    );

    let critique = if config.review && config.max_rounds >= 2 {
        let critique_request = ChatCompletionRequest {
            model: req.model.clone(),
            messages: vec![
                Message {
                    role: "system".into(),
                    content: "You are an independent reviewer. Challenge the collective report, identify conflicts, unsupported claims, missing requirements, and concrete tests needed before implementation.".into(),
                    name: Some("reviewer".into()),
                    tool_call_id: None,
                    tool_calls: None,
                    extra_content: None,
                },
                Message {
                    role: "user".into(),
                    content: format!("COLLECTIVE REPORT:\n{}", aggregate),
                    name: None,
                    tool_call_id: None,
                    tool_calls: None,
                    extra_content: None,
                },
            ],
            temperature: req.temperature,
            max_tokens: Some(role_budget),
            stream: Some(false),
            tools: None,
            tool_choice: None,
            parallel_tool_calls: None,
            collective: None,
            collective_config: None,
        };
        let entry = state
            .pools
            .get(&req.model)
            .ok_or(ApiError::AllProvidersUnavailable)?;
        match tokio::time::timeout(
            deadline,
            execute_pool_chat(
                state,
                &entry,
                &critique_request,
                crate::token::TokenCounter::count_request(&critique_request),
            ),
        )
        .await
        {
            Ok(Ok(response)) => {
                tracing::info!(workflow_id = %workflow_id, "collective_critique_finished");
                response
                    .choices
                    .first()
                    .map(|choice| choice.message.content.clone())
                    .unwrap_or_else(|| "No critique was returned.".into())
            }
            Ok(Err(error)) => {
                tracing::warn!(workflow_id = %workflow_id, error = %error, "collective_critique_failed");
                "Critique failed; treat the collective report as incomplete.".into()
            }
            Err(_) => {
                tracing::warn!(workflow_id = %workflow_id, "collective_critique_timed_out");
                "Critique timed out; treat the collective report as incomplete.".into()
            }
        }
    } else {
        "Critique was disabled by configuration.".into()
    };

    let synthesis_prompt = format!(
        "You are the final synthesizer for a coding task. Combine the specialist reports below \
         into one accurate answer. Identify disagreements and uncertainty. Do not claim code was \
         changed or tests were run unless the reports provide evidence. Explain the recommended \
         implementation and concrete next steps.\n\nSPECIALIST REPORTS:\n{}\n\nINDEPENDENT CRITIQUE:\n{}",
        aggregate, critique
    );
    let synthesis_request = ChatCompletionRequest {
        model: req.model.clone(),
        messages: vec![
            Message {
                role: "system".into(),
                content: "Synthesize a reliable, concise coding response from specialist evidence."
                    .into(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
                extra_content: None,
            },
            Message {
                role: "user".into(),
                content: synthesis_prompt,
                name: None,
                tool_call_id: None,
                tool_calls: None,
                extra_content: None,
            },
        ],
        temperature: req.temperature,
        max_tokens: Some(role_budget),
        stream: Some(false),
        tools: req.tools.clone(),
        tool_choice: req.tool_choice.clone(),
        parallel_tool_calls: req.tools.as_ref().map(|_| false),
        collective: None,
        collective_config: None,
    };
    let entry = state
        .pools
        .get(&req.model)
        .ok_or(ApiError::AllProvidersUnavailable)?;
    let mut response = execute_pool_chat(
        state,
        &entry,
        &synthesis_request,
        crate::token::TokenCounter::count_request(&synthesis_request),
    )
    .await?;
    if let Some(tool_calls) = response
        .choices
        .first()
        .and_then(|choice| choice.message.tool_calls.as_ref())
    {
        for tool_call in tool_calls {
            validate_tool_call(ToolWorkflowState::AwaitingCall, tool_call)?;
        }
        tracing::info!(
            workflow_id = %workflow_id,
            tool_calls = tool_calls.len(),
            "collective_tool_call_requested"
        );
    }
    response.model = req.model.clone();
    tracing::info!(
        workflow_id = %workflow_id,
        completed_roles = results.len(),
        "collective_finished"
    );
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_bounded_and_tool_execution_is_opt_in() {
        let config = CollectiveConfig::default();
        assert!(config.validate().is_ok());
        assert!(!config.tools);
    }

    #[test]
    fn excessive_parallelism_is_rejected() {
        let config = CollectiveConfig {
            max_parallel: MAX_PARALLEL + 1,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn tool_calls_require_the_expected_state_and_function_name() {
        let call = serde_json::json!({
            "id": "call-1",
            "function": {"name": "read_file", "arguments": "{}"}
        });
        assert_eq!(
            validate_tool_call(ToolWorkflowState::AwaitingCall, &call).unwrap(),
            ToolWorkflowState::CallValidated
        );
        assert!(validate_tool_call(ToolWorkflowState::AwaitingResult, &call).is_err());
        assert!(
            validate_tool_call(ToolWorkflowState::AwaitingCall, &serde_json::json!({})).is_err()
        );
    }
}
