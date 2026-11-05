//! GAN-mode agents: Planner, Generator, and Evaluator.
//!
//! Each agent is a thin wrapper that:
//!   1. Builds a focused system + user message set.
//!   2. Calls the pool via `execute_pool_chat`.
//!   3. Returns a typed artifact.
//!
//! The Planner runs once.  Generator and Evaluator iterate until the Evaluator's
//! score meets `pass_threshold` or `max_rounds` is exhausted.

use crate::api::v1::chat::{ChatCompletionRequest, Message};
use crate::collective::types::{EvaluatorArtifact, GeneratorArtifact, PlanArtifact};
use crate::errors::ApiResult;
use crate::pools::{execute_pool_chat, PoolEntry};
use crate::state::AppState;
use std::sync::Arc;

// ── helpers ──────────────────────────────────────────────────────────────────

fn extract_content(resp: &crate::api::v1::chat::ChatCompletionResponse) -> String {
    resp.choices
        .first()
        .map(|c| c.message.content.clone())
        .unwrap_or_default()
}

fn make_request(
    model: &str,
    system: impl Into<String>,
    user: impl Into<String>,
    max_tokens: i32,
    temperature: Option<f32>,
) -> ChatCompletionRequest {
    ChatCompletionRequest {
        model: model.to_string(),
        messages: vec![
            Message {
                role: "system".into(),
                content: system.into(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
                extra_content: None,
            },
            Message {
                role: "user".into(),
                content: user.into(),
                name: None,
                tool_call_id: None,
                tool_calls: None,
                extra_content: None,
            },
        ],
        temperature,
        max_tokens: Some(max_tokens),
        stream: Some(false),
        tools: None,
        tool_choice: None,
        parallel_tool_calls: None,
        collective: None,
        collective_config: None,
    }
}

// ── Planner ──────────────────────────────────────────────────────────────────

/// The Planner turns the user's original messages into a structured product spec
/// and an evaluation rubric.  It runs once at the start of the GAN workflow.
pub async fn run_planner(
    state: &Arc<AppState>,
    entry: &Arc<PoolEntry>,
    original_messages: &[Message],
    token_budget: i32,
    temperature: Option<f32>,
) -> ApiResult<PlanArtifact> {
    let task = original_messages
        .iter()
        .filter(|m| m.role == "user")
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");

    let system = "\
You are the **Planner** in a GAN-style multi-agent coding workflow.\n\
Your job is to take the user's request and produce two documents:\n\
\n\
1. **SPEC** — A precise product/implementation specification with:\n\
   - Clear goal and context\n\
   - Must-have requirements (numbered)\n\
   - Should-have requirements (numbered)\n\
   - Acceptance criteria for each must-have\n\
   - Known constraints and edge cases\n\
   - Technical stack and approach direction\n\
\n\
2. **RUBRIC** — A weighted evaluation rubric with exactly four criteria:\n\
   - Correctness (weight 0.35): Does the implementation satisfy all must-have requirements?\n\
   - Completeness (weight 0.25): Are edge cases, error states, and constraints handled?\n\
   - Code Quality (weight 0.25): Is the code idiomatic, readable, and well-structured?\n\
   - Security & Reliability (weight 0.15): Are there obvious safety or reliability concerns?\n\
\n\
Format your output EXACTLY as:\n\
---SPEC---\n\
<spec content>\n\
---RUBRIC---\n\
<rubric content>\n\
\n\
Be ambitious but realistic. Conservative planning leads to underwhelming results.";

    let user = format!(
        "USER REQUEST:\n{}\n\nProduce the SPEC and RUBRIC now.",
        task
    );

    let req = make_request(&entry.pool.id, system, user, token_budget, temperature);
    let resp = execute_pool_chat(
        state,
        entry,
        &req,
        crate::token::TokenCounter::count_request(&req),
    )
    .await?;

    let content = extract_content(&resp);
    let (spec, rubric) = parse_planner_output(&content);
    Ok(PlanArtifact { spec, rubric })
}

fn parse_planner_output(raw: &str) -> (String, String) {
    let spec_tag = "---SPEC---";
    let rubric_tag = "---RUBRIC---";

    let spec = if let (Some(s), Some(r)) = (raw.find(spec_tag), raw.find(rubric_tag)) {
        raw[s + spec_tag.len()..r].trim().to_string()
    } else {
        // fallback: treat whole output as spec
        raw.trim().to_string()
    };

    let rubric = if let Some(r) = raw.find(rubric_tag) {
        raw[r + rubric_tag.len()..].trim().to_string()
    } else {
        "Evaluate correctness, completeness, code quality, and reliability.".to_string()
    };

    (spec, rubric)
}

// ── Generator ────────────────────────────────────────────────────────────────

/// The Generator implements (or refines) the solution based on the Planner's spec
/// and any Evaluator feedback from the previous iteration.
pub async fn run_generator(
    state: &Arc<AppState>,
    entry: &Arc<PoolEntry>,
    original_messages: &[Message],
    plan: &PlanArtifact,
    previous_implementation: Option<&GeneratorArtifact>,
    evaluator_feedback: Option<&EvaluatorArtifact>,
    iteration: u8,
    token_budget: i32,
    temperature: Option<f32>,
) -> ApiResult<GeneratorArtifact> {
    let task = original_messages
        .iter()
        .filter(|m| m.role == "user")
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");

    let system = "\
You are the **Generator** in a GAN-style multi-agent coding workflow.\n\
Your role is to produce high-quality, complete implementation code or solutions.\n\
\n\
Core principles:\n\
- Read the spec carefully — every must-have requirement must be addressed.\n\
- If you received evaluator feedback, fix EVERY issue listed as critical or major.\n\
- Produce real, runnable implementation — not pseudocode, not placeholders.\n\
- Clearly label your implementation section with ---IMPLEMENTATION---.\n\
- After the implementation, add ---SUMMARY--- with a brief list of what you built/changed.\n\
- Do NOT claim changes you didn't make. Do NOT skip required features.\n\
- Code quality: idiomatic, well-named, properly error-handled.";

    let mut context_parts = vec![
        format!("USER REQUEST:\n{}", task),
        format!("PLANNER SPEC:\n{}", plan.spec),
    ];

    if let Some(prev) = previous_implementation {
        context_parts.push(format!(
            "YOUR PREVIOUS IMPLEMENTATION (iteration {}):\n{}",
            prev.iteration, prev.implementation
        ));
    }

    if let Some(eval) = evaluator_feedback {
        context_parts.push(format!(
            "EVALUATOR FEEDBACK (score {:.1}/10 — {}):\n{}",
            eval.score,
            if eval.passed { "PASSED" } else { "FAILED" },
            eval.report
        ));
        context_parts.push(
            "IMPORTANT: Address every Critical and Major issue listed above before anything else."
                .to_string(),
        );
    }

    let action = if previous_implementation.is_some() {
        format!(
            "Refine your implementation for iteration {}. Fix all evaluator issues.",
            iteration
        )
    } else {
        format!(
            "Produce the initial implementation for iteration {}.",
            iteration
        )
    };
    context_parts.push(action);

    let user = context_parts.join("\n\n");
    let req = make_request(&entry.pool.id, system, user, token_budget, temperature);
    let resp = execute_pool_chat(
        state,
        entry,
        &req,
        crate::token::TokenCounter::count_request(&req),
    )
    .await?;

    let content = extract_content(&resp);
    let implementation = parse_generator_output(&content);
    Ok(GeneratorArtifact {
        iteration,
        implementation,
    })
}

fn parse_generator_output(raw: &str) -> String {
    let impl_tag = "---IMPLEMENTATION---";
    if let Some(pos) = raw.find(impl_tag) {
        // include everything after the tag, trimming the summary if present
        let after = &raw[pos + impl_tag.len()..];
        let summary_tag = "---SUMMARY---";
        if let Some(s) = after.find(summary_tag) {
            after[..s].trim().to_string()
        } else {
            after.trim().to_string()
        }
    } else {
        raw.trim().to_string()
    }
}

// ── Evaluator ────────────────────────────────────────────────────────────────

/// The Evaluator reviews the Generator's implementation against the Planner's spec
/// and rubric, assigns a weighted score, and writes structured feedback.
pub async fn run_evaluator(
    state: &Arc<AppState>,
    entry: &Arc<PoolEntry>,
    plan: &PlanArtifact,
    generator: &GeneratorArtifact,
    iteration: u8,
    pass_threshold: f32,
    token_budget: i32,
    temperature: Option<f32>,
) -> ApiResult<EvaluatorArtifact> {
    let system = "\
You are the **Evaluator** in a GAN-style multi-agent coding workflow.\n\
Your role is to ruthlessly but fairly evaluate the Generator's implementation.\n\
\n\
You are NOT here to be encouraging. You are here to find every flaw and shortcut.\n\
\n\
Scoring (0–10 per criterion):\n\
- 1–3: Broken or embarrassing — does not meet basic requirements\n\
- 4–5: Functional but incomplete — tutorial-quality, missing important features\n\
- 6:   Decent but unremarkable — missing polish or edge-case handling\n\
- 7:   Good — solid work, meets requirements, minor rough edges\n\
- 8:   Very good — professional quality, well-structured\n\
- 9:   Excellent — polished, handles edge cases, idiomatic\n\
- 10:  Exceptional — could ship as-is, nothing to improve\n\
\n\
Output format EXACTLY:\n\
---SCORES---\n\
Correctness: X/10\n\
Completeness: X/10\n\
Code Quality: X/10\n\
Security & Reliability: X/10\n\
WEIGHTED_SCORE: X.X\n\
VERDICT: PASS or FAIL\n\
---ISSUES---\n\
Critical Issues (must fix):\n\
1. [issue] → [how to fix]\n\
Major Issues (should fix):\n\
1. [issue] → [how to fix]\n\
Minor Issues (nice to fix):\n\
1. [issue] → [how to fix]\n\
---SUGGESTIONS---\n\
1. [concrete, actionable suggestion]\n\
\n\
Weighted score formula: (Correctness*0.35) + (Completeness*0.25) + (CodeQuality*0.25) + (Security*0.15)\n\
PASS threshold: 7.0";

    let user = format!(
        "PLANNER SPEC:\n{}\n\nEVALUATION RUBRIC:\n{}\n\nGENERATOR IMPLEMENTATION (iteration {}):\n{}\n\nEvaluate the implementation now.",
        plan.spec, plan.rubric, generator.iteration, generator.implementation
    );

    let req = make_request(&entry.pool.id, system, user, token_budget, temperature);
    let resp = execute_pool_chat(
        state,
        entry,
        &req,
        crate::token::TokenCounter::count_request(&req),
    )
    .await?;

    let content = extract_content(&resp);
    let (score, report) = parse_evaluator_output(&content);
    let passed = score >= pass_threshold;

    Ok(EvaluatorArtifact {
        iteration,
        score,
        passed,
        report,
    })
}

/// Extracts the weighted score and full report from the evaluator's raw output.
fn parse_evaluator_output(raw: &str) -> (f32, String) {
    // Try to find the WEIGHTED_SCORE line.
    let score = raw
        .lines()
        .find_map(|line| {
            let line = line.trim();
            if line.starts_with("WEIGHTED_SCORE:") {
                line.trim_start_matches("WEIGHTED_SCORE:")
                    .trim()
                    .parse::<f32>()
                    .ok()
            } else {
                None
            }
        })
        .unwrap_or_else(|| {
            // Fallback: average any "X/10" lines we can find
            let scores: Vec<f32> = raw
                .lines()
                .filter_map(|l| {
                    let l = l.trim();
                    if let Some(pos) = l.rfind('/') {
                        let after = l[pos + 1..].trim();
                        if after.starts_with("10") {
                            let before = l[..pos].rsplit(':').next().unwrap_or("").trim();
                            before.parse::<f32>().ok()
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect();
            if scores.is_empty() {
                5.0 // conservative default
            } else {
                scores.iter().sum::<f32>() / scores.len() as f32
            }
        })
        .clamp(0.0, 10.0);

    (score, raw.trim().to_string())
}

// ── unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planner_output_is_split_correctly() {
        let raw = "---SPEC---\nBuild a thing\n---RUBRIC---\nScore it well";
        let (spec, rubric) = parse_planner_output(raw);
        assert_eq!(spec, "Build a thing");
        assert_eq!(rubric, "Score it well");
    }

    #[test]
    fn planner_fallback_on_no_tags() {
        let raw = "Just a plain spec";
        let (spec, _rubric) = parse_planner_output(raw);
        assert_eq!(spec, "Just a plain spec");
    }

    #[test]
    fn generator_implementation_is_extracted() {
        let raw = "preamble\n---IMPLEMENTATION---\nfn main() {}\n---SUMMARY---\nDid stuff";
        let impl_ = parse_generator_output(raw);
        assert_eq!(impl_, "fn main() {}");
    }

    #[test]
    fn evaluator_score_is_parsed() {
        let raw = "---SCORES---\nCorrectness: 8/10\nWEIGHTED_SCORE: 7.5\nVERDICT: PASS";
        let (score, _) = parse_evaluator_output(raw);
        assert!((score - 7.5).abs() < f32::EPSILON);
    }

    #[test]
    fn evaluator_score_is_clamped() {
        let raw = "WEIGHTED_SCORE: 15.0";
        let (score, _) = parse_evaluator_output(raw);
        assert!((score - 10.0).abs() < f32::EPSILON);
    }
}
