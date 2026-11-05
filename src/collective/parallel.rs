//! Parallel-mode orchestration: fan-out → aggregate → critique → synthesize.
//!
//! This preserves the original Phase 5.5 collective intelligence behaviour.

use crate::api::v1::chat::{ChatCompletionRequest, ChatCompletionResponse, Message};
use crate::collective::tools::validate_tool_call;
use crate::collective::types::{CollectiveConfig, RoleArtifact, ToolWorkflowState};
use crate::errors::{ApiError, ApiResult};
use crate::pools::execute_pool_chat;
use crate::state::AppState;
use futures::StreamExt;
use std::sync::Arc;
use std::time::Duration;

pub async fn run(
    state: &Arc<AppState>,
    req: &ChatCompletionRequest,
    config: &CollectiveConfig,
    workflow_id: &str,
) -> ApiResult<ChatCompletionResponse> {
    let pool_entry = state
        .pools
        .get(&req.model)
        .ok_or(ApiError::AllProvidersUnavailable)?;

    let token_budget = (config.max_tokens_total / (config.roles.len() as i32 + 1)).max(256);
    let deadline = Duration::from_millis(config.timeout_ms);

    // ── Round 1: parallel specialist fan-out ─────────────────────────────────
    let role_requests: Vec<(String, ChatCompletionRequest)> = config
        .roles
        .iter()
        .map(|role| {
            let mut messages = vec![Message {
                role: "system".into(),
                content: format!(
                    "You are the {} specialist in a bounded collective coding workflow. \
                     Produce concise, evidence-based analysis. {} \
                     Do not execute tools or claim that changes were made.",
                    role.name(),
                    role.system_prompt()
                ),
                name: Some(role.name().into()),
                tool_call_id: None,
                tool_calls: None,
                extra_content: None,
            }];
            messages.extend(req.messages.clone());
            (
                role.name().to_string(),
                ChatCompletionRequest {
                    model: req.model.clone(),
                    messages,
                    temperature: req.temperature,
                    max_tokens: Some(token_budget),
                    stream: Some(false),
                    tools: None,
                    tool_choice: None,
                    parallel_tool_calls: None,
                    collective: None,
                    collective_config: None,
                },
            )
        })
        .collect();

    let state_clone = Arc::clone(state);
    let role_work = async move {
        let mut results: Vec<RoleArtifact> = Vec::new();
        let mut pending =
            futures::stream::iter(role_requests.into_iter().map(|(role, request)| {
                let s = Arc::clone(&state_clone);
                async move {
                    let entry = s
                        .pools
                        .get(&request.model)
                        .ok_or(ApiError::AllProvidersUnavailable)?;
                    let response = execute_pool_chat(
                        &s,
                        &entry,
                        &request,
                        crate::token::TokenCounter::count_request(&request),
                    )
                    .await?;
                    let content = response
                        .choices
                        .first()
                        .map(|c| c.message.content.clone())
                        .unwrap_or_default();
                    Ok::<_, ApiError>(RoleArtifact { role, content })
                }
            }))
            .buffer_unordered(config.max_parallel);

        while let Some(result) = pending.next().await {
            match result {
                Ok(artifact) => results.push(artifact),
                Err(e) => tracing::warn!(error = %e, "collective_role_failed"),
            }
        }
        Ok::<_, ApiError>(results)
    };

    let results = tokio::time::timeout(deadline, role_work)
        .await
        .map_err(|_| ApiError::RequestTimeout)??;

    if results.is_empty() {
        return Err(ApiError::AllProvidersUnavailable);
    }

    // ── Round 2: aggregation ──────────────────────────────────────────────────
    let aggregate = results
        .iter()
        .map(|a| format!("[{}]\n{}", a.role, a.content))
        .collect::<Vec<_>>()
        .join("\n\n");

    tracing::info!(
        workflow_id = %workflow_id,
        completed_roles = results.len(),
        "collective_aggregate_created"
    );

    // ── Round 3: independent critique ─────────────────────────────────────────
    let critique = if config.review && config.max_rounds >= 2 {
        let critique_req = ChatCompletionRequest {
            model: req.model.clone(),
            messages: vec![
                Message {
                    role: "system".into(),
                    content: "You are an independent reviewer. Challenge the collective report, \
                               identify conflicts, unsupported claims, missing requirements, \
                               and concrete tests needed before implementation."
                        .into(),
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
            max_tokens: Some(token_budget),
            stream: Some(false),
            tools: None,
            tool_choice: None,
            parallel_tool_calls: None,
            collective: None,
            collective_config: None,
        };

        match tokio::time::timeout(
            deadline,
            execute_pool_chat(
                state,
                &pool_entry,
                &critique_req,
                crate::token::TokenCounter::count_request(&critique_req),
            ),
        )
        .await
        {
            Ok(Ok(resp)) => {
                tracing::info!(workflow_id = %workflow_id, "collective_critique_finished");
                resp.choices
                    .first()
                    .map(|c| c.message.content.clone())
                    .unwrap_or_else(|| "No critique returned.".into())
            }
            Ok(Err(e)) => {
                tracing::warn!(workflow_id = %workflow_id, error = %e, "collective_critique_failed");
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

    // ── Round 4: final synthesis ──────────────────────────────────────────────
    let synthesis_prompt = format!(
        "You are the final synthesizer for a coding task. Combine the specialist reports below \
         into one accurate answer. Identify disagreements and uncertainty. Do not claim code was \
         changed or tests were run unless the reports provide evidence. Explain the recommended \
         implementation and concrete next steps.\n\n\
         SPECIALIST REPORTS:\n{}\n\nINDEPENDENT CRITIQUE:\n{}",
        aggregate, critique
    );

    let synthesis_req = ChatCompletionRequest {
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
        max_tokens: Some(token_budget),
        stream: Some(false),
        tools: req.tools.clone(),
        tool_choice: req.tool_choice.clone(),
        parallel_tool_calls: req.tools.as_ref().map(|_| false),
        collective: None,
        collective_config: None,
    };

    let mut response = execute_pool_chat(
        state,
        &pool_entry,
        &synthesis_req,
        crate::token::TokenCounter::count_request(&synthesis_req),
    )
    .await?;

    // Validate any tool calls in the synthesis response.
    if let Some(tool_calls) = response
        .choices
        .first()
        .and_then(|c| c.message.tool_calls.as_ref())
    {
        for tc in tool_calls {
            validate_tool_call(ToolWorkflowState::AwaitingCall, tc)?;
        }
        tracing::info!(
            workflow_id = %workflow_id,
            tool_calls = tool_calls.len(),
            "collective_tool_call_requested"
        );
    }

    response.model = req.model.clone();
    Ok(response)
}
