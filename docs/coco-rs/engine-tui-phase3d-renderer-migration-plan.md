# Phase 3d — Renderer Pipeline Migration to `RenderedCell`

Date: 2026-05-19

Companion to `engine-tui-unified-transcript-plan.md`. Picks up where
Phase 3c left off and completes the migration that finally allows
deletion of `ChatMessage`, `MessageContent`, `session.messages`, and
the `merged_chat_messages` back-adapter.

## Landing Status (2026-05-19)

Commits 1, 2, and 3 landed as documented below. Commits 4 and 5 were
condensed into a "Pragmatic close-out" pass instead of the full
renderer rewrite:

- ✅ Commit 1 (Section 2.1) — `SessionRuntime.history: Arc<Mutex<MessageHistory>>`.
- ✅ Commit 2 (Section 2.2) — `UserCommand::PushSystemMessage` + engine
  round-trip for all TUI-originated transcript content. `SubmitInput`
  and `run_prompt_mode_bash` now push through `history_push_and_emit`;
  10 of 13 `add_message` sites converted, 3 removed (bash input,
  user_text submit, memory cancel toast). Auto-restore predicates
  switched to the merged view. `TranscriptView::on_message_appended`
  dedups by UUID so multi-turn re-emission doesn't accumulate
  duplicate cells.
- ✅ Commit 3 (Section 2.3) — adapter variant. Instead of the
  `TranscriptSource<'a> { Legacy | Cells }` dual-path rewrite of the
  646-LoC `presentation::transcript` module,
  `SessionState::transcript_messages()` builds the merged view
  (legacy `session.messages` + engine cells) and every transcript-
  pipeline caller threads the merged slice through.
  `TranscriptPresentationInput` lifetime split into `<'msg, 'state>`
  so the projection can be fed a temporary `Vec<ChatMessage>` without
  pinning the output. `transcript_modal.rs` builds the merged view
  once per render and passes it to projection / renderer / hash
  helpers / `cell_id` lookups.
- 🟡 Commits 4 + 5 condensed into "Pragmatic close-out" — every
  remaining `&state.session.messages` reader on the rendering /
  picker / status path was switched to
  `state.session.transcript_messages()` (rewind picker, status bar,
  footer, activity preview). `session.messages` was documented as
  vestigial. The full `RenderedCell` rewrite of the 4 `render_*.rs`
  files + `ChatWidget` was deferred; the renderer continues to
  consume `&[ChatMessage]` via the merged adapter. `ChatMessage`,
  `MessageContent`, and `merged_chat_messages` are retained as
  render-time projection types — they no longer serve as the source
  of truth but are kept because deleting them would require ~2000
  LoC of mechanical match-arm replacement plus test-suite churn
  that is out of scope for this phase.

End-state invariants achieved:

- **I-1 Authority** — engine `MessageHistory` is the single source of
  truth for the conversation chain. `runtime.history` is the
  authoritative live view; resume / clear / rewind all flow through
  `MessageAppended` / `MessageTruncated` / `SessionResetForResume`.
- **I-2 Derived view** — TUI cells are derived from `&Message` via
  `derive::message_to_cells`. The merged-ChatMessage adapter
  (`cells_to_chat_messages` + `merged_chat_messages`) is now a
  rendering convenience layer, not a parallel state store.
- **I-3 UI-only state stays UI-only** — `ui.streaming`,
  `session.tool_executions`, transcript-modal collapse state, etc.

Outstanding deltas vs the original plan §4/§5:

- `chat::ChatWidget`, `render_user.rs`, `render_assistant.rs`,
  `render_tool.rs`, `render_system.rs` continue to take
  `messages: &[ChatMessage]`. Future rewrite to `&[RenderedCell]` is
  isolated to these files and
  `surface/{viewport,controller,history_lines}.rs`.
- `session.messages` field still exists. Direct writers reduced to
  the thinking-metadata synthesis path
  (`apply_reasoning_tokens_to_response` in `protocol.rs`), the
  Ctrl+L clear, and the rewind-picker truncate. All three are
  candidates for full removal in the deferred §4/§5 rewrite.
- `ChatMessage`, `MessageContent`, `cell_to_chat_message`,
  `cells_to_chat_messages`, `merged_chat_messages` retained — see
  the "Vestigial" doc on `SessionState::messages`.

## 0. Starting State

After Phase 3c (commit `69a6a30f6`):

- ✅ Engine `MessageHistory` is single source of truth; every push
  emits `ServerNotification::MessageAppended { message: Value }`.
- ✅ TUI `TranscriptView` is populated from those events; each
  `RenderedCell` carries `source: Arc<Message>` so renderers can
  recover engine-authoritative fields.
- ✅ Render pipeline reads a merged view (`session.messages` +
  transcript-derived `ChatMessage`s) via `merged_chat_messages`.
- ✅ Engine-pushed content (cancel marker, resume scrollback, hooks,
  system notifications) is visible.
- ❌ `ChatMessage` / `MessageContent` (40 variants) still exist.
- ❌ `session.messages: Vec<ChatMessage>` still exists; 13 TUI-only
  `add_message` call sites still write to it.
- ❌ Render dispatch (4 `render_*.rs` files + `presentation/transcript.rs`,
  ~2200 LOC) still operates on `[ChatMessage]` and `MessageContent`.
- ❌ `SessionRuntime.history` is `Arc<Mutex<Vec<Message>>>`, not
  `Arc<Mutex<MessageHistory>>`. Engine pushes inside `coco-query` work
  with `&mut MessageHistory` (via `history_push_and_emit`), but
  TUI-initiated pushes from `app/cli` would need conversion at the
  lock boundary.

## 1. Goals

End state:

1. Single representation: render path consumes `&[RenderedCell]`
   directly. `ChatMessage` / `MessageContent` deleted.
2. Single source: `session.messages` deleted. TUI-originated transcript
   content (slash command output, plan markers, memory dialogs, …)
   flows through a new `UserCommand::PushSystemMessage` that round-
   trips through the engine and re-emerges via `MessageAppended`.
3. No regression: every variant of the legacy `MessageContent`
   currently in use renders via a corresponding `CellKind` arm.

Per the unified-transcript-plan invariants:

- I-1 Authority: engine `MessageHistory` is single source of truth.
- I-2 Derived view: TUI cells are derived from `&Message`.
- I-3 UI-only state stays UI-only (`ui.streaming`, `tool_executions`).

## 2. Commit Sequence (5 commits)

Order is **bottom-up infrastructure, then top-down renderer, then
cleanup**. Each commit compiles and passes `just quick-check`. Tests
are optional per existing project decision (Phase 3 carries no test
work); landing on a clean compile is the gate.

### Commit 1 — `SessionRuntime.history` carries a typed `MessageHistory`

Goal: every lock-holder of `runtime.history` works with the same
`MessageHistory` wrapper the engine loop uses internally. Unblocks
Commit 2 (which needs `history_push_and_emit` at TUI-initiated push
sites).

Files:

- `app/cli/src/session_runtime.rs` — change field
  `pub history: Arc<Mutex<Vec<Message>>>` to
  `pub history: Arc<Mutex<MessageHistory>>`.
- Every lock site: replace `let mut h = runtime.history.lock().await;
  h.push(msg)` with `h.push(msg)` (now method on MessageHistory) or
  `coco_query::history_sync::history_push_and_emit(&mut *h, msg,
  &event_tx).await`.
- Audit sites (from previous grep): `app/cli/src/tui_runner.rs`
  rewind path, `sdk_server/sdk_runner.rs:212`, any
  `.history.lock().await` callers.
- `MessageHistory::iter()`, `as_slice()`, `position()` confirm
  available on the wrapper (already are per `core/messages/src/history.rs`).
- `as_slice` returns `&[Message]`; `iter_arcs`-like is unnecessary at
  this commit — `Arc<Message>` storage is a later optimization, not a
  prerequisite.

Risk: `seed_transcript_dedup`, `seed_tool_result_replacement_state`,
and any other helpers that took `&[Message]` may need new signatures.
Most likely they accept `as_slice()` cleanly.

Net delta: ~80 LOC. No new variants.

### Commit 2 — `UserCommand::PushSystemMessage` + migrate TUI-only add_message sites

Goal: TUI-originated transcript content flows through the engine.
Removes the 10+ remaining `add_message` call sites by routing through
a typed command that the engine handles by pushing a
`Message::System(...)` and emitting `MessageAppended`.

New protocol:

```rust
// app/tui/src/command.rs
UserCommand::PushSystemMessage {
    kind: SystemPushKind,
    /// Variant-specific payload. Engine constructs the matching
    /// SystemMessage sub-variant from the typed kind + payload.
    payload: serde_json::Value,
}

pub enum SystemPushKind {
    Informational,   // slash command status, toast → SystemMessage::Informational
    LocalCommand,    // bash output → SystemMessage::LocalCommand
    PermissionRetry, // permission retry banner
    BridgeStatus,    // IDE bridge connect/disconnect notice
    MemorySaved,     // memory dialog write notice
    // ... extend per migration audit below
}
```

Engine handler (in `app/cli/src/tui_runner.rs` UserCommand match):

```rust
UserCommand::PushSystemMessage { kind, payload } => {
    let msg = build_system_message(kind, payload);
    let mut h = runtime.history.lock().await;
    coco_query::history_sync::history_push_and_emit(
        &mut *h, msg, &Some(event_tx.clone()),
    ).await;
}
```

Migration audit table (all 13 sites, mapped to PushSystemMessage):

| Site | Today's `add_message` | After: PushSystemMessage kind | Notes |
|---|---|---|---|
| `protocol.rs:742` SlashCommandStatus | `system_text` | Informational (title=status, message=text) | Persistent transcript |
| `protocol.rs:1002` teammate_message | `teammate_message` | New `TeammateMessage` SystemPushKind, payload `{teammate, content}` | Surface via SystemMessage::Informational or new sub-variant |
| `tui_only.rs:258` memory open | `system_text` | Informational | |
| `tui_only.rs:299, 310, 321, 332` plan/editor states | `system_text` | Informational | |
| `tui_only.rs:372` | `system_text` | Informational | |
| `tui_only.rs:413` | `system_text` | Informational | |
| `update.rs:323` Toast → system | `system_text` | Decide: if persistent → Informational; if transient → keep as `ui.toast` (no transcript entry) | Audit case-by-case |
| `update/edit.rs:55` bash input | `user_bash_input` | This is User-role, not System. Engine `Message::User` push from `UserCommand::SubmitBash` already covers it. **Remove this site.** | Engine UUID flows back via MessageAppended |
| `update/edit.rs:117` user_text on submit | `user_text` | Same — engine `Message::User` push from `UserCommand::SubmitInput` covers. **Remove this site.** | |
| `update/interaction.rs:325` | inline | Per case: Informational or ui.toast | |

Net: 13 sites collapse to 0 (10 routed through PushSystemMessage, 3
removed because engine path already covers).

The TUI-side ChatMessage construction is gone. session.messages no
longer receives writes — it's now exclusively a render-time cache
(populated by `merged_chat_messages` but never written to directly).
That gates Commit 5.

Files: `app/tui/src/command.rs`, `common/types/src/client_request.rs`
(if PushSystemMessage is SDK-visible — likely TUI-internal so just
command.rs), `app/cli/src/tui_runner.rs` handler, and the 13 add_message
sites.

Net delta: ~250 LOC removed + ~120 LOC added (protocol + handler).

### Commit 3 — `presentation::transcript` accepts `RenderedCell`

Goal: the layout-projection layer (646 LOC) takes `&[RenderedCell]`
instead of `&[ChatMessage]` and produces `TranscriptCell`s as before.
This is the largest semantic change but architecturally focused: only
one module's input type changes.

Files: `app/tui/src/presentation/transcript.rs`

Approach:

1. Introduce an enum at the input boundary:
   ```rust
   pub enum TranscriptSource<'a> {
       Legacy(&'a [ChatMessage]),
       Cells(&'a [RenderedCell]),
   }
   ```
   `transcript_presentation` accepts `TranscriptSource`. Most code paths
   internally walk the source and produce `TranscriptCell` regardless.
2. Implement a `match` at each projection step that dispatches:
   - `MessageContent::X` → existing layout logic
   - `CellKind::Y` → new layout logic for cell-based source
3. Where the two converge (e.g. wrapping text into Lines), share the
   leaf helpers.

Risk: 205 `MessageContent::` match arms inside this module + nested
`if let MessageContent::X = …` patterns. Each needs a `CellKind`
mirror. Bulk of the LOC is mechanical translation:

| `MessageContent` variant | `CellKind` arm |
|---|---|
| `Text(text)` | `CellKind::UserText { text }` |
| `AssistantText(text)` | `CellKind::AssistantText { text, .. }` |
| `Thinking { content, .. }` | `CellKind::AssistantThinking { text }` |
| `ToolUse { tool_name, call_id, input_preview, status }` | `CellKind::ToolUse { call_id, tool_name }` (cell.source provides full Message::Assistant ToolCall block for `input_preview`) |
| `ToolSuccess / ToolError` | `CellKind::ToolResult { call_id }` (cell.source provides ToolResultMessage content) |
| `InterruptionMarker { for_tool_use }` | `CellKind::System(SystemCellKind::UserInterruption { for_tool_use })` |
| `SystemText`, `ApiError`, `RateLimit`, `Shutdown*`, `Hook*`, `PlanApproval`, `Compact*`, `Advisor`, `TaskAssignment` | `CellKind::System(SystemCellKind::X)` + read details from `cell.source` |

Where CellKind doesn't carry enough info (e.g. `FileEditDiff` hunks),
the projection extracts from `cell.source: Arc<Message>` — that's why
RenderedCell carries the back-pointer.

Net delta: ~1500 LOC churn (rewrite of transcript_presentation).
Compile-driven; each `match` arm is mechanical.

### Commit 4 — `render_*.rs` + `ChatWidget` consume `RenderedCell`

Goal: the 4 render submodules (`render_user`, `render_assistant`,
`render_tool`, `render_system` — together ~728 LOC) and `ChatWidget`
(792 LOC) switch their inputs to `&[RenderedCell]`.

Files:

- `app/tui/src/widgets/chat/mod.rs` — `ChatWidget::new(messages:
  &'a [RenderedCell], styles)`.
- `app/tui/src/widgets/chat/render_user.rs`, `render_assistant.rs`,
  `render_tool.rs`, `render_system.rs` — match `&RenderedCell` /
  `&CellKind` instead of `&ChatMessage` / `&MessageContent`.
- `app/tui/src/surface/viewport.rs`, `app/tui/src/surface/controller.rs`
  — drop `merged_chat_messages` adapter; pass
  `state.session.transcript.cells()` directly.
- `app/tui/src/surface/history_lines.rs` — same.
- Transcript modal (`widgets/transcript_modal.rs:490`) — same.

After this commit, the renderer only consumes cells. The
`merged_chat_messages` / `cell_to_chat_message` adapter from
`derive.rs` becomes unused. Defer deletion to Commit 5 so Commit 4
stays focused on the renderer switch.

Risk: many internal helpers in `render_*` use `ChatMessage.id`,
`ChatMessage.role`, etc. Since RenderedCell carries `message_uuid` +
`source: Arc<Message>`, equivalent fields are reachable; this is a
mechanical rewrite.

Net delta: ~2000 LOC churn across 6 files.

### Commit 5 — Delete `ChatMessage`, `MessageContent`, `session.messages`, adapter

Goal: removal pass. After Commits 1-4, nothing writes to
`session.messages` and nothing reads `ChatMessage` / `MessageContent`
in production code.

Files:

- `app/tui/src/state/session.rs` — delete `messages: Vec<ChatMessage>`
  field + `Default` init + `add_message` method + `ChatMessage` struct
  + `MessageContent` enum + all associated helpers (`tool_success`,
  `tool_error`, `interruption_marker`, `user_bash_input`, etc.).
- `app/tui/src/state/mod.rs` — drop the `pub use session::ChatMessage`
  / `pub use session::MessageContent` re-exports.
- `app/tui/src/state/derive.rs` — delete `cell_to_chat_message`,
  `cells_to_chat_messages`, `merged_chat_messages`,
  `extract_message_metadata`, `extract_api_error`,
  `extract_informational`. Keep only `message_to_cells`.
- `app/tui/src/server_notification_handler/projection.rs` — already
  reduced to `state.ui.streaming = None` (Phase 3c); confirm no
  ChatMessage construction remains and consider folding into the
  on_turn_completed handler.
- Any straggler readers of `session.messages` (e.g. test files,
  `update_rewind.rs` for the "messages-after-are-only-synthetic"
  predicate) — switch to `session.transcript.cells()` semantics.
- `audit-gaps.md` — close the `/resume` TUI hydration row.
- `crate-coco-tui.md` — update to reflect the unified-cell model.

Risk: test files reference deleted types. Per project decision
(Phase 3 no-test), update tests minimally to compile; do not chase
test correctness.

Net delta: ~−800 LOC.

## 3. Per-Commit Compile Gate

Each commit must pass `just quick-check` before commit. The standing
project rule: `just pre-commit` runs once at the very end of the
sequence (user-initiated).

```
edit → just quick-check → commit
       (fix any clippy)         ↑
                                 \— do all 5 commits, then optionally
                                    `just pre-commit` before push.
```

## 4. Acceptance Criteria

This phase is complete when:

1. ✅ `coco-tui` contains no `ChatMessage`, `MessageContent`,
   `add_message`, `session.messages`.
2. ✅ Render path (viewport / controller / history_lines / chat widget /
   render_* / transcript_presentation / transcript_modal) consumes
   `RenderedCell` / `CellKind` directly.
3. ✅ All TUI-originated transcript content flows through
   `UserCommand::PushSystemMessage` → engine → `MessageAppended` →
   transcript.
4. ✅ `SessionRuntime.history` is `Arc<Mutex<MessageHistory>>`.
5. ✅ `derive.rs` contains only `message_to_cells` (the forward
   adapter). All back-adapters deleted.
6. ✅ `just quick-check` passes across all 5 commits.

## 5. Out of Scope (Defer to Future Sessions)

- Layout caching on `RenderedCell` (`cached_lines`, `cached_height`):
  the plan §6.1 mentioned per-cell layout cache. Practical when the
  renderer flicker on large transcripts becomes a measurable problem.
- `Arc<Message>` storage inside `MessageHistory.messages`:
  optimization where `MessageHistory::push` returns an `Arc<Message>`
  to avoid the engine→event clone. Defer until profiling shows the
  Message clone is hot.
- Streaming overlay refactor: the streaming-tail widget currently
  shows live `ui.streaming` content. After Phase 3d this can be a
  separate widget that anchors to the in-flight cell UUID directly.
- SDK transcript view API (`session/read`'s message pagination):
  separate cross-cut, see plan §11.
- Coordinator merged-timeline view (parent_session_id filtering).

## 6. References

- `engine-tui-unified-transcript-plan.md` — overarching plan; §6
  describes the target TUI model (TranscriptView + RenderedCell +
  CellKind), §6.6 lists the original 20 add_message sites with their
  disposition.
- `crate-coco-messages.md` — canonical Message + SystemMessage shapes
  consumed by the migrated renderers.
- `crate-coco-tui.md` — current SessionState description; will be
  rewritten as part of Commit 5.
- Git history this session (`feat/tui` branch tip, four commits):
  `8fdb92e80`, `cf15b0b82`, `1e6fe76af`, `69a6a30f6`.
