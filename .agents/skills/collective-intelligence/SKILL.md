---
name: collective-intelligence
description: >
  Two-mode collective intelligence orchestration for HalloRoute: the original
  parallel specialist fan-out (Parallel mode) and a GAN-style iterative
  Planner → Generator ⇄ Evaluator loop (GAN mode). Both modes route through
  the existing model pool — whichever models are configured do the work.
tags: [collective, gan, multi-agent, orchestration, rust]
---

# Collective Intelligence — Skill Reference

## Overview

`src/collective/` is an **opt-in orchestration layer** that sits above the
normal HalloRoute model pools.  Clients activate it by sending
`"collective": true` in a chat completion request.

Two modes are available, selected via `collective_config.mode`:

| Mode | Behaviour |
|------|-----------|
| `parallel` (default) | Fan out to specialist roles concurrently → aggregate → critique → synthesize |
| `gan` | Planner produces a spec and rubric → Generator implements → Evaluator scores → iterate until pass or max_rounds |

Both modes use your existing provider pools.  No external agents, no hardcoded
models — **the pool models do all the work**.

---

## Module Layout

```
src/collective/
├── mod.rs       Public entry points (run, run_internal), pool validation,
│                mode dispatch, module-level tests
├── types.rs     CollectiveConfig, CollectiveMode, CollectiveRole, all artifact structs
├── tools.rs     ToolWorkflowState, validate_tool_call (state machine + tests)
├── agents.rs    run_planner, run_generator, run_evaluator (GAN agents)
├── parallel.rs  Original fan-out → aggregate → critique → synthesize
└── gan.rs       GAN orchestration loop
```

---

## Parallel Mode

```json
{
  "model": "first-hallo",
  "collective": true,
  "collective_config": {
    "mode": "parallel",
    "roles": ["planner", "explorer", "architect", "reviewer"],
    "max_parallel": 3,
    "max_rounds": 3,
    "max_tokens_total": 16000,
    "timeout_ms": 120000,
    "review": true
  },
  "messages": [{"role": "user", "content": "Refactor the auth module"}]
}
```

### Workflow

```
Round 0  Intake: validate config, pool eligibility, generate workflow_id
Round 1  Parallel fan-out: each role gets a focused system prompt + original messages
Round 2  Aggregation: join all role responses into a compact report
Round 3  Critique: an independent reviewer challenges the report
Round 4  Synthesis: one final answer combining report + critique
```

### Available Roles

| Role | System prompt focus |
|------|-------------------|
| `planner` | Requirements, constraints, acceptance criteria, execution plan |
| `explorer` | Relevant files, symbols, existing patterns, integration points |
| `architect` | Implementation design, trade-offs, edge cases |
| `implementer` | Concrete changes required (evidence-based proposal, not claims) |
| `tester` | Focused tests and failure scenarios |
| `reviewer` | Adversarial reliability + security review |

---

## GAN Mode

Inspired by ECC's `gan-planner`, `gan-generator`, `gan-evaluator` agent
pattern (`github.com/affaan-m/ecc`).  Instead of Claude sub-agents, each role
is a **system-prompt injection** into the existing pool.

```json
{
  "model": "first-hallo",
  "collective": true,
  "collective_config": {
    "mode": "gan",
    "max_rounds": 3,
    "pass_threshold": 7.5,
    "max_tokens_total": 24000,
    "timeout_ms": 180000,
    "review": true
  },
  "messages": [{"role": "user", "content": "Add JWT authentication"}]
}
```

### Workflow

```
Round 0 — Planner
  Input:  user messages
  Output: PlanArtifact { spec, rubric }
          (delimited by ---SPEC--- and ---RUBRIC--- tags)

Round 1…max_rounds — Generator + Evaluator loop
  Generator input:   spec + [prev implementation] + [evaluator feedback]
  Generator output:  GeneratorArtifact { implementation }
                     (delimited by ---IMPLEMENTATION--- and ---SUMMARY--- tags)

  Evaluator input:   spec + rubric + implementation
  Evaluator output:  EvaluatorArtifact { score 0-10, passed, report }
                     (weighted: Correctness×0.35 + Completeness×0.25
                               + CodeQuality×0.25 + Security×0.15)

  If score >= pass_threshold  OR  iteration == max_rounds → stop loop

Round N+1 — Final Synthesis
  Input:  spec + best implementation + final evaluator report
  Output: One coherent OpenAI-compatible ChatCompletionResponse
```

### Pass threshold

`pass_threshold` (default `7.0`) is the minimum weighted score on a 0–10 scale
that the Evaluator must return for the loop to stop early.  If it is never
reached the best-scoring iteration is used for synthesis.

### Token budget allocation

```
phase_count = 2 + (max_rounds × 2)  // plan + (gen+eval)×rounds + synth
phase_budget = max_tokens_total / phase_count  (minimum 512)
synthesis    = phase_budget × 2               (synthesis gets extra budget)
```

---

## Configuration Reference

```rust
pub struct CollectiveConfig {
    pub mode: CollectiveMode,        // "parallel" | "gan"
    pub roles: Vec<CollectiveRole>,  // parallel only; default [planner, explorer, reviewer]
    pub max_parallel: usize,         // 1–4; default 3
    pub max_rounds: u8,              // 1–5; default 3
    pub max_tokens_total: i32,       // 1–32000; default 16000
    pub timeout_ms: u64,             // 1–300000; default 120000
    pub review: bool,                // enable critique/evaluator; default true
    pub tools: bool,                 // tool execution; default false (not yet enabled)
    pub pass_threshold: f32,         // 0.0–10.0; GAN mode only; default 7.0
}
```

### Server-side hard limits (always enforced)

| Field | Max |
|-------|-----|
| roles | 6 |
| max_parallel | 4 |
| max_rounds | 5 |
| max_tokens_total | 32 000 |
| timeout_ms | 300 000 |

---

## Enabling a Pool for Collective Mode

The pool must have `collective_enabled: true`.  This prevents accidental
activation on pools not designed for multi-call fan-out.

```json
{
  "id": "first-hallo",
  "strategy": "round_robin",
  "collective_enabled": true,
  "members": [
    { "provider_id": "groq",       "model": "llama-3.1-70b-versatile" },
    { "provider_id": "openrouter", "model": "meta-llama/llama-3.1-8b-instruct:free" }
  ]
}
```

---

## Observability — Structured Log Events

All events are emitted via `tracing::info!` / `tracing::warn!` with a
`workflow_id` correlation field.

| Event | Mode | Description |
|-------|------|-------------|
| `collective_started` | both | Workflow begins; includes mode |
| `collective_aggregate_created` | parallel | Specialist results joined |
| `collective_critique_finished` | parallel | Critique round complete |
| `collective_tool_call_requested` | parallel | Tool calls in synthesis |
| `gan_planner_started/finished` | gan | Planner run |
| `gan_generator_started/finished` | gan | Generator iteration |
| `gan_evaluator_started/finished` | gan | Evaluator iteration + score |
| `gan_pass_threshold_met` | gan | Early exit; score met threshold |
| `gan_synthesis_started` | gan | Final synthesis starts |
| `gan_finished` | gan | Final score logged |
| `collective_finished` | both | Workflow complete |
| `collective_failed` | both | Workflow error |
| `collective_role_failed` | parallel | One specialist failed (non-fatal) |

---

## Testing Checklist

- [ ] `CollectiveConfig::default()` validates without error
- [ ] `max_parallel > MAX_PARALLEL` is rejected
- [ ] `tools: true` is rejected
- [ ] `pass_threshold > 10.0` is rejected
- [ ] GAN defaults validate
- [ ] All 6 `CollectiveRole` variants have non-empty `name()` and `system_prompt()`
- [ ] `validate_tool_call` rejects wrong state, empty names
- [ ] Planner output parses `---SPEC---` / `---RUBRIC---` tags correctly
- [ ] Generator output parses `---IMPLEMENTATION---` / `---SUMMARY---` tags
- [ ] Evaluator `WEIGHTED_SCORE` line is parsed; score is clamped to 0–10
- [ ] Non-collective requests to the same pool remain unchanged

---

## Common Patterns

### Testing with a mock pool response

```rust
// Unit-test the parser without a real provider call:
let raw = "---SPEC---\nBuild X\n---RUBRIC---\nScore Y";
let (spec, rubric) = parse_planner_output(raw);  // exposed in agents::tests
assert_eq!(spec, "Build X");
```

### Checking score from evaluator output

```rust
let raw = "WEIGHTED_SCORE: 8.2\nVERDICT: PASS";
let (score, _report) = parse_evaluator_output(raw);
assert!((score - 8.2).abs() < f32::EPSILON);
```
