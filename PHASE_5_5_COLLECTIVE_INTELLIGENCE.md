# Phase 5.5 — HalloRoute Collective Intelligence

## Status

**Complete — bounded collective gateway orchestration implemented**

### Implemented baseline

- Explicit `collective: true` request activation.
- Server-side validation for roles, parallelism, rounds, token budget, timeout, and tool mode.
- Parallel specialist execution through the existing model pool.
- Planner, explorer, architect, implementer, tester, and reviewer roles.
- Aggregated evidence and final synthesis through the existing pool.
- Independent critique round before final synthesis.
- Explicit tool workflow states and tool-call validation.
- Collective request/cache-field separation.
- Correlation logging for workflow start, aggregation, role failures, and completion.
- JSON and buffered SSE-compatible final responses.

### Design boundary

HalloRoute does not directly mutate a workspace. It validates collective tool-call requests, forces a single tool-call lane, preserves tool metadata and finish reasons, and routes the client's tool-result continuation through the normal pool path. The connected coding client remains the single workspace writer.

Phase 5.5 introduces an opt-in collective coding layer above the existing HalloRoute provider pools. Multiple economical models collaborate as specialized coding agents while the current pool behavior remains unchanged for ordinary requests.

## Problem

The current pool load-balances and falls back between provider models, but one model produces each answer. Complex coding tasks benefit from independent planning, codebase exploration, architecture analysis, implementation reasoning, testing, security review, and synthesis.

The objective is to determine whether a bounded collective of free-tier models can produce more reliable coding results than one model while controlling latency, quota usage, context size, and tool safety.

## Goals

- Add an explicit opt-in collective coding mode.
- Reuse existing provider discovery, pools, quotas, authentication, retries, caching, and SSE.
- Assign bounded specialist roles to eligible pool members.
- Run independent analysis in parallel where safe.
- Aggregate results into a compact working brief.
- Add independent critique and final synthesis.
- Preserve OpenAI tool-call and streaming compatibility.
- Support controlled coding-tool execution.
- Provide correlation logs and workflow status.
- Degrade gracefully when members fail or exceed limits.
- Compare collective and ordinary pool quality, cost, and latency.

## Non-goals

- Do not make collective mode the default initially.
- Do not change the existing pool balancing/fallback contract.
- Do not allow multiple agents to mutate the workspace concurrently.
- Do not assume every provider supports tools, streaming, structured output, or long context.
- Do not persist prompts or source code unnecessarily.
- Do not claim completion when stages failed or only produced proposals.

## User-facing contract

Existing requests remain unchanged:

```json
{
  "model": "first-hallo",
  "messages": []
}
```

Collective mode should be explicitly activated. Preferred initial design:

```json
{
  "model": "first-hallo",
  "collective": true,
  "collective_config": {
    "roles": ["planner", "explorer", "reviewer"],
    "max_parallel": 3,
    "max_rounds": 3,
    "max_tokens_total": 12000,
    "timeout_ms": 120000,
    "review": true,
    "tools": false
  },
  "messages": []
}
```

The collective fields are gateway-only and must not be forwarded to upstream providers. Server-side limits always override client values.

Safe configuration limits must cover:

- maximum specialist roles;
- maximum parallel calls;
- maximum workflow rounds;
- maximum total tokens;
- maximum wall-clock duration;
- maximum tool iterations;
- maximum workspace actions.

Invalid or excessive configuration must produce a clear validation error or follow documented clamping rules. It must never create an unbounded workflow.

## Architecture

### Existing responsibilities

- **Provider registry:** provider availability, models, dynamic providers.
- **Pool registry:** member selection, balancing, cooldowns, and fallback.
- **Provider client:** normalized requests, bearer authentication, tool forwarding, and streaming.
- **Quota manager:** quota preflight and usage recording.
- **Cache:** ordinary request caching with collective keys kept separate.
- **API/auth:** existing Bearer authentication and OpenAI-compatible routes.

### New collective module

Add a dedicated `collective` module with testable responsibilities:

- `types.rs` — roles, configuration, budgets, workflow states, artifacts, outcomes.
- `orchestrator.rs` — bounded workflow state machine and round execution.
- `roles.rs` — role definitions and role-specific instructions.
- `context.rs` — task summaries, artifact selection, and context budgeting.
- `aggregate.rs` — result normalization, duplicate removal, conflicts, and evidence.
- `synthesis.rs` — critique and final-answer prompts.
- `tools.rs` — controlled tool-call continuation state machine.

The exact file layout may follow existing project conventions, but orchestration must remain separate from ordinary pool execution.

## Workflow

### Round 0 — Intake

1. Authenticate and validate the request.
2. Detect explicit collective mode.
3. Generate request and workflow correlation IDs.
4. Estimate tokens and reserve a bounded workflow budget.
5. Resolve the target pool.
6. Filter members by required capabilities.
7. Build a compact task summary.
8. Select roles according to the request and server limits.

### Round 1 — Parallel specialist analysis

Run independent roles concurrently up to `max_parallel`:

- **Planner:** requirements, constraints, acceptance criteria, and task decomposition.
- **Codebase explorer:** relevant files, symbols, call paths, tests, and existing patterns.
- **Architect:** designs, trade-offs, and migration concerns.
- **Implementation analyst:** concrete changes, edge cases, and integration points.
- **Test analyst:** targeted tests and failure scenarios.
- **Security/reliability reviewer:** unsafe assumptions, leaks, injection risks, and recovery gaps.

Specialists should produce structured artifacts containing findings, assumptions, evidence references, risks, proposed changes, and confidence.

### Round 2 — Aggregation

1. Normalize specialist outputs.
2. Remove duplicate findings.
3. Group disagreements.
4. Prefer claims backed by file paths, symbols, tests, or request evidence.
5. Build a compact working brief.
6. Record failed or incomplete roles explicitly.

### Round 3 — Critique

Independent reviewers challenge the working brief:

- Are requirements missing?
- Are proposed changes consistent with the codebase?
- Are tool calls necessary and safe?
- Could the design break streaming, auth, quotas, or compatibility?
- What tests could disprove the proposal?

Critique must identify contradictions and uncertainty instead of merely repeating the aggregate.

### Round 4 — Controlled implementation

For coding-agent workflows:

1. The implementation agent proposes a tool action.
2. The orchestrator validates its scope and parameters.
3. One controlled executor performs the action.
4. The result becomes a tool message and workflow artifact.
5. The agent continues with the result.
6. Reviewers inspect the changed state.

The initial release must use a single writer for workspace mutations. If implementation is text-only, the response must clearly label it as a proposal.

### Round 5 — Final synthesis

The synthesizer receives the compact brief, critique, implementation/test results, and explicit failures. It returns one coherent assistant response containing:

- conclusions or completed changes;
- relevant files and evidence;
- checks or tests performed;
- unresolved risks and uncertainty;
- no unsupported completion claims.

Intermediate specialist responses must not appear as separate assistant turns to OpenAI-compatible clients.

## Tool calls and streaming

The gateway must preserve:

- request `tools`;
- `tool_choice`;
- `parallel_tool_calls`;
- assistant `tool_calls`;
- `tool_call_id` tool-result messages;
- streamed tool-call deltas;
- provider `finish_reason`;
- `finish_reason: "tool_calls"` when tool calls are emitted.

The state machine must distinguish text completion, tool request, tool result continuation, final completion, and failure.

The first implementation should buffer internal specialist output and stream only the final synthesis. If workflow status events are later exposed, they require a documented event shape that cannot confuse OpenAI chat clients.

The final SSE response must:

1. emit an assistant role chunk;
2. emit content or tool-call deltas in order;
3. preserve the provider finish reason;
4. emit an explicit error if interrupted;
5. emit `[DONE]` exactly once;
6. cancel upstream work after client disconnect.

## Capability filtering

Providers or pool members need capability metadata, configured or discovered:

- `supports_tools`;
- `supports_streaming`;
- `supports_structured_output`;
- `context_window`;
- `supports_parallel_tool_calls`;
- health/reliability status.

Collective mode must filter incompatible members before fan-out. If no eligible member remains, return a clear missing-capability error.

## Budgets, quotas, and cancellation

Each workflow requires:

- specialist count limit;
- parallelism limit;
- round limit;
- input/output token limit;
- wall-clock deadline;
- tool-iteration limit;
- workspace-action limit;
- quota preflight and usage recording.

Request disconnects and deadline expiry must cancel internal work. If one member fails, continue only when enough evidence remains; otherwise return an explicit partial or failed result.

Usage must be tracked by workflow ID, role, provider, model, and attempt. Retries must not double-count the same artifact accidentally.

## Context and memory

- Do not send the full conversation to every specialist by default.
- Build compact task summaries and role-specific context.
- Include selected file excerpts and artifacts only when relevant.
- Enforce member context limits.
- Redact keys, headers, and sensitive values from logs.
- Keep initial workflow state in memory with correlation IDs.
- Consider SQLite workflow metadata only after the state machine is stable.

## Observability

Emit structured lifecycle events:

- `collective_started`;
- `collective_role_started`;
- `collective_role_finished`;
- `collective_role_failed`;
- `collective_aggregate_created`;
- `collective_critique_finished`;
- `collective_tool_call_requested`;
- `collective_tool_call_completed`;
- `collective_synthesis_started`;
- `collective_finished`;
- `collective_cancelled`.

Include workflow ID, request ID, role, provider, model, round, latency, token estimate, and status. Never log full prompts, API keys, or raw source content by default.

Dashboard summaries may expose active workflows, role counts, failures, latency, token use, and completion state without secrets.

## API and dashboard

Add dashboard controls for:

- enabling collective mode;
- selecting a pool/profile;
- selecting roles;
- setting bounded budgets;
- showing eligible provider capabilities;
- showing recent workflow status;
- showing failures and partial results;
- showing quota/token impact.

Admin endpoints must validate all configuration and never expose stored provider secrets.

## Testing

### Unit tests

- request/config deserialization and validation;
- server-side budget caps;
- role selection;
- capability filtering;
- context compaction;
- artifact aggregation and conflict grouping;
- cancellation and timeout transitions;
- tool-call state machine;
- finish-reason preservation;
- cache-key separation.

### Mock-provider tests

Cover successful text, streamed tool calls, malformed SSE, timeout, 429/5xx, unsupported tools, partial role failure, conflicting results, and client disconnect.

### Integration tests

Verify normal pool behavior remains unchanged, collective fan-out obeys limits, per-role fallback works, quota is recorded, tool results continue correctly, synthesis returns one valid OpenAI-compatible response, SSE preserves finish reasons and `[DONE]`, and logs contain correlation events without secrets.

### Quality evaluation

Compare normal pool and collective mode on fixed coding tasks:

- correctness;
- repository evidence accuracy;
- tool-call correctness;
- tests suggested or executed;
- regression rate;
- latency;
- token/quota use;
- failure recovery;
- final-answer usefulness.

## Rollout

1. Implement analysis-only collective mode behind explicit opt-in.
2. Add mock-provider tests and lifecycle observability.
3. Add critique and synthesis quality checks.
4. Add controlled single-writer tool execution.
5. Add dashboard configuration and workflow summaries.
6. Run comparative evaluation.
7. Enable selected profiles by default only after quality and quota evidence.

## Risks and mitigations

| Risk | Mitigation |
|---|---|
| Quota multiplication | Hard budgets, concurrency caps, and preflight checks |
| Increased latency | Parallel independent roles, deadlines, and reduced role sets |
| Groupthink | Independent prompts, evidence requirements, and adversarial critique |
| Contradictory edits | Single-writer executor and review gate |
| Tool incompatibility | Capability filtering and explicit errors |
| Invalid client stream | Final-response adapter preserving OpenAI SSE |
| Sensitive-data leakage | Redacted logs and bounded context |
| Partial failure presented as success | Explicit incomplete-stage metadata |
| Existing clients breaking | Opt-in contract and unchanged normal pool path |

## Acceptance criteria

- Existing tests and ordinary pool behavior remain green.
- Collective mode is explicit and bounded.
- At least three specialist roles run through existing pools.
- Aggregation identifies duplicate and conflicting results.
- Critique can reject unsupported or inconsistent proposals.
- Tool calls and finish reasons remain compatible with Zed-style clients.
- Client disconnects cancel internal work.
- Provider failures degrade predictably.
- Logs trace one workflow without secrets.
- Usage and quota impact are measurable.
- Documentation distinguishes proposals, completed changes, and partial results.
