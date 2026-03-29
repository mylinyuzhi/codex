# cocode-rs Architecture Review & Refactoring Guide

Full architecture review of 79-crate cocode-rs workspace. Goal: identify structural debt and prioritize refactoring for **optimal architecture** (no backward compatibility constraints).

---

## 1. Overall Assessment: B+ (Good, with concentrated debt)

The architecture is fundamentally sound. The debt is concentrated in 3 areas: **tools coupling**, **CoreEvent bloat**, and **god objects** (ToolContext, SessionState). Everything else is well-designed.

### What's Excellent (Don't Touch)

| Area | Rating | Why |
|------|--------|-----|
| Vercel AI isolation | A | Zero reverse deps; clean abstraction layer |
| Utils independence | A | 24 crates, minimal upward coupling |
| Error handling | A | Consistent snafu + StatusCode across all layers |
| Crate naming | A | Uniform `cocode-*` prefix |
| Test organization | A | 863 `.test.rs` files; consistent pattern |
| Feature crate design | A- | hooks, skill, team are clean and composable |
| Builder patterns | A- | Consistent across all config objects |
| Workspace management | A | Centralized dep versions; strict lints |

### What Needs Refactoring

| Issue | Severity | Root Cause |
|-------|----------|------------|
| `CoreEvent` has **90 variants** | Critical | Catch-all event enum for entire system |
| `cocode-tools` has **23+ internal deps** | Critical | All tool impls in one crate; inverts core->features layering |
| `ToolContext` has **40+ pub fields** | High | God object passed to every tool |
| `SessionState` has **40 fields, 93 methods** | High | God facade aggregating 25 subsystems |
| `StreamingToolExecutor` has **28 fields, 48 methods** | High | Execution + building + hooks + context in one struct |
| `cocode-protocol` triggers **30+ crate rebuilds** | Medium | 166 re-exports; any change cascades |
| 6 files exceed 500 LoC guideline significantly | Medium | session/state.rs (3460), tui/ui.rs (3063), tools/executor.rs (2175) |
| 3 separate Mutexes for related executor state | Medium | Inconsistency risk; no lock ordering docs |

---

## 2. Critical Issue #1: CoreEvent (90 Variants)

**File:** `common/protocol/src/loop_event.rs` (1,408 lines)

This is the single largest design debt. A 90-variant enum is:
- Impossible to match exhaustively without wildcard arms
- Violates open-closed principle (every new feature adds variants)
- Forces all consumers to depend on all event types

### Variant Breakdown by Domain

| Domain | Count | Examples |
|--------|-------|---------|
| Stream/Turn lifecycle | 6 | RequestStart, RequestEnd, TurnStarted |
| Content streaming | 4 | ContentDelta, ThinkingDelta |
| Tool execution | 5 | ToolUseQueued, ToolUseStarted, ToolUseCompleted |
| Permission | 3 | PermissionRequested, PermissionGranted |
| Agent/background | 7 | AgentSpawned, AgentCompleted |
| **Compaction** | **19** | Started, Completed, Retrying, Failed... (internal detail leaked) |
| MCP | 4 | McpServerConnected, McpToolInvoked |
| Plan mode | 3 | PlanModeEntered, PlanModeExited |
| Speculative execution | 3 | SpeculationStarted, SpeculationCompleted |
| Rewind | 4 | RewindStarted, RewindCompleted |
| Sandbox | 3 | SandboxEnabled, SandboxViolation |
| Queue/steering | 3 | CommandQueued, InterruptReceived |
| Other (12 domains) | ~26 | Scattered across cron, hooks, cost, fast-mode, etc. |

### Optimal Design: Categorized Event Hierarchy

```rust
pub enum CoreEvent {
    Stream(StreamEvent),        // 10 variants: lifecycle + content
    Tool(ToolEvent),            // 8 variants: execution pipeline
    Permission(PermissionEvent),// 3 variants
    Agent(AgentEvent),          // 7 variants: spawn/complete/background
    Compaction(CompactionEvent),// 19 variants (internal, can be feature-gated)
    Context(ContextEvent),      // 7 variants: rewind, clear, restore
    Model(ModelEvent),          // 5 variants: fallback, retry, fast-mode
    Mcp(McpEvent),              // 4 variants
    System(SystemEvent),        // 10 variants: plan, sandbox, hooks, cron, cost
    Error(ErrorEvent),          // 5 variants: api, stall, overflow
    Queue(QueueEvent),          // 3 variants
}
```

**Value:** Pattern matching becomes ergonomic. Each sub-enum is 3-19 variants (manageable). New features add to specific categories. Consumers only need to handle categories they care about.

---

## 3. Critical Issue #2: cocode-tools Hub (23+ Deps)

**File:** `core/tools/Cargo.toml`

`cocode-tools` sits in `core/` but depends on `features/` crates -- this **inverts the intended layering**:

```
Intended:  common -> core -> features -> app
Actual:    common -> core/tools <- features (hooks, skill, plan-mode, team, cron, auto-memory)
```

### Current Dependency Load

```
cocode-tools (23+ internal deps):
  Common:   protocol, policy, config, inference, message, error
  Features: hooks, auto-memory, plan-mode, team, cron, skill     <- LAYER VIOLATION
  Exec:     sandbox, shell, shell-parser, apply-patch, file-backup
  Utils:    file-encoding, file-ignore, string, secret-redact
  MCP:      mcp-types, rmcp-client
  Other:    lsp, otel, git
```

### What cocode-loop Actually Uses from tools

Only **15 types**: ExecutorConfig, FileTracker, StreamingToolExecutor, ToolRegistry, ToolExecutionResult, SpawnAgentFn, ModelCallFn, PermissionRequester, etc. None require feature crate dependencies.

### What cocode-subagent Uses from tools

**Nothing.** It is wrongly listed as a dependency.

### Optimal Split

```
cocode-tools-api/  (NEW -- trait + context + registry)
  Deps: protocol, error, policy, hooks (for HookRegistry type)
  Contains: Tool trait, ToolContext, ToolRegistry, StreamingToolExecutor,
            FileTracker, ExecutorConfig, SpawnAgentFn, ModelCallFn
  ~8 dependencies instead of 23+

cocode-tools/  (EXISTING -- builtin implementations only)
  Deps: tools-api + all current deps (the 40+ tool impls need them)
  Contains: All builtin tools (Read, Write, Edit, Bash, Glob, etc.)
            register_builtin_tools()
```

**Impact:**
- `cocode-loop`: deps drop from 23+ to ~8 (77% reduction)
- `cocode-subagent`: remove tools dependency entirely
- `cocode-executor`: depends on tools-api only
- Feature crate changes no longer force loop/executor recompilation
- Core->Features layering restored

---

## 4. High Issue #1: ToolContext God Object (40+ pub fields)

**File:** `core/tools/src/context.rs` (2,166 lines)

Every tool receives the full 40-field context. Most tools use 3-5 fields. All fields are `pub` -- no encapsulation. 7 `Arc<Mutex<>>` fields with no lock ordering docs.

### Field Grouping (current 40+ fields -> 5 sub-contexts)

```rust
pub struct ToolContext {
    pub identity: ExecutionIdentity,      // call_id, session_id, turn_id, turn_number, agent_id
    pub env: ToolEnvironment,             // cwd, additional_dirs, permission_mode, features, configs
    pub channels: ToolChannels,           // event_tx, cancel_token
    pub state: Arc<ToolSharedState>,      // approval_store, file_tracker, invoked_skills, output_offsets
    pub services: ToolServices,           // shell_executor, lsp_manager, skill_manager, hook_registry, sandbox_state
    pub agent: AgentContext,              // spawn_agent_fn, cancel_tokens, killed_agents, output_dir, parent_selections
    pub session: SessionContext,          // session_dir, cocode_home, auto_memory_dir, plan info
}
```

**Value:** Tools declare which sub-context they need. Mock contexts are trivial. Lock ordering is scoped to `ToolSharedState`. Feature-specific services are isolated in `ToolServices`.

---

## 5. High Issue #2: SessionState God Facade (40 fields, 93 methods)

**File:** `app/session/src/state.rs` (3,460 lines)

93 public methods spanning 4 distinct domains mixed into one struct:

| Domain | Methods | Examples |
|--------|---------|---------|
| Turn execution | ~10 | run_turn, run_skill_turn, spawn_subagent |
| Context management | ~15 | rewind, compaction, clear_context, file_tracker |
| Permission/policy | ~10 | set_permission_mode, inject_allowed_prompts |
| Model/config | ~15 | switch_role, get_selections, set_loop_config |
| Accessors/lifecycle | ~43 | Getters/setters for all 40 fields |

### Optimal Split

```
SessionState (slim coordinator -- holds references, delegates)
  |-- TurnRunner        (run_turn, run_skill_turn, streaming variants)
  |-- ContextManager    (rewind, compaction, reminder_file_tracker)
  |-- PermissionManager (rules, approval_store, mode switching)
  +-- Accessors remain on SessionState (getters/setters are fine)
```

**Value:** Each extracted struct is testable independently. SessionState becomes a thin coordinator. Method count drops from 93 to ~50 (accessors + delegation).

---

## 6. High Issue #3: StreamingToolExecutor (28 fields, 48 methods)

**File:** `core/tools/src/executor.rs` (2,175 lines)

25 builder methods + 15 execution methods + 5 hook methods + 3 context methods all on one struct.

### Optimal Split

```
executor.rs        (~1000 lines) -- Core execution: on_tool_complete, execute_pending_unsafe, drain, abort
executor_builder.rs (~400 lines) -- All 25 with_* builder methods -> builds executor
executor_hooks.rs   (~400 lines) -- Pre/post hook coordination, emit_event
```

Also consolidate 3 related Mutexes into one:

```rust
// Before: 3 separate locks, inconsistency risk
active_tasks: Arc<Mutex<HashMap<...>>>
pending_unsafe: Arc<Mutex<Vec<...>>>
completed_results: Arc<Mutex<Vec<...>>>

// After: single lock, atomic state transitions
executor_state: Arc<Mutex<ToolExecutionState>>
```

---

## 7. Medium Issue: cocode-protocol Blast Radius

**File:** `common/protocol/src/lib.rs` -- 166 re-exports, 24 modules, 30+ dependent crates

### Current Structure

All 166 types flat-exported from one crate. Any change triggers 30+ crate rebuilds.

### Optimal Split

```
cocode-protocol-core/    (foundation: model, provider, permission, execution, tool_types, features)
  ~40 exports, used by all layers
  Dependencies: external only (serde, chrono, strum, uuid)

cocode-protocol-events/  (runtime: loop_event, queue, tracking, correlation, agent_status)
  ~50 exports, used by loop/executor/tui
  Dependencies: protocol-core

cocode-protocol-config/  (settings: compact_config, mcp_config, tool_config, web_*, plan_*, etc.)
  ~70 exports (mostly defaults/constants), used by config/loop/tools
  Dependencies: protocol-core
```

**Value:** Changing compaction config does not rebuild features/cron. Event changes do not rebuild config layer. Core types (model, permission) are maximally stable.

---

## 8. Dependency Graph Overview

### Compilation Bottleneck Crates

```
                    Reverse Deps
cocode-protocol  ------ 30+     (any change = near-full rebuild)
cocode-error     ------ 25+     (fundamental, but stable)
cocode-config    ------ 10+     (moderate blast radius)
cocode-tools     ------  8      (BUT: 23+ forward deps = slow to compile)
cocode-inference ------  6      (moderate)
```

### Layering Compliance

```
Expected:  utils -> common -> core -> features -> app
Actual:    utils -> common -> core <-> features -> app
                               ^         |
                               +---------+  (tools depends on features)
```

### Longest Compilation Chain

```
cocode-cli
  -> cocode-session (25 internal deps)
    -> cocode-executor
      -> cocode-loop (26 internal deps)
        -> cocode-tools (23+ internal deps)
          -> 40+ individual tool implementations
```

---

## 9. Concurrency Patterns Assessment

### Arc<Mutex<>> Usage (38 locations)

**StreamingToolExecutor** holds 3 separate Mutexes for related state:
```rust
active_tasks: Arc<Mutex<HashMap<String, JoinHandle<ToolExecutionResult>>>>
pending_unsafe: Arc<Mutex<Vec<PendingToolCall>>>
completed_results: Arc<Mutex<Vec<ToolExecutionResult>>>
```

**Risk:** Related state split across locks can lead to inconsistent reads. No lock ordering documentation exists.

**ToolContext** carries 7 `Arc<Mutex<>>` fields that flow through the entire tool pipeline.

**Positive:** All Mutex usage includes poison recovery (`unwrap_or_else`), and `RwLock` is used in read-heavy paths (features/ide).

### Required Lock Ordering Documentation

```rust
/// Lock Acquisition Order (deadlock prevention):
/// 1. executor_state (if consolidated)
/// 2. approval_store
/// 3. file_tracker
/// 4. invoked_skills
/// 5. output_offsets
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
| Module size < 500 LoC | Partial | ~6 files exceed significantly; most comply |
| Exhaustive matches | Pass | Clippy enforced |
| No hardcoded tool/enum names | Pass | Uses `ToolName::X.as_str()` pattern |
| Integer types (i32/i64 over u32/u64) | Pass | Convention followed |

---

## 11. Refactoring Priority Matrix

### Tier 1: High Value, Reasonable Effort

| # | Refactoring | Value | Effort | Files |
|---|-------------|-------|--------|-------|
| **R1** | Split CoreEvent into categorized sub-enums | Very High | Medium | `common/protocol/src/loop_event.rs` + all consumers |
| **R2** | Split cocode-tools into tools-api + tools | Very High | High | `core/tools/` + Cargo.toml updates |
| **R3** | Decompose ToolContext into sub-contexts | High | Medium | `core/tools/src/context.rs` |
| **R4** | Consolidate executor Mutex state | Medium | Low | `core/tools/src/executor.rs` |
| **R5** | Split StreamingToolExecutor into executor + builder + hooks | Medium | Low | `core/tools/src/executor.rs` |

### Tier 2: Medium Value, Low-Medium Effort

| # | Refactoring | Value | Effort | Files |
|---|-------------|-------|--------|-------|
| **R6** | Split SessionState (extract TurnRunner, ContextManager) | Medium | Medium | `app/session/src/state.rs` |
| **R7** | Split cocode-protocol into core/events/config | Medium | High | `common/protocol/` + all consumers |
| **R8** | Add lock ordering documentation | Medium | Low | `core/tools/src/executor.rs`, `core/executor/` |
| **R9** | Remove cocode-subagent's dependency on cocode-tools | Low | Very Low | `core/subagent/Cargo.toml` |

### Tier 3: Low Value or High Risk

| # | Refactoring | Value | Effort | Notes |
|---|-------------|-------|--------|-------|
| **R10** | Split large TUI files (ui.rs, render.rs, update.rs) | Low | Medium | TUI state machine coherence may suffer |
| **R11** | Namespace protocol re-exports | Low | Medium | Flat re-exports work fine for a types-only crate |
| **R12** | Replace `serde_json::Value` in CoreEvent payloads with typed structs | Low | High | Many consumers; Value is adequate for events |

---

## 12. Recommended Execution Order

```
Phase 1: Quick Wins (1-2 days)
  R4  Consolidate executor Mutex state
  R5  Split StreamingToolExecutor file
  R8  Add lock ordering docs
  R9  Remove subagent -> tools dependency

Phase 2: Tools Layer Restructuring (3-5 days)
  R3  Decompose ToolContext into sub-contexts
  R2  Split cocode-tools into tools-api + tools

Phase 3: Event System Redesign (3-5 days)
  R1  Split CoreEvent into categorized sub-enums
      (touch all event consumers -- TUI, CLI, session, loop)

Phase 4: Session & Protocol (5-7 days, can parallelize)
  R6  Split SessionState
  R7  Split cocode-protocol (largest blast radius -- do last)
```

---

## 13. What NOT to Refactor

| Area | Reason |
|------|--------|
| Vercel AI layer | Already exemplary isolation |
| Utils crate structure | Independent, well-scoped |
| Error handling (snafu + StatusCode) | Consistent and effective |
| Feature crate design | hooks, skill, team are clean |
| Builder patterns | Already consistent |
| Test organization | 863 test files; solid |
| Plugin crate coupling (9 deps) | Correct for an orchestration layer |
| TUI state (ui.rs, 3063 lines) | Complex but coherent; splitting breaks state machine |
| Compaction algorithm (1,855 LoC) | Specialized algorithm; coherence matters |
| Feature enum (27 variants) | Well-designed with Stage lifecycle |
