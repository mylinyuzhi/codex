# cocode-rs Architecture Review & Refactoring Guide (v2)

Full architecture review of 78-crate cocode-rs workspace. Updated 2026-03-29 with current codebase metrics. Reference: `codex-rs` (Anthropic's official Rust implementation).

---

## 1. Overall Assessment: B+ (Good, with concentrated debt)

The architecture is fundamentally sound. The CoreEvent redesign (v1's top priority) is complete and ahead of codex-rs. Remaining debt is concentrated in **tools coupling** and **god objects** (ToolContext, SessionState, StreamingToolExecutor).

### What's Excellent (Don't Touch)

| Area | Rating | Why |
|------|--------|-----|
| CoreEvent / ServerNotification | A+ | 3-layer split with macro-based wire format; ahead of codex-rs |
| Vercel AI isolation | A | Zero reverse deps; clean abstraction layer |
| Utils independence | A | 20 crates, minimal upward coupling |
| Error handling | A | Consistent snafu + StatusCode across all layers |
| Crate naming | A | Uniform `cocode-*` prefix |
| Test organization | A | 863 `.test.rs` files; consistent pattern |
| Feature crate design | A- | hooks, skill, team are clean and composable |
| Builder patterns | A- | Consistent across all config objects |
| Workspace management | A | Centralized dep versions; strict lints |

### What Needs Refactoring

| Issue | Severity | Current Metrics |
|-------|----------|-----------------|
| `cocode-tools` has **26 internal deps** | Critical | 6 feature-layer deps invert core→features layering |
| `ToolContext` has **42 pub fields** | High | God object; 2,182 LoC; every tool gets everything |
| `StreamingToolExecutor` has **30 fields** | High | 2,358 LoC; 3 separate Mutexes; 30+ field duplication with ToolContext |
| `SessionState` has **45 fields, 83 methods** | High | 3,477 LoC; god facade aggregating 25+ subsystems |
| 3 files exceed 2,000 LoC | Medium | state.rs (3,477), executor.rs (2,358), context.rs (2,182) |
| `cocode-protocol` has **203 re-exports** | Medium | Better file org now; crate split deferred |

### Completed Since v1

| Item | Status | Details |
|------|--------|---------|
| **R1: CoreEvent split** | DONE | 90-variant `LoopEvent` → 3-variant `CoreEvent{Protocol, Stream, Tui}`. ServerNotification: 55 variants (17 domain groups, macro-generated with JSON-RPC wire format). StreamEvent: 7. TuiEvent: 20. `loop_event.rs`: 9 lines. |
| **R9: Subagent dep** | Confirmed unused | `cocode-tools` in `core/subagent/Cargo.toml:11` — zero imports in source |

---

## 2. CoreEvent Architecture (COMPLETED — Reference)

**Files:**
- `common/protocol/src/core_event.rs` (22 lines)
- `common/protocol/src/server_notification/notification.rs` (753 lines)
- `common/protocol/src/stream_event.rs` (81 lines)
- `common/protocol/src/tui_event.rs` (168 lines)

```rust
pub enum CoreEvent {
    Protocol(ServerNotification),  // 55 variants — SDK/app-server/TUI consumers
    Stream(StreamEvent),           // 7 variants — raw streaming deltas
    Tui(TuiEvent),                 // 20 variants — TUI-only events
}
```

### ServerNotification Domain Breakdown (55 variants)

| Domain | Count | Wire prefix | Examples |
|--------|-------|-------------|---------|
| Session lifecycle | 3 | `session/*` | Started, Result, Ended |
| Turn lifecycle | 6 | `turn/*` | Started, Completed, Failed, Interrupted, MaxReached, Retry |
| Item lifecycle | 3 | `item/*` | Started, Updated, Completed |
| Content streaming | 2 | `agentMessage/*`, `reasoning/*` | Delta events |
| Sub-agent | 4 | `subagent/*` | Spawned, Completed, Backgrounded, Progress |
| MCP | 2 | `mcp/*` | StartupStatus, StartupComplete |
| Context management | 5 | `context/*` | Compacted, UsageWarning, CompactionStarted/Failed, Cleared |
| Background tasks | 4 | `task/*`, `agents/*` | Started, Completed, Progress, Killed |
| Model | 3 | `model/*` | FallbackStarted, FallbackCompleted, FastModeChanged |
| Permission | 1 | `permission/*` | ModeChanged |
| IDE | 2 | `ide/*` | SelectionChanged, DiagnosticsUpdated |
| Plan mode | 1 | `plan/*` | ModeChanged |
| Queue | 3 | `queue/*` | StateChanged, CommandQueued, CommandDequeued |
| Rewind | 2 | `rewind/*` | Completed, Failed |
| Cost/Sandbox | 3 | `cost/*`, `sandbox/*` | Warning, StateChanged, ViolationsDetected |
| Hooks/Agents | 2 | `hook/*`, `agents/*` | Executed, Registered |
| Summarize | 2 | `summarize/*` | Completed, Failed |
| Stream health | 3 | `stream/*` | StallDetected, WatchdogWarning, RequestEnd |
| System | 3 | `error`, `rateLimit`, `keepAlive` | Non-domain events |
| Prompt | 1 | `prompt/*` | Suggestion |

Generated via `server_notification_definitions!` macro with `#[serde(tag = "method", content = "params")]`.

**Note:** This is architecturally **ahead of codex-rs**, which still uses a monolithic 77-variant `EventMsg` enum without sub-enum grouping.

---

## 3. Critical: cocode-tools Hub (26 Internal Deps)

**File:** `core/tools/Cargo.toml`

### Layer Violation

```
Intended:  common → core → features → app
Actual:    common → core/tools ← features (hooks, skill, plan-mode, team, cron, auto-memory)
```

### Current Dependencies (26)

```
cocode-tools:
  Common:   protocol, policy, config, inference, message, error
  Features: hooks, auto-memory, plan-mode, team, cron, skill     ← LAYER VIOLATION
  Exec:     sandbox, shell, shell-parser, apply-patch, file-backup
  Utils:    file-encoding, file-ignore, utils-string, secret-redact
  MCP:      mcp-types, rmcp-client
  Other:    lsp, otel, git
```

### What Consumers Actually Use

| Consumer | Types imported from tools |
|----------|-------------------------|
| cocode-loop | ExecutorConfig, FileTracker, FileReadState, ModelCallFn, SpawnAgentFn, StreamingToolExecutor, ToolExecutionResult, ToolRegistry |
| cocode-executor | SpawnAgentFn, ToolRegistry, PermissionRequester |
| cocode-system-reminder | FileTracker, FileReadState |
| cocode-session | ToolContext, ToolRegistry, StreamingToolExecutor, QuestionResponder + context types |
| **cocode-subagent** | **Nothing** (zero imports — unused dependency) |

### Design: Split into `tools-api` + `tools`

```
core/tools-api/  (NEW — API surface: trait + context + registry + executor)
  Deps (~14): protocol, policy, error, inference, hooks, skill, lsp,
              sandbox, shell, mcp-types, rmcp-client, otel, file-encoding, file-backup
  Contains:
    Tool trait, ToolContext (with sub-structs), ToolContextBuilder
    ToolRegistry, McpToolInfo, McpToolWrapper
    StreamingToolExecutor, ExecutorConfig, ToolExecutionResult
    FileTracker, FileReadState
    SpawnAgentFn, ModelCallFn, PermissionRequester, QuestionResponder
    Error types, result persistence, sensitive file detection

core/tools/  (EXISTING — tool implementations only)
  Deps: tools-api + plan-mode, team, cron, auto-memory, apply-patch,
        shell-parser, file-ignore, git, utils-string, secret-redact
  Contains:
    40+ tool implementation files (flatten current builtin/ into src/)
    register_tools()
    Re-exports from tools-api
```

### Consumer Migration

| Consumer | Before | After |
|----------|--------|-------|
| cocode-loop | `cocode-tools` (26 deps) | `cocode-tools-api` (~14 deps) |
| cocode-executor | `cocode-tools` | `cocode-tools-api` |
| cocode-system-reminder | `cocode-tools` | `cocode-tools-api` |
| cocode-subagent | `cocode-tools` (unused) | Remove entirely |
| cocode-session | `cocode-tools` | Both `tools-api` + `tools` |
| cocode-cli, app-server, plugin | `cocode-tools` | Both `tools-api` + `tools` |

### Impact

- loop/executor/system-reminder: deps drop from 26 to ~14 (46% reduction)
- Feature crate changes no longer force loop/executor recompilation
- Core→features layering restored
- Compilation chain shortened

### codex-rs Comparison

codex-rs keeps tools in a single `core` crate but separates concerns internally:
- `tools/context.rs` (508 lines) — lean `ToolInvocation` with 7 fields
- `tools/registry.rs` (542 lines) — `ToolHandler` trait + `ToolRegistry`
- `tools/orchestrator.rs` (365 lines) — `ToolOrchestrator` with 1 field
- `tools/sandboxing.rs` (367 lines) — `Approvable` + `Sandboxable` traits
- `tools/handlers/` — 40+ per-tool files

Key insight: codex-rs `ToolOrchestrator` has **1 field** (SandboxManager) — everything else is passed as method parameters or accessed via `Arc<Session>`.

---

## 4. High: ToolContext God Object (42 pub fields)

**File:** `core/tools/src/context.rs` (2,182 lines)

Every tool receives the full 42-field struct via `&mut ToolContext`. Most tools use 3-5 fields. All fields are `pub`. 7 `Arc<Mutex<>>` fields with no lock ordering docs.

### Current Fields (42)

```
Identity (5):     call_id, session_id, turn_id, turn_number, agent_id
Environment (8):  cwd, additional_working_directories, permission_mode, features,
                  web_search_config, web_fetch_config, task_type_restrictions,
                  is_plan_mode, is_ultraplan
Channels (2):     event_tx, cancel_token
State (4):        approval_store [Mutex], file_tracker [Mutex], invoked_skills [Mutex],
                  output_offsets [Mutex]
Services (10):    shell_executor, sandbox_state, lsp_manager, skill_manager,
                  skill_usage_tracker, hook_registry, permission_requester,
                  permission_evaluator, file_backup_store, question_responder
Agent (6):        spawn_agent_fn, agent_cancel_tokens [Mutex], killed_agents [Mutex],
                  agent_output_dir, model_call_fn, parent_selections
Paths (4):        session_dir, cocode_home, auto_memory_dir, plan_file_path
```

### Also in context.rs (non-ToolContext types): ~1,500 lines

| Type | Lines | Purpose |
|------|-------|---------|
| FileTracker + FileReadState + FileReadKind + FileChangeInfo | ~550 | File change tracking for system reminders |
| SpawnAgentInput/Result/Fn + ModelCallInput/Result/Fn | ~160 | Callback types for agent spawning and model calls |
| QuestionResponder | ~110 | AskUserQuestion tool responder |
| ToolContextBuilder | ~350 | 42-field builder with `with_*` methods |
| Type aliases + InvokedSkill + PermissionRequester | ~80 | Supporting types |
| Utility functions (hash, sensitive files) | ~250 | File hashing, sensitivity detection |

### Design: Sub-struct Decomposition

```rust
pub struct ToolContext {
    pub identity: ToolCallIdentity,    // 5 fields: call_id, session_id, turn_id, turn_number, agent_id
    pub env: ToolEnvironment,          // 8 fields: cwd, dirs, permissions, features, configs, plan flags
    pub channels: ToolChannels,        // 2 fields: event_tx, cancel_token
    pub state: Arc<ToolSharedState>,   // 4 fields: approval_store, file_tracker, invoked_skills, output_offsets
    pub services: ToolServices,        // 10 fields: shell, sandbox, lsp, skill, hooks, permissions, backup, question
    pub agent: AgentContext,           // 6 fields: spawn_fn, cancel_tokens, killed, output_dir, model_fn, parent_selections
    pub paths: SessionPaths,           // 4 fields: session_dir, cocode_home, auto_memory_dir, plan_file_path
}
```

**Note:** `ToolCallIdentity` is a NEW type — not `ExecutionIdentity` (which is about model addressing: Role/Spec/Inherit).

### codex-rs Comparison

```rust
// codex-rs: 7 fields — delegates heavy state via Arc<Session>
pub struct ToolInvocation {
    pub session: Arc<Session>,         // session-scoped services and state
    pub turn: Arc<TurnContext>,        // turn-scoped config
    pub tracker: SharedTurnDiffTracker,
    pub call_id: String,
    pub tool_name: String,
    pub tool_namespace: Option<String>,
    pub payload: ToolPayload,          // Function | ToolSearch | Custom | LocalShell | Mcp
}
```

---

## 5. High: StreamingToolExecutor (30 fields, 2,358 LoC)

**File:** `core/tools/src/executor.rs` (2,358 lines)

### Current Problems

1. **3 separate Mutexes** for related execution state:
   ```rust
   active_tasks: Arc<Mutex<HashMap<String, JoinHandle<ToolExecutionResult>>>>
   pending_unsafe: Arc<Mutex<Vec<PendingToolCall>>>
   completed_results: Arc<Mutex<Vec<ToolExecutionResult>>>
   ```
   Risk: related state split across locks → inconsistent reads. No lock ordering docs.

2. **30+ field duplication with ToolContext**: Both structs carry approval_store, file_tracker, shell_executor, sandbox_state, spawn_agent_fn, agent_cancel_tokens, killed_agents, model_call_fn, lsp_manager, skill_manager, permission_requester, permission_evaluator, etc. The executor's `create_context()` copies all these into ToolContextBuilder per call.

3. **Mixed responsibilities**: 22 builder methods + execution methods + hook coordination + batch/abort management in one struct and file.

### Design: 3-File Split + Mutex Consolidation + Field Dedup

**Mutex consolidation:**
```rust
struct ToolExecutionState {
    active_tasks: HashMap<String, JoinHandle<ToolExecutionResult>>,
    pending_unsafe: Vec<PendingToolCall>,
    completed_results: Vec<ToolExecutionResult>,
}
// 3 Arc<Mutex<>> → 1 Arc<Mutex<ToolExecutionState>>
```

**File split:**

| File | Content | ~Lines |
|------|---------|--------|
| `executor.rs` | Core execution: on_tool_complete, execute_pending_unsafe, drain, abort, create_context | ~1,200 |
| `executor_builder.rs` | ExecutorConfig, 22 `with_*` builder methods | ~400 |
| `executor_hooks.rs` | Pre/post hook coordination, emit_event, async hook tracking | ~400 |

**Field deduplication (after ToolContext decomposition):**

Instead of duplicating 30+ fields, executor holds shared sub-structs:
```rust
pub struct StreamingToolExecutor {
    // Shared with ToolContext (stamped per-call)
    services: Arc<ToolServices>,
    shared_state: Arc<ToolSharedState>,
    env: ToolEnvironment,
    agent: AgentContext,
    paths: SessionPaths,

    // Executor-specific
    registry: Arc<ToolRegistry>,
    executor_state: Arc<Mutex<ToolExecutionState>>,
    async_hook_tracker: Arc<AsyncHookTracker>,
    current_batch_id: Arc<RwLock<Option<String>>>,
    sibling_abort_token: Arc<RwLock<CancellationToken>>,
    sibling_error_desc: Arc<RwLock<Option<String>>>,
    allowed_tool_names: Arc<RwLock<Option<HashSet<String>>>>,
    skill_allowed_tools: Arc<RwLock<Option<HashSet<String>>>>,
    invoked_skills: Arc<Mutex<Vec<InvokedSkill>>>,
    otel_manager: Option<Arc<OtelManager>>,
}
```

### codex-rs Comparison

codex-rs `ToolOrchestrator` has **1 field**:
```rust
pub struct ToolOrchestrator {
    sandbox: SandboxManager,
}
```
Everything else is passed as method parameters. The approval/sandbox flow uses trait-based composition (`Approvable + Sandboxable`).

---

## 6. High: SessionState God Facade (45 fields, 83 methods)

**File:** `app/session/src/state.rs` (3,477 lines)

### Current Field Groups (45 fields)

| Group | Count | Fields |
|-------|-------|--------|
| Core | 5 | session, message_history, tool_registry, hook_registry, config |
| Model/Inference | 3 | api_client, model_hub, api (ProviderApi) |
| Skills/Plugins | 4 | skills, skill_manager, plugin_registry, plugin_output_styles |
| Lifecycle | 3 | cancel_token, loop_config, plan_mode_state |
| Metrics | 4 | total_turns, total_input_tokens, total_output_tokens, context_window |
| Exec/Sandbox | 4 | shell_executor, sandbox_state, sandbox_proxy, sandbox_bridge |
| Steering | 2 | queued_commands [Mutex], fast_mode [AtomicBool] |
| Prompt | 3 | system_prompt_suffix, system_prompt_override, structured_output_schema |
| Subagent | 1 | subagent_manager [Mutex] |
| MCP/LSP | 2 | _mcp_clients, lsp_manager |
| Permissions | 2 | permission_rules, shared_approval_store [Mutex] |
| State tracking | 7 | todos, structured_tasks, cron_jobs, output_style_override, reminder_file_tracker_state, killed_agents, auto_memory_state |
| Rewind | 1 | snapshot_manager |
| Question/Team | 3 | question_responder, team_store, team_mailbox |

### Current Method Groups (83 pub methods)

| Domain | ~Count | Examples |
|--------|--------|---------|
| Turn execution | 8 | run_turn, run_skill_turn, run_turn_streaming, run_partial_compact |
| Context/rewind | 12 | rewind_*, prune_reminder_*, rebuild_reminder_*, clear_context |
| Permission | 5 | set_permission_mode, inject_allowed_prompts, append_permission_rules |
| Model/selection | 10 | switch_role, get_selections, main_model, build_thinking_options |
| Queue/steering | 5 | queue_command, queued_count, take_queued_commands |
| Accessors | 43 | Getters/setters for 45 fields |

### Design: Extract Domain Managers

```
SessionState (slim coordinator — holds references, delegates)
  ├── turn_runner.rs     (~600 lines)  — run_turn_streaming, run_skill_turn, build_agent_loop, subagent spawning
  ├── context_ops.rs     (~500 lines)  — rewind_*, compaction, reminder_file_tracker, clear_context
  └── permission_ops.rs  (~250 lines)  — permission rules, approval_store, mode switching
```

SessionState retains: field definitions, `new()`, accessors/getters, model management, queue management. Drops from 83 to ~50 pub methods. File from 3,477 to ~1,500 lines.

### codex-rs Comparison

codex-rs `SessionState`: 11 fields, 240 lines. Delegates to `ContextManager` (history), `Session` (coordinator), and `TurnContext` (per-turn config).

---

## 7. Medium: cocode-protocol Blast Radius (DEFERRED)

**File:** `common/protocol/src/lib.rs` — 203 re-exports, 275 lines, 23 dependent crates

### Current State

The protocol crate has been internally reorganized since v1:
- Events: `core_event.rs`, `stream_event.rs`, `tui_event.rs`, `server_notification/` (well-organized)
- Models: `model/` subdirectory
- Execution: `execution/` subdirectory (ExecutionIdentity, InferenceContext)
- Config: separate files per domain (compact_config.rs, mcp_config.rs, etc.)

File-level separation is good. The remaining issue is crate-level: changing any constant in any config file rebuilds all 23 dependents.

### Assessment

Crate-level split (protocol-core/events/config) has very high blast radius (23 Cargo.toml + import changes) for moderate value. The internal file organization already provides good locality. Incremental compilation handles source-level changes well.

**Decision: Defer.** Re-evaluate if compilation times become a bottleneck after R2 (tools-api split) reduces the dependency chain.

---

## 8. Dependency Graph

### Compilation Bottleneck Crates

```
                    Reverse Deps
cocode-protocol  ------ 23      (any change = near-full rebuild)
cocode-error     ------ 25+     (fundamental, but very stable)
cocode-config    ------ 10+     (moderate blast radius)
cocode-tools     ------  8      (BUT: 26 forward deps = slow to compile)
cocode-inference ------  6      (moderate)
```

### Layering (Current vs Target)

```
Current:   utils → common → core/tools ←→ features → app
                                 ↑             |
                                 +-------------+  (tools depends on features)

Target:    utils → common → core/tools-api → features → app
                              ↓
                          core/tools (tool impls only)
```

### Longest Compilation Chain

```
Current:
  cocode-cli → cocode-session (25 deps) → cocode-executor → cocode-loop (26 deps)
    → cocode-tools (26 deps) → 40+ tool implementations

Target (after R2):
  cocode-cli → cocode-session → cocode-executor → cocode-loop
    → cocode-tools-api (~14 deps)
  cocode-session → cocode-tools (impls only, for registration)
```

---

## 9. Concurrency Patterns

### Arc<Mutex<>> Usage

**StreamingToolExecutor:** 3 separate Mutexes for related state (see Section 5).

**ToolContext:** 7 `Arc<Mutex<>>` fields flowing through the entire tool pipeline:
- `approval_store`, `file_tracker`, `invoked_skills`, `output_offsets` (tokio::sync::Mutex)
- `agent_cancel_tokens`, `killed_agents` (tokio::sync::Mutex)
- `pending` in QuestionResponder (std::sync::Mutex)

**Positive patterns:**
- All Mutex usage includes poison recovery (`unwrap_or_else`)
- `RwLock` used in read-heavy paths (features/ide, executor batch_id/allowed_tool_names)
- Lock poisoning recovery utility in `features/hooks/src/lock_utils.rs`

### Required Lock Ordering

```rust
/// Lock Acquisition Order (deadlock prevention):
/// 1. executor_state (consolidated from active_tasks + pending_unsafe + completed_results)
/// 2. approval_store
/// 3. file_tracker
/// 4. invoked_skills
/// 5. agent_cancel_tokens
/// 6. killed_agents
/// 7. output_offsets
```

---

## 10. Rust Best Practices Compliance

| Practice | Status | Notes |
|----------|--------|-------|
| No `unsafe` code | Pass | Project policy; enforced |
| No `.unwrap()` in non-test code | Pass | Workspace lint denies it |
| `?` for error propagation | Pass | Consistent throughout |
| `Send + Sync` on async traits | Pass | `Tool: Send + Sync` |
| Workspace dep management | Pass | All versions centralized |
| Feature flags for optional deps | Pass | retrieval, lsp use feature gates |
| Separate test files | Pass | 863 test files with `.test.rs` |
| No inline tests in source | Pass | Project policy; enforced |
| Module size < 500 LoC | Partial | 3 files exceed 2,000 LoC; most comply |
| Exhaustive matches | Pass | Clippy enforced; HookEventType made exhaustive |
| No hardcoded tool/enum names | Pass | Uses `ToolName::X.as_str()` pattern |
| Integer types (i32/i64 over u32/u64) | Pass | Convention followed |

---

## 11. Refactoring Priority Matrix

### Tier 1: High Value

| # | Refactoring | Value | Effort | Key Files |
|---|-------------|-------|--------|-----------|
| **R2** | Split cocode-tools into tools-api + tools | Very High | High | `core/tools/Cargo.toml`, 8 consumer Cargo.tomls |
| **R3** | Decompose ToolContext into sub-structs | High | Medium | `core/tools/src/context.rs` (2,182 lines) |
| **R4** | Consolidate executor Mutexes | Medium | Low | `core/tools/src/executor.rs` |
| **R5** | Split executor into 3 files | Medium | Low | `core/tools/src/executor.rs` (2,358 lines) |

### Tier 2: Medium Value

| # | Refactoring | Value | Effort | Key Files |
|---|-------------|-------|--------|-----------|
| **R6** | Extract SessionState managers | Medium | Medium | `app/session/src/state.rs` (3,477 lines) |
| **N1** | Eliminate executor↔context field duplication | Medium | Medium | executor.rs + context.rs |
| **R8** | Add lock ordering documentation | Medium | Low | executor.rs |

### Tier 3: Quick Wins

| # | Refactoring | Value | Effort | Key Files |
|---|-------------|-------|--------|-----------|
| **N2** | Remove subagent→tools unused dependency | Low | Very Low | `core/subagent/Cargo.toml:11` |

### Completed

| # | Refactoring | Status |
|---|-------------|--------|
| **R1** | CoreEvent split into categorized sub-enums | DONE — 3-variant envelope with 82 total variants |

### Deferred

| # | Refactoring | Reason |
|---|-------------|--------|
| **R7** | Protocol crate split | File-level org is good; blast radius too high for moderate value |
| **R10** | TUI file splits | Coherent state machine; splitting may hurt clarity |

---

## 12. Recommended Execution Order

```
Phase 0: Quick Wins (< 1 day)
  N2  Remove cocode-subagent → cocode-tools dependency (1-line Cargo.toml change)
  R8  Add lock ordering documentation to executor.rs

Phase 1: ToolContext Decomposition (2-3 days)
  R3a  Extract FileTracker, SpawnAgent*, QuestionResponder to own files
       (pure file moves, no API changes — context.rs drops from 2,182 to ~600 lines)
  R3b  Group ToolContext fields into 7 sub-structs
       (update ~40 tool impls: ctx.cwd → ctx.env.cwd etc.)

Phase 2: Executor Cleanup (2-3 days, parallelizable with Phase 1)
  R4   Consolidate 3 Mutexes into ToolExecutionState
  R5   Extract executor_builder.rs and executor_hooks.rs
  N1   Share sub-structs between executor and ToolContext (after R3b)

Phase 3: Tools Crate Split (3-5 days, depends on Phase 1)
  R2   Create core/tools-api with API surface
       Slim core/tools to tool impls only (flatten builtin/ into src/)
       Update 8 consumer Cargo.tomls

Phase 4: Session State (3-5 days, independent of Phases 1-3)
  R6   Extract turn_runner.rs, context_ops.rs, permission_ops.rs
       (state.rs drops from 3,477 to ~1,500 lines)
```

---

## 13. Files Exceeding 500 LoC Guideline

| File | Lines | Action |
|------|-------|--------|
| `app/session/src/state.rs` | 3,477 | Phase 4: Extract managers |
| `app/tui/src/state/ui.rs` | 3,063 | Leave: coherent TUI state machine |
| `core/tools/src/executor.rs` | 2,358 | Phase 2: Split 3 files |
| `core/tools/src/context.rs` | 2,182 | Phase 1: Extract types + decompose |
| `core/loop/src/compaction.rs` | 1,855 | Leave: specialized algorithm |

---

## 14. What NOT to Refactor

| Area | Reason |
|------|--------|
| CoreEvent / ServerNotification | Already refactored; ahead of codex-rs |
| Vercel AI layer | Exemplary isolation |
| Utils crate structure | Independent, well-scoped |
| Error handling (snafu + StatusCode) | Consistent and effective |
| Feature crate design | hooks, skill, team are clean |
| Builder patterns | Already consistent |
| Test organization | 863 test files; solid |
| TUI state (ui.rs, 3,063 lines) | Complex but coherent state machine |
| Compaction algorithm (1,855 LoC) | Specialized algorithm; coherence matters |
| Feature enum (27 variants) | Well-designed with Stage lifecycle |
| Plugin crate coupling (9 deps) | Correct for an orchestration layer |
