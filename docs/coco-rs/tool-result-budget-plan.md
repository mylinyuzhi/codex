# Tool Result Budget Plan

> Status: Phase 0 landed (config keys staged in `coco-config::CompactConfig.tool_result_budget`); Phase 1 partially stubbed (Bash-only, divergent shape); Phase 2 absent.
> Scope: `coco-rs/core/tool-runtime/`, `coco-rs/core/tools/`, `coco-rs/app/query/`, `coco-rs/app/session/`, `coco-rs/services/compact/`
> Owners: `coco-tool-runtime` (storage + Level 1) · `coco-tools` (per-tool thresholds) · `coco-query` (Level 2 wiring) · `coco-session` (transcript records) · cross-reference from `coco-compact`
> TS source: `utils/toolResultStorage.ts` (1040 LOC), `constants/toolLimits.ts`, `utils/mcpOutputStorage.ts`, integration in `services/tools/toolExecution.ts:1403` (`addToolResult`) and `query.ts:99,379` (`applyToolResultBudget` import + call)

## TS Feature Gates (all three must be honored)

| TS gate | Scope | Rust mapping |
|---|---|---|
| `tengu_satin_quoll` | GrowthBook override of **per-tool** `maxResultSizeChars` | `Tool::max_result_size_chars()` (post Phase 1.B `ResultSizeBound` migration) — not a Rust config key, lives on each tool impl |
| `tengu_hawthorn_window` | GrowthBook override of **per-message** budget cap | `compact.tool_result_budget.per_message_chars` (env `COCO_COMPACT_TOOL_RESULT_BUDGET_PER_MESSAGE_CHARS`) |
| `tengu_hawthorn_steeple` | Feature gate that **enables Level 2** | `compact.tool_result_budget.enabled` (env `COCO_COMPACT_TOOL_RESULT_BUDGET_ENABLE`) |

The TS feature lives in `utils/`, not `services/compact/`, but functionally it
is the **first line of defense** in the compact capability cluster: stop
oversize tool output from polluting context before any compaction strategy
runs. Mapping docs previously routed the file to `coco-context`; this plan
re-routes it to `coco-tool-runtime` (Level 1) + `coco-query` (Level 2)
because that is where TS actually invokes it.

## Two-Level Architecture

```
┌──────────────────────────────────────────────────────────────────────────┐
│ Level 1 — Per-tool persistence  (utils/toolResultStorage.ts)             │
│  Trigger: each tool call, inside services/tools/toolExecution.ts:addToolResult │
│  Threshold: min(tool.maxResultSizeChars, DEFAULT_MAX_RESULT_SIZE_CHARS=50_000) │
│  Path: <projectDir>/<sessionId>/tool-results/<toolUseId>.{txt,json}      │
│  Action: replace tool_result.content with                                 │
│          <persisted-output>...preview (first 2.0 KB)...</persisted-output> │
│  Idempotent: writeFile(..., {flag:'wx'}); EEXIST tolerated                │
│  Empty guard: empty content → "(<toolName> completed with no output)"     │
│  Skip: image content, content already starting with <persisted-output>    │
├──────────────────────────────────────────────────────────────────────────┤
│ Level 2 — Per-message aggregate budget  (same file, applyToolResultBudget)│
│  Trigger: query.ts before each API call (after attachments, before MC)    │
│  Limit: MAX_TOOL_RESULTS_PER_MESSAGE_CHARS = 200_000 (per API-level group)│
│  Action: pick largest fresh candidates, persist them, replace with preview│
│  State: ContentReplacementState{seenIds, replacements} keyed by tool_use_id│
│  Cache stability: replay byte-identical replacement strings on every turn │
│  Persistence: ContentReplacementRecord written to transcript for resume   │
│  Skip: non-finite maxResultSizeChars (Read self-bounds via maxTokens)     │
└──────────────────────────────────────────────────────────────────────────┘
```

Two levels compose cleanly: Level 1 caps **per-tool** output (one giant Bash
tail); Level 2 caps **per-turn aggregate** (eight parallel Greps each just
under the per-tool cap). Without Level 2, N parallel results can collectively
blow past the budget.

## TS Constants (Source of Truth)

| Constant | Value | Used by |
|---|---|---|
| `DEFAULT_MAX_RESULT_SIZE_CHARS` | `50_000` | Level 1 cap (clamps tool-declared values) |
| `MAX_TOOL_RESULT_TOKENS` | `100_000` | Header for `MAX_TOOL_RESULT_BYTES` |
| `BYTES_PER_TOKEN` | `4` | Persistence analytics estimator |
| `MAX_TOOL_RESULT_BYTES` | `400_000` | Fallback Level 1 threshold when tool didn't declare |
| `MAX_TOOL_RESULTS_PER_MESSAGE_CHARS` | `200_000` | Level 2 budget |
| `PREVIEW_SIZE_BYTES` | `2_000` | Preview window |
| `TOOL_RESULTS_SUBDIR` | `'tool-results'` | Session-relative storage dir |
| `PERSISTED_OUTPUT_TAG` | `'<persisted-output>'` | Wire wrapper open |
| `PERSISTED_OUTPUT_CLOSING_TAG` | `'</persisted-output>'` | Wire wrapper close |
| `TOOL_RESULT_CLEARED_MESSAGE` | `'[Old tool result content cleared]'` | Shared with `microCompact.ts` |

Rust mirror lives in `coco-tool-runtime::tool_result_storage` (proposed). All
constants must use these exact values for cross-runtime transcript interop.

## Per-Tool Thresholds (TS → Rust parity)

| Tool | TS `maxResultSizeChars` | Rust `Tool::max_result_size_chars()` | Aligned? |
|---|---|---|---|
| Bash | `30_000` | `30_000` | ✅ |
| PowerShell | `30_000` | `30_000` | ✅ |
| Grep | `20_000` | `20_000` | ✅ |
| Glob | `100_000` | `100_000` | ✅ |
| FileRead | `Infinity` (opt-out) | trait default `100_000` (cannot express Infinity) | ❌ — see Phase 1.B |
| WebFetch / WebSearch / MCP / others | declared per-tool | inherited from default `100_000` | partial |

## Current Rust State

What exists:
- `Tool::max_result_size_chars() -> i32` trait method (`core/tool-runtime/src/traits.rs:293`).
- Per-tool values declared correctly for Bash/PowerShell/Grep/Glob.
- A bespoke, **non-conforming** Bash-only persistence: `core/tools/src/tools/bash.rs::maybe_persist_oversized_output` writes to `std::env::temp_dir()/coco-bash-output/`, attaches `persistedOutputPath`/`persistedOutputSize` JSON fields **without** replacing `stdout` content, no idempotency, no `<persisted-output>` wrapper.

What is missing:
- Generic Level 1 pipeline (`processToolResultBlock`).
- `<persisted-output>` content replacement (model still sees full content for Bash; not even attempted for other tools).
- 2KB preview generation with newline-aware truncation.
- Session-scoped storage directory (currently `temp_dir()` for Bash only).
- `wx`-flag idempotency.
- Empty-content `(<toolName> completed with no output)` guard.
- Image-block bypass.
- Level 2 entirely: no `ContentReplacementState`, no `enforceToolResultBudget`, no transcript record, no resume reconstruction, no API-level group walking, no fork gap-fill.
- MCP variant (`mcpOutputStorage.ts`) — also missing.
- `Infinity` opt-out semantics (Rust returns `i32`, no sentinel).

## Phase 0 — Config stub (LANDED)

Mirrors the Snip / Collapse "config exists, runtime inert" pattern so callers can
stage settings + tests before the runtime pipeline ships. All defaults match
TS feature-stripped behavior (gate off, no enforcement).

Status: implemented in `coco-rs/common/config/src/compact_settings.rs`.

```rust
pub struct ToolResultBudgetConfig {
    pub enabled: bool,           // tengu_hawthorn_steeple — default false
    pub per_message_chars: i64,  // tengu_hawthorn_window — default 200_000
    pub persist_records: bool,   // transcript writes — default true
}
```

Settings key: `compact.tool_result_budget.{enabled,per_message_chars,persist_records}`.
Env keys: `COCO_COMPACT_TOOL_RESULT_BUDGET_ENABLE`, `COCO_COMPACT_TOOL_RESULT_BUDGET_PER_MESSAGE_CHARS`.

Per-tool overrides (`tengu_satin_quoll`) are **not** surfaced in this struct
— they belong on `Tool::max_result_size_chars()` once Phase 1.B migrates it to
`ResultSizeBound { Chars(i32), Unbounded }`.

## Phase 1 — Level 1 Pipeline (~1-2 days)

### Phase 1.A — Constants module

New file: `coco-rs/core/tool-runtime/src/tool_result_storage/constants.rs`

```rust
pub const DEFAULT_MAX_RESULT_SIZE_CHARS: i32 = 50_000;
pub const MAX_TOOL_RESULT_TOKENS: i32 = 100_000;
pub const BYTES_PER_TOKEN: i32 = 4;
pub const MAX_TOOL_RESULT_BYTES: i32 = MAX_TOOL_RESULT_TOKENS * BYTES_PER_TOKEN;
pub const PREVIEW_SIZE_BYTES: usize = 2_000;
pub const TOOL_RESULTS_SUBDIR: &str = "tool-results";
pub const PERSISTED_OUTPUT_TAG: &str = "<persisted-output>";
pub const PERSISTED_OUTPUT_CLOSING_TAG: &str = "</persisted-output>";
pub const TOOL_RESULT_CLEARED_MESSAGE: &str = "[Old tool result content cleared]";
```

**Ownership direction**: `coco-tool-runtime` is the canonical owner — the
persistence pipeline lives there and that's where TS has the constants too
(`utils/toolResultStorage.ts` reaches into `constants/toolLimits.ts`).

The cleared-message marker is currently duplicated:

| Crate | Constant | Location |
|---|---|---|
| `coco-tool-runtime` (planned) | `TOOL_RESULT_CLEARED_MESSAGE` | `tool_result_storage/constants.rs` |
| `coco-compact` (today) | `CLEARED_TOOL_RESULT_MESSAGE` | `services/compact/src/types.rs:64` |

The Rust name `CLEARED_TOOL_RESULT_MESSAGE` should rename to TS-aligned
`TOOL_RESULT_CLEARED_MESSAGE` during the port to keep cross-language grep parity.

`coco-compact` does **not** currently depend on `coco-tool-runtime`
(verified: `services/compact/Cargo.toml` lists `coco-types`, `coco-inference`,
`coco-messages` only). Two options for de-duplication:

1. **Add `coco-tool-runtime` as a `coco-compact` dep, then re-export.** Both
   crates are L3 → intra-layer dep is allowed, no cycle (`coco-tool-runtime`
   does not depend on `coco-compact`). Simpler dep graph cost than option 2.
2. **Move the shared constant to `coco-types` (L1).** Lower coupling — every
   crate can read it without crossing layer 3. Use this if option 1 ends up
   adding more `coco-tool-runtime` types into `coco-compact` than just the one
   string.

Pick at port time based on whether other Phase 1 types (e.g. `PersistedToolResult`)
are needed inside `coco-compact`. If only the marker string is shared, option 2
is cleanest.

### Phase 1.B — Trait surface

Change `Tool::max_result_size_chars` return type:

```rust
// BEFORE
fn max_result_size_chars(&self) -> i32 { 100_000 }

// AFTER
fn max_result_size_chars(&self) -> ResultSizeBound { ResultSizeBound::Chars(100_000) }

pub enum ResultSizeBound {
    /// Persist when content exceeds `min(value, DEFAULT_MAX_RESULT_SIZE_CHARS)`.
    Chars(i32),
    /// Tool opts out of size-based persistence (TS Infinity). Used by FileRead
    /// which self-bounds via `maxTokens` so wrapping its output in
    /// `<persisted-output>` then re-reading the same file would be circular.
    Unbounded,
}
```

Migrate the four declared values; leave default at `Chars(100_000)`.

### Phase 1.C — Storage module

New file: `coco-rs/core/tool-runtime/src/tool_result_storage/persist.rs`

```rust
pub struct PersistedToolResult {
    pub filepath: PathBuf,
    pub original_size: usize,
    pub is_json: bool,
    pub preview: String,
    pub has_more: bool,
}

pub fn resolve_threshold(declared: ResultSizeBound) -> Option<i32>;
pub fn generate_preview(content: &str, max_bytes: usize) -> (String, bool);
pub fn build_large_tool_result_message(result: &PersistedToolResult) -> String;
pub fn is_content_already_compacted(content: &str) -> bool;
pub fn is_tool_result_content_empty(content: &ToolResultContent) -> bool;

pub async fn persist_tool_result(
    content: &ToolResultContent,
    tool_use_id: &str,
    storage_root: &Path,
) -> Result<PersistedToolResult, PersistError>;

pub async fn maybe_persist_large_tool_result(
    block: ToolResultBlock,
    tool_name: &str,
    threshold: ResultSizeBound,
    storage_root: &Path,
) -> ToolResultBlock;
```

Storage root resolution — defer to `coco-session::TranscriptStore::tool_results_dir(session_id)` (new method, returns `<projectDir>/<sessionId>/tool-results/`). Never `temp_dir()`.

Idempotency: `OpenOptions::new().write(true).create_new(true)` → `ErrorKind::AlreadyExists` tolerated, fall through to preview generation from existing file (NOT from in-memory bytes — read back to ensure byte-identical).

Empty guard: when content is empty/whitespace-only, return `"(${tool_name} completed with no output)"`.

Image bypass: `hasImageBlock(content) → true` → return block unchanged.

### Phase 1.D — Executor wiring

Extend `coco-tool-runtime::execution::execute_tool_call` to call the persistence pipeline **after** `tool.execute(...)` returns Ok, before constructing `ToolExecutionResult`. The post-tool-result transformation happens at the executor level — `Tool::execute` returns the original `ToolResult<Value>`, and the runtime wraps it.

Pseudocode (insert after current line 169 `let duration_ms = ...`):

```rust
let result = match result {
    Ok(tr) => Ok(persist_if_oversize(tr, &tool, ctx, tool_use_id).await),
    Err(e) => Err(e),
};
```

Where `persist_if_oversize` consults `tool.max_result_size_chars()` and `ctx.storage_root()`. Storage root is threaded through `ToolUseContext` (new field `tool_results_root: Option<PathBuf>`; absent disables persistence — covers test harness).

### Phase 1.E — Bash refactor

Replace `bash.rs::maybe_persist_oversized_output` with delegation to the generic helper. Remove the parallel `persistedOutputPath`/`persistedOutputSize` JSON fields (TS doesn't expose them — the wrapper string IS the message-visible artifact). Bash keeps the `outputSchema` shape that has those keys for **internal** tool result construction, but the wire content shown to the model is the `<persisted-output>` envelope.

Drop the `temp_dir()` path. All Bash output flows through `tool_results_root`.

### Phase 1.F — Tests

| Test | Location |
|---|---|
| `generate_preview` truncates at last newline within upper half | `tool_result_storage/persist.test.rs` |
| `build_large_tool_result_message` produces TS-byte-identical wrapper | same |
| `persist_tool_result` is idempotent (second call returns same preview without rewriting) | same |
| `maybe_persist_large_tool_result` skips images | same |
| `maybe_persist_large_tool_result` skips already-compacted content | same |
| Executor end-to-end: oversize Bash → wire content starts with `<persisted-output>` | `core/tool-runtime/tests/persist_e2e.rs` |
| Per-tool threshold parity (Bash 30k, Grep 20k, Glob 100k, PowerShell 30k) | already covered |
| `Tool::max_result_size_chars() == ResultSizeBound::Unbounded` for FileRead | new |

## Phase 2 — Level 2 Per-Message Budget (~2-3 days)

### Phase 2.A — State types

New file: `coco-rs/core/tool-runtime/src/tool_result_storage/budget.rs`

```rust
pub struct ContentReplacementState {
    pub seen_ids: HashSet<String>,
    pub replacements: HashMap<String, String>,
}

pub fn create_content_replacement_state() -> ContentReplacementState;
pub fn clone_content_replacement_state(src: &ContentReplacementState) -> ContentReplacementState;

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ContentReplacementRecord {
    ToolResult { tool_use_id: String, replacement: String },
}
```

State lives on `app/query::QueryEngine` (single instance per conversation
thread). Subagent spawns clone the parent's state at fork time. Resume
reconstructs from transcript.

### Phase 2.B — Budget enforcement

```rust
pub async fn enforce_tool_result_budget(
    messages: &[Message],
    state: &mut ContentReplacementState,
    skip_tool_names: &HashSet<String>,
    tool_results_root: &Path,
) -> EnforcementResult { /* … */ }

pub async fn apply_tool_result_budget(
    messages: Vec<Message>,
    state: Option<&mut ContentReplacementState>,
    write_to_transcript: Option<&dyn Fn(&[ContentReplacementRecord])>,
    skip_tool_names: &HashSet<String>,
    tool_results_root: &Path,
) -> Vec<Message>;
```

### Phase 2.C — API-group walking

Mirror TS `collectCandidatesByMessage` semantics:
- A "group" is a maximal run of user messages NOT separated by an assistant message with a **new** message ID. (Same-ID assistant fragments don't create boundaries — they merge in `normalizeMessagesForAPI`.)
- `progress` / `attachment` / `system` types do NOT create boundaries.

This is the trickiest part to align — see `enforceToolResultBudget` in TS for
the exact `seenAsstIds` invariant. Wrong grouping silently breaks Level 2 in
parallel-tool turns.

### Phase 2.D — Resume reconstruction

```rust
pub fn reconstruct_content_replacement_state(
    messages: &[Message],
    records: &[ContentReplacementRecord],
    inherited_replacements: Option<&HashMap<String, String>>,
) -> ContentReplacementState;

pub fn reconstruct_for_subagent_resume(
    parent_state: Option<&ContentReplacementState>,
    resumed_messages: &[Message],
    sidechain_records: &[ContentReplacementRecord],
) -> Option<ContentReplacementState>;
```

Persistence: `coco-session::TranscriptStore` adds two new methods analogous
to the marble-origami pair: `append_content_replacement_record` and
`load_content_replacement_records`. Records key off session id.

### Phase 2.E — Query-engine wiring

Insert call **before** micro-compact in `app/query/src/engine_finalize_turn.rs`
(or `engine_prompt.rs`, whichever site builds the per-API messages). TS
invokes from `query.ts:379`, before the snip / autocompact escalation. Rust
should keep the same ordering: budget → snip-stub → micro → autocompact.

Skip-tool list: built from `ToolRegistry::iter()` filtering tools whose
`max_result_size_chars()` returns `ResultSizeBound::Unbounded`.

### Phase 2.F — Feature gating

Already wired by Phase 0: `coco_config::ToolResultBudgetConfig` lives in
`compact_settings.rs` with the three fields below. Phase 2 just reads them.

| Field | TS gate | Default | Behavior when off |
|---|---|---|---|
| `enabled` | `tengu_hawthorn_steeple` | `false` | `provision_content_replacement_state` returns `None`; `apply_tool_result_budget` is pass-through |
| `per_message_chars` | `tengu_hawthorn_window` (override) | `200_000` | n/a (always read when `enabled`) |
| `persist_records` | — (transcript writes) | `true` | Skip `recordContentReplacement`-equivalent writes; cache stability still works in-memory for ephemeral fork agents |

### Phase 2.G — Tests

| Test | Location |
|---|---|
| API-group walking: same-ID assistant fragments don't split | `tool_result_storage/budget.test.rs` |
| Frozen-overage accepted (replays cached preview, doesn't re-pick) | same |
| Selection picks largest first | same |
| `Unbounded` tools skipped | same |
| Resume reconstruction = original session decisions | new |
| Subagent gap-fill from parent state | new |
| Cache-stability: 5 turns of replay produce byte-identical wire | E2E |

## Owner Re-Routing (mapping doc fixes)

The original `audit-gaps.md` entry routed the entire file to `coco-context`
and described only Level 2. That's the root cause of multi-round review
miss. Updated routing:

| TS surface | Rust crate | Doc owner |
|---|---|---|
| `utils/toolResultStorage.ts::persistToolResult` + `processToolResultBlock` + preview helpers | `coco-tool-runtime::tool_result_storage` | this plan + `crate-coco-tool-runtime.md` |
| `utils/toolResultStorage.ts::ContentReplacementState` + `enforceToolResultBudget` + `applyToolResultBudget` | `coco-tool-runtime::tool_result_storage::budget` (state + enforcement) | this plan |
| Query-loop integration (`query.ts:379` callsite) | `coco-query` | `crate-coco-query.md` |
| Transcript records (`recordContentReplacement` + `LogOption.contentReplacements`) | `coco-session::TranscriptStore` | `crate-coco-app.md` (session subsection) |
| Per-tool `maxResultSizeChars` declarations | `coco-tools` | `crate-coco-tools.md` |
| `constants/toolLimits.ts` | `coco-tool-runtime::tool_result_storage::constants` | this plan |
| `utils/mcpOutputStorage.ts` | `coco-tool-runtime` (parallel module) | this plan, deferred to Phase 3 |
| Cross-reference (compact capability cluster) | `coco-compact` | `crate-coco-compact.md` (cross-ref only, no impl) |

## Phase 3 — MCP Output Storage (deferred)

`utils/mcpOutputStorage.ts` is a parallel pipeline for MCP server
responses. Phase 3 ports it after Phase 1+2 land, since it depends on the
same storage primitives.

## Cache-Stability Invariants

1. **Once a tool_use_id is seen, its replacement decision is frozen.**
   - Replaced → re-apply the byte-identical replacement string every turn.
   - Not-replaced → never replace later (would change a prefix already cached).
2. **Replacement strings are stored verbatim, not regenerated.**
   - TS stores the full message string; Rust must do the same. Code-template
     drift (e.g. size formatting changes) cannot silently break cache.
3. **Subagent forks clone parent state at fork time.**
   - Cache-sharing forks (agent_summary, fork-with-cache) need byte-identical
     wire prefix → must inherit the parent's replacement decisions.
4. **Resume reconstructs decisions from records.**
   - Records key off `tool_use_id` (UUID) so post-`/clear` / post-rewind
     stale entries are harmless.

Violating any of these silently invalidates the prompt cache for the
remainder of the conversation. Add an integration test that asserts the
wire-prefix bytes are byte-identical across N turns of replay.

## Verification Checklist (post-implementation)

- [ ] `Tool::max_result_size_chars()` is read by the executor for every tool call.
- [ ] Bash output > 30K is replaced inline with `<persisted-output>` (not extra JSON fields).
- [ ] Storage path is session-scoped, not `temp_dir()`.
- [ ] Re-running a session → same files on disk (idempotency).
- [ ] FileRead is exempt from persistence (`Unbounded`).
- [ ] Empty Bash success returns `(Bash completed with no output)` to model.
- [ ] Image content blocks pass through unchanged.
- [ ] Five Bash turns of 100K output → wire byte-identical replacement strings (cache stability).
- [ ] Eight parallel Greps each 25K → Level 2 picks largest, total ≤ 200K.
- [ ] Resume of session with persisted records reproduces exact wire bytes.
- [ ] Subagent fork inherits parent replacements.

## TODO Markers (in source until Phase 1 lands)

- `core/tools/src/tools/bash.rs::maybe_persist_oversized_output` — comment
  references this plan as Phase 1.E.
- `core/tool-runtime/src/traits.rs::max_result_size_chars` — comment
  references the planned `ResultSizeBound` migration.

## Open Questions

1. **Storage root for non-session callers** (test harness, one-shot SDK calls). Proposal: `ctx.tool_results_root = None` disables persistence; the Bash special case currently uses `temp_dir()` and breaks idempotency. Phase 1 closes this by making storage opt-in via `ctx`.
2. **Cleanup policy.** TS `utils/cleanup.ts:198` deletes `<sessionDir>/tool-results/` on session expiry. Rust mirror: `coco-session::cleanup` to add `cleanup_tool_results_dir`.
3. **Concurrent fork persistence.** Two subagents persisting the same `tool_use_id` (impossible in TS because each fork has a fresh UUID space, but worth asserting). Add a debug-only invariant check.
4. **Per-message budget vs. context_management.** When the Anthropic-only API-native `clear_tool_uses_20250919` is active, Level 2 may double-clear. TS doesn't dedupe — it lets the server clear what the client preview replaced. Rust should match: the preview is a tag-wrapped string, not a tool_result_content array, so the server's `clear_tool_uses` operation has nothing to clear in those messages anyway.
