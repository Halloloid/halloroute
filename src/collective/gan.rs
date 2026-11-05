//! GAN-mode orchestration: Planner → Generator ⇄ Evaluator (iterative loop).
//!
//! Inspired by the ECC GAN harness (github.com/affaan-m/ecc) but adapted for
//! HalloRoute's model-pool architecture: instead of Claude sub-agents, each
//! GAN role is a focused system-prompt injected into the existing pool pipeline.
//!
//! ## Workflow
//!
//! ```text
//! Round 0 — Planner:
//!   User request → PlanArtifact { spec, rubric }
//!
//! Round 1…N — Generator + Evaluator loop:
//!   PlanArtifact + [prev implementation] + [evaluator feedback]
//!       → GeneratorArtifact { implementation }
//!       → EvaluatorArtifact  { score, report }
//!   If score >= pass_threshold  OR  max_rounds exhausted → stop
//!
//! Round N+1 — Final synthesis:
//!   Best GeneratorArtifact + final EvaluatorArtifact
//!       → One coherent OpenAI-compatible ChatCompletionResponse
//! ```

use crate::api::v1::chat::{ChatCompletionRequest, ChatCompletionResponse, Message};
use crate::collective::agents::{run_evaluator, run_generator, run_planner};
use crate::collective::types::{CollectiveConfig, EvaluatorArtifact, GeneratorArtifact};
use crate::errors::{ApiError, ApiResult};
use crate::pools::execute_pool_chat;
use crate::state::AppState;
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

    let deadline = Duration::from_millis(config.timeout_ms);
    // Divide the total token budget across workflow phases.
    // Planner gets 1 share, each Generator + Evaluator pair gets 2 shares,
    // and the final synthesis gets 1 share.
    let phase_count = 2 + (config.max_rounds as i32 * 2); // plan + (gen+eval)*rounds + synth
    let phase_budget = (config.max_tokens_total / phase_count).max(512);

    // ── Round 0: Planner ──────────────────────────────────────────────────────
    tracing::info!(workflow_id = %workflow_id, "gan_planner_started");

    let plan = tokio::time::timeout(
        deadline,
        run_planner(
            state,
            &pool_entry,
            &req.messages,
            phase_budget,
            req.temperature,
        ),
    )
    .await
    .map_err(|_| ApiError::RequestTimeout)??;

    tracing::info!(workflow_id = %workflow_id, "gan_planner_finished");

    // ── Rounds 1…N: Generator ⇄ Evaluator loop ────────────────────────────────
    let mut prev_generator: Option<GeneratorArtifact> = None;
    let mut prev_evaluator: Option<EvaluatorArtifact> = None;
    let mut best_generator: Option<GeneratorArtifact> = None;
    let mut best_evaluator: Option<EvaluatorArtifact> = None;
    let mut best_score: f32 = -1.0;

    for iteration in 1..=config.max_rounds {
        tracing::info!(
            workflow_id = %workflow_id,
            iteration = %iteration,
            "gan_generator_started"
        );

        // Generator
        let generator = tokio::time::timeout(
            deadline,
            run_generator(
                state,
                &pool_entry,
                &req.messages,
                &plan,
                prev_generator.as_ref(),
                prev_evaluator.as_ref(),
                iteration,
                phase_budget,
                req.temperature,
            ),
        )
        .await
        .map_err(|_| ApiError::RequestTimeout)??;

        tracing::info!(
            workflow_id = %workflow_id,
            iteration = %iteration,
            "gan_generator_finished"
        );

        // Evaluator (if review is enabled)
        if config.review {
            tracing::info!(
                workflow_id = %workflow_id,
                iteration = %iteration,
                "gan_evaluator_started"
            );

            let evaluator = tokio::time::timeout(
                deadline,
                run_evaluator(
                    state,
                    &pool_entry,
                    &plan,
                    &generator,
                    iteration,
                    config.pass_threshold,
                    phase_budget,
                    req.temperature,
                ),
            )
            .await
            .map_err(|_| ApiError::RequestTimeout)??;

            tracing::info!(
                workflow_id = %workflow_id,
                iteration = %iteration,
                score = %evaluator.score,
                passed = %evaluator.passed,
                "gan_evaluator_finished"
            );

            if evaluator.score > best_score {
                best_score = evaluator.score;
                best_generator = Some(generator.clone());
                best_evaluator = Some(evaluator.clone());
            }

            let passed = evaluator.passed;
            prev_evaluator = Some(evaluator);
            prev_generator = Some(generator);

            if passed {
                tracing::info!(
                    workflow_id = %workflow_id,
                    iteration = %iteration,
                    score = %best_score,
                    "gan_pass_threshold_met"
                );
                break;
            }
        } else {
            // No review: accept first generator output and stop.
            best_generator = Some(generator);
            break;
        }
    }

    let final_generator = best_generator.ok_or_else(|| {
        ApiError::InvalidRequest("GAN workflow produced no generator output".into())
    })?;

    // ── Final synthesis ───────────────────────────────────────────────────────
    tracing::info!(
        workflow_id = %workflow_id,
        final_score = %best_score,
        "gan_synthesis_started"
    );

    let synthesis_context =
        build_synthesis_context(&plan.spec, &final_generator, best_evaluator.as_ref());

    let synthesis_req = ChatCompletionRequest {
        model: req.model.clone(),
        messages: vec![
            Message {
                role: "system".into(),
                content: "\
You are the final synthesizer in a GAN-style multi-agent coding workflow.\n\
Your job is to produce ONE coherent, accurate response from the GAN workflow artifacts.\n\
\n\
Rules:\n\
- Present the Generator's final implementation clearly.\n\
- Summarize what was built and why it satisfies the spec.\n\
- Note any remaining issues identified by the Evaluator.\n\
- Do NOT add fictional features or claim tests were run that weren't.\n\
- Be concise: users want the answer, not the process narrative."
                    .into(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
                extra_content: None,
            },
            Message {
                role: "user".into(),
                content: synthesis_context,
                name: None,
                tool_call_id: None,
                tool_calls: None,
                extra_content: None,
            },
        ],
        temperature: req.temperature,
        max_tokens: Some(phase_budget * 2), // synthesis gets a larger budget
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

    response.model = req.model.clone();

    tracing::info!(
        workflow_id = %workflow_id,
        final_score = %best_score,
        "gan_finished"
    );

    Ok(response)
}

fn build_synthesis_context(
    spec: &str,
    generator: &GeneratorArtifact,
    evaluator: Option<&EvaluatorArtifact>,
) -> String {
    let mut parts = vec![
        format!("PLANNER SPEC:\n{}", spec),
        format!(
            "FINAL IMPLEMENTATION (iteration {}):\n{}",
            generator.iteration, generator.implementation
        ),
    ];

    if let Some(eval) = evaluator {
        parts.push(format!(
            "EVALUATOR FINAL SCORE: {:.1}/10 ({})\n\nEVALUATOR REPORT:\n{}",
            eval.score,
            if eval.passed {
                "PASSED"
            } else {
                "FAILED — best available"
            },
            eval.report
        ));
    }

    parts.push(
        "Synthesize the above into one clear, accurate final answer for the user.".to_string(),
    );

    parts.join("\n\n")
}
