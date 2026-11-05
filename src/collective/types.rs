//! Core types shared across the collective module.

use serde::{Deserialize, Serialize};

// ── Server-side hard limits ──────────────────────────────────────────────────
pub const MAX_ROLES: usize = 6;
pub const MAX_PARALLEL: usize = 4;
pub const MAX_ROUNDS: u8 = 5;
pub const MAX_TOKENS: i32 = 32_000;
pub const MAX_TIMEOUT_MS: u64 = 300_000;

// ── Collective operating mode ────────────────────────────────────────────────

/// Which orchestration style to use.
///
/// - `Parallel` (default) – the original fan-out → aggregate → critique → synthesize flow.
/// - `Gan`                – a GAN-style iterative loop: Planner produces a spec, Generator
///                          implements it, Evaluator scores and critiques, Generator refines,
///                          until the score threshold is met or `max_rounds` is exhausted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CollectiveMode {
    #[default]
    Parallel,
    Gan,
}

// ── Specialist roles (Parallel mode) ────────────────────────────────────────

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
    pub fn system_prompt(&self) -> &'static str {
        match self {
            Self::Planner => {
                "Break the coding task into requirements, constraints, acceptance criteria, \
                 and a safe execution plan. Be concise and evidence-based."
            }
            Self::Explorer => {
                "Inspect the supplied codebase context conceptually. Identify relevant files, \
                 symbols, existing patterns, and likely integration points."
            }
            Self::Architect => {
                "Propose an implementation design and explain important trade-offs, \
                 compatibility concerns, and edge cases."
            }
            Self::Implementer => {
                "Describe the concrete implementation changes required. Do not claim files were \
                 changed; provide an evidence-based proposal."
            }
            Self::Tester => {
                "Design focused tests and failure scenarios that would validate or disprove the \
                 proposed solution."
            }
            Self::Reviewer => {
                "Act as an adversarial reliability and security reviewer. Identify missing \
                 requirements, contradictions, unsafe assumptions, and uncertainty."
            }
        }
    }

    pub fn name(&self) -> &'static str {
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

// ── Tool-call state machine ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolWorkflowState {
    AwaitingCall,
    CallValidated,
    AwaitingResult,
    Completed,
    Failed,
}

// ── Configuration ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectiveConfig {
    /// Operating mode. Defaults to `parallel`.
    #[serde(default)]
    pub mode: CollectiveMode,

    // ── Parallel-mode options ────────────────────────────────────────────────
    /// Which specialist roles to fan out to (parallel mode only).
    #[serde(default = "default_roles")]
    pub roles: Vec<CollectiveRole>,

    // ── Shared options ───────────────────────────────────────────────────────
    /// Maximum concurrent upstream calls at once.
    #[serde(default = "default_max_parallel")]
    pub max_parallel: usize,

    /// Maximum workflow rounds.
    ///
    /// - Parallel mode: max aggregate + critique + synthesis rounds (capped at 3).
    /// - GAN mode: maximum Generator → Evaluator refinement iterations.
    #[serde(default = "default_max_rounds")]
    pub max_rounds: u8,

    /// Hard cap on total tokens consumed by this workflow.
    #[serde(default = "default_max_tokens")]
    pub max_tokens_total: i32,

    /// Wall-clock deadline for the entire workflow, in milliseconds.
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,

    /// Whether to run an independent critique pass (parallel mode) or score-based
    /// evaluation loop (GAN mode).
    #[serde(default = "default_true")]
    pub review: bool,

    /// Whether to allow tool-call forwarding (not yet enabled).
    #[serde(default)]
    pub tools: bool,

    // ── GAN-mode options ─────────────────────────────────────────────────────
    /// Minimum weighted score (0.0–10.0) the Evaluator must return for the
    /// Generator's output to be accepted without further refinement.
    #[serde(default = "default_pass_threshold")]
    pub pass_threshold: f32,
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
    16_000
}
fn default_timeout_ms() -> u64 {
    120_000
}
fn default_true() -> bool {
    true
}
fn default_pass_threshold() -> f32 {
    7.0
}

impl Default for CollectiveConfig {
    fn default() -> Self {
        Self {
            mode: CollectiveMode::Parallel,
            roles: default_roles(),
            max_parallel: default_max_parallel(),
            max_rounds: default_max_rounds(),
            max_tokens_total: default_max_tokens(),
            timeout_ms: default_timeout_ms(),
            review: true,
            tools: false,
            pass_threshold: default_pass_threshold(),
        }
    }
}

impl CollectiveConfig {
    pub fn validate(&self) -> crate::errors::ApiResult<()> {
        use crate::errors::ApiError;
        if self.roles.is_empty() || self.roles.len() > MAX_ROLES {
            return Err(ApiError::InvalidRequest(format!(
                "collective roles must contain 1–{} entries",
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
        if !(0.0..=10.0).contains(&self.pass_threshold) {
            return Err(ApiError::InvalidRequest(
                "collective pass_threshold must be between 0.0 and 10.0".into(),
            ));
        }
        Ok(())
    }
}

// ── Intermediate artifacts ───────────────────────────────────────────────────

/// Output produced by one specialist role in parallel mode.
#[derive(Debug, Clone)]
pub struct RoleArtifact {
    pub role: String,
    pub content: String,
}

/// Output produced by the GAN Planner agent.
#[derive(Debug, Clone)]
pub struct PlanArtifact {
    pub spec: String,
    pub rubric: String,
}

/// Output produced by the GAN Generator agent.
#[derive(Debug, Clone)]
pub struct GeneratorArtifact {
    pub iteration: u8,
    pub implementation: String,
}

/// Score and structured feedback from the GAN Evaluator agent.
#[derive(Debug, Clone)]
pub struct EvaluatorArtifact {
    pub iteration: u8,
    /// Weighted score on a 0.0–10.0 scale.
    pub score: f32,
    /// Whether the score meets the pass threshold.
    pub passed: bool,
    /// Full evaluation report with issue list and suggestions.
    pub report: String,
}
