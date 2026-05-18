# Engine ↔ TUI Unified Transcript Plan

Date: 2026-05-18

Supersedes `engine-tui-message-bridge-plan.md` and
`engine-tui-transcript-economical-plan.md` (both deleted).

This plan **does not redefine** canonical types. Authoritative definitions
live in:

- `crate-coco-messages.md` — `Message`, `SystemMessage`, `MessageHistory`,
  `Visibility`, `MessageKind`.
- `crate-coco-tui.md` — `AppState`, `SessionState`, `StreamingState`,
  `ToolExecution`.
- `event-system-design.md` — three-layer `CoreEvent`.

This doc owns the **cross-layer plan** that removes the engine/TUI message
representation split and stops the cancel-marker / resume / auto-restore
divergence by construction.

## 0. TL;DR

| Concern | Today | After plan |
|---|---|---|
| Representations of one message | Two (`Message` in engine, `ChatMessage`/`MessageContent` 40-variant enum in TUI) | **One** (`Message`). TUI render functions take `&Message`. |
| "Synthetic" UI rows (hooks, plan markers, rate limits, …) | TUI-only `MessageContent` variants, never enter engine history | Engine pushes `Message::System` with extended `SystemMessage` sub-variants. Round-tripped through history, visible to SDK, restored on resume. |
| Engine ↔ TUI sync | 19 ad-hoc `add_message` sites in TUI, independent of engine pushes | Engine pushes; emits one `MessageAppended { Arc<Message> }`. TUI is a pure derived view. |
| Cancel marker `for_tool_use` | Computed twice (engine + TUI) — race | Computed once in engine, stored on `SystemMessage::UserInterruption`, TUI reads field. |
| `/resume` TUI scrollback | Empty (engine seeds history, TUI not notified) | Engine emits one `MessageAppended` per loaded message; TUI rebuilds derived view via the same handler used for live turns. |
| Auto-restore round-trip | TUI truncates `session.messages` only; engine history retains content | Goes through `UserCommand::Rewind { mode: AutoRestore }`. Engine truncates. Emits `MessageTruncated`. SDK + TUI both see it. |
| Streaming + in-flight tools | Tracked in `ui.streaming` + `tool_executions` AND mirrored as `MessageContent` rows | Stay in `ui.streaming` + `tool_executions` as widget overlays. Not part of transcript. |

Net code delta: ~-300 LOC, no bridge module, no dual-write phase, no
`HistoryReplaced`-style full-history broadcasts.

## 1. The Three Invariants

These are the only durable rules of the design. Everything else is mechanism.

**I-1 — Authority.** `coco_messages::MessageHistory` is the single source
of truth for transcript content. No other store may claim authority. SDK,
TUI, persistence: all read from this one.

**I-2 — Derived view.** TUI render cells are pure functions of `&Message`.
The TUI may cache derived output for performance but never as an
authoritative store. On any transcript mutation, the derived view
reconciles to `MessageHistory`.

**I-3 — UI-only state stays UI-only.** Streaming in-flight text
(`ui.streaming`) and running tool executions (`tool_executions`) are
widget state, not transcript content. They are rendered as overlays
between the transcript and the input bar. When a streaming turn or tool
call finalizes, the engine pushes a `Message`; the overlay is then
cleared.

## 2. Layer Ownership and Pollution Rules

Per review feedback: `coco-messages` must not be polluted with UI
rendering details. Layer responsibilities are pinned as follows.

| Crate | Owns | Forbidden contents |
|---|---|---|
| `coco-messages` (L3) | `Message`, `SystemMessage` sub-variants, `MessageHistory`. Business-semantic metadata on messages (`is_meta`, `is_compact_summary`, `permission_mode`, `origin`, `visibility`). | Markdown rendering state, diff display state, hover/selection, viewport-relative state, theme/color, cell IDs, derived row sequence numbers. |
| `coco-types` (L1) | New `ServerNotification` variants, new `RewindMode` enum on existing `UserCommand::Rewind`. | Message content schema (belongs in `coco-messages`). |
| `coco-query` (L5) | Helper that pairs each `MessageHistory::push` with a `MessageAppended` emit; `finalize_user_cancel` collapsed to one site; `handle_rewind` reads the new `mode` field. | Per-cell UI choices. |
| `coco-tui` (L5) | `TranscriptView` (derived cell cache), `RenderedCell`, `CellKind`, the function `message_to_cell`, all rendering. UI-only state (`ui.streaming`, `tool_executions`, toasts, modal overlays). | Any persistent representation of transcript content. Re-computation of fields that exist on `Message` (e.g. `for_tool_use`). |
| `app/cli` | Resume wiring: load JSONL into `MessageHistory`, then emit a `MessageAppended` per message. | Direct TUI state mutation. |

The "derived view" function — call it `message_to_cell(&Message) ->
RenderedCell` — lives in `coco-tui`, **not** in `coco-messages`. This is
the load-bearing rule that protects layer hygiene.

## 3. Data Model

### 3.1 `Message` stays as-is structurally

`Message` (8 variants: User, Assistant, **System**, Attachment,
ToolResult, Progress, Tombstone, ToolUseSummary) needs no shape change.
Refer to `crate-coco-messages.md`.

### 3.2 `SystemMessage` extension absorbs TUI "synthetic" variants

Current `SystemMessage` already has 14 sub-variants (per
`crate-coco-messages.md` and `core/messages/src/types/message.rs:414`):
`Informational`, `ApiError`, `CompactBoundary`, `MicrocompactBoundary`,
`LocalCommand`, `PermissionRetry`, `BridgeStatus`, `MemorySaved`,
`AwaySummary`, `AgentsKilled`, `ApiMetrics`, `StopHookSummary`,
`TurnDuration`, `ScheduledTaskFire`.

TUI's 22-ish "synthetic" `MessageContent` variants map to `SystemMessage`
as follows. Some collapse into existing variants; some become new ones.

| Current `MessageContent` variant | Maps to |
|---|---|
| `InterruptionMarker { for_tool_use }` | **NEW** `SystemMessage::UserInterruption { for_tool_use: bool }` |
| `ApiError` | existing `SystemMessage::ApiError` |
| `CompactBoundary`, `CompactSummary` | existing `SystemMessage::CompactBoundary` (carry summary fields on it; extend if needed) |
| `HookSuccess`, `HookNonBlockingError`, `HookBlockingError`, `HookCancelled`, `HookSystemMessage`, `HookAdditionalContext`, `HookStoppedContinuation`, `HookAsyncResponse` | **NEW** `SystemMessage::HookOutcome { hook_name, outcome: HookOutcomeKind, payload }` — collapses 8 ad-hoc TUI variants into one System variant + `HookOutcomeKind` enum |
| `PlanMarker`, `PlanApproval` | **NEW** `SystemMessage::PlanEvent { event: PlanEventKind, ... }` |
| `RateLimit`, `Shutdown`, `ShutdownRequest`, `ShutdownRejected` | **NEW** `SystemMessage::SessionLifecycle { kind: LifecycleKind, ... }` (or fold into `Informational` with `level` field) |
| `Advisor`, `TaskAssignment` | **NEW** `SystemMessage::Coordinator { kind: CoordinatorKind, ... }` |
| `ChannelMessage`, `ResourceUpdate`, `AgentNotification`, `TeammateMessage` | **NEW** `SystemMessage::ExternalNotification { source, kind, payload }` (these flow from coordinator / MCP / bridge into the user's transcript) |
| `SystemText` (catch-all) | existing `SystemMessage::Informational` |
| `Attachment` | `Message::Attachment` (already a separate top-level variant) |
| `BashInput`, `BashOutput` | `Message::User` / `Message::ToolResult` respectively (these are real interactions, not synthetic) |
| `Image` | `Message::User` with image content part |
| `AssistantText`, `Thinking`, `RedactedThinking`, `ToolUse` | content blocks inside `Message::Assistant`; no new variant |
| `ToolSuccess`, `ToolError`, `ToolRejected`, `ToolCanceled` | `Message::ToolResult` with status field; no new variant |
| `FileEditDiff`, `FileWriteResult` | rendered from `Message::ToolResult.content`; no enum variant (render concern) |

Variant collapse: 40 TUI `MessageContent` variants → 8 existing
`Message` + `SystemMessage` shapes plus ~5 new `SystemMessage` sub-variants
(`UserInterruption`, `HookOutcome`, `PlanEvent`, `SessionLifecycle` or
`Informational`-folded, `Coordinator`, `ExternalNotification`).

Exact sub-variant set is finalized during Phase 1 review; the principle
is **all transcript-visible synthetic rows are real `Message`s**, mirroring
TS where `HookResultMessage` is just a `Message`.

### 3.3 What does NOT change

- `Message::Attachment` stays as is.
- `Message::Progress`, `Message::Tombstone`, `Message::ToolUseSummary` stay.
- All existing `SystemMessage::*` variants stay.
- `LlmMessage` / vercel-ai aliases stay.

### 3.4 What gets deleted from `coco-tui`

- `pub struct ChatMessage { ... }`
- `pub enum MessageContent { 40 variants }`
- `pub fn add_message` and the 19 call sites
- `session.messages: Vec<ChatMessage>`

## 4. Wire Protocol Additions

### 4.1 Three event variants (all in `coco-types::event::ServerNotification`)

```rust
ServerNotification::MessageAppended {
    /// Shared with engine MessageHistory storage; cheap to broadcast.
    message: Arc<Message>,
}

ServerNotification::MessageTruncated {
    /// New length of MessageHistory.
    keep_count: usize,
}

ServerNotification::SessionResetForResume {
    session_id: SessionId,
}
```

Sizes: `MessageAppended` is one `Arc` clone (~8 bytes plus refcount bump);
`MessageTruncated` is 8 bytes; `SessionResetForResume` is one UUID.

No `HistoryReplaced { Arc<Vec<Message>> }`. Engine never broadcasts the
full history vector. Resume re-emits per-message via the same
`MessageAppended` path the live loop uses.

`SessionResetForResume` is the **only** broadcast that triggers a TUI
view rebuild. After receipt, TUI clears `TranscriptView`, then processes
the trailing burst of `MessageAppended`s as normal.

All three variants are added to the `wire_tagged_enum!` table in
`common/types/src/event.rs` — same pattern as existing variants, no
manual serde glue.

### 4.2 `UserCommand::Rewind` extended with `mode`

Today:

```rust
UserCommand::Rewind { message_id: Uuid, restore_type: RestoreType, rewound_turn: ... }
```

After:

```rust
pub enum RewindMode {
    /// User-initiated `/rewind` command. Modal flow, may show overlay,
    /// may restore files based on `restore_type`.
    Explicit,
    /// TUI auto-restore on user cancel before model produced output.
    /// Synchronous truncation; no file restoration; no modal overlay.
    AutoRestore,
}

UserCommand::Rewind {
    message_id: Uuid,
    restore_type: RestoreType,
    rewound_turn: ...,
    mode: RewindMode,
}
```

One command, two modes. No `RewindTo`. Engine handler reads `mode` to
gate file restoration and overlay emission. Both modes end with
`MessageTruncated` emission. SDK sees both.

### 4.3 No new push paths

All transcript-visible content already goes through `MessageHistory::push`.
This plan does not introduce new push call sites; it only ensures each
existing push emits exactly one `MessageAppended`. See §5.

## 5. Engine Changes (`coco-query` + `coco-messages`)

### 5.1 `MessageHistory::push` returns `Arc<Message>`

```rust
// coco-messages::history
impl MessageHistory {
    pub fn push(&mut self, msg: Message) -> Arc<Message> {
        let arc = Arc::new(msg);
        self.messages.push(arc.clone());
        self.index.insert(*arc.uuid().expect("transcript message has uuid"), self.messages.len() - 1);
        arc
    }
}
```

`messages` field type changes from `Vec<Message>` to `Vec<Arc<Message>>`.
The existing UUID index (`HashMap<Uuid, usize>`) stays. All read APIs
return `&Arc<Message>` or `Arc<Message>` clones (cheap).

### 5.2 `history_push_and_emit` helper in `coco-query`

```rust
// app/query/src/helpers.rs
pub async fn history_push_and_emit(
    history: &mut MessageHistory,
    msg: Message,
    event_tx: &Option<Sender<CoreEvent>>,
) -> Arc<Message> {
    let arc = history.push(msg);
    emit_protocol(event_tx, ServerNotification::MessageAppended {
        message: arc.clone(),
    }).await;
    arc
}
```

All ~21 `history.push(msg)` sites in `app/query/src/engine.rs` and
`engine_finalize_turn.rs` migrate to `history_push_and_emit`. Audited in
Phase 2.

### 5.3 Cancel paths collapse into one finalizer

Current: two cancel sites at `engine.rs:708` and `engine.rs:1787` each
compute `for_tool_use` from different premises and each push their own
interrupt message.

After: one helper that any cancel exit path calls. `for_tool_use` is
computed once at the helper entry from the engine's authoritative view
(tool-call state at the moment of cancellation):

```rust
// app/query/src/cancel.rs (new or merged into engine.rs)
pub async fn finalize_user_cancel(
    history: &mut MessageHistory,
    in_flight_tool_calls: bool,
    event_tx: &Option<Sender<CoreEvent>>,
) {
    if last_message_is_user_interruption(history) {
        return; // dedup
    }
    let msg = create_user_interruption_message(in_flight_tool_calls);
    // create_user_interruption_message returns
    //   Message::System(SystemMessage::UserInterruption { for_tool_use, uuid, timestamp })
    history_push_and_emit(history, msg, event_tx).await;
}
```

Both former cancel exits call this. TUI never recomputes `for_tool_use`.

### 5.4 Rewind handler reads `mode`

```rust
// app/query/src/rewind.rs (consolidate explicit + auto-restore paths)
pub async fn handle_rewind(
    history: &mut MessageHistory,
    target_uuid: Uuid,
    restore_type: RestoreType,
    mode: RewindMode,
    event_tx: &Option<Sender<CoreEvent>>,
) -> Result<()> {
    let idx = history.position_of(target_uuid)
        .ok_or(QueryError::UnknownMessageId)?;
    history.truncate(idx);
    emit_protocol(event_tx, ServerNotification::MessageTruncated {
        keep_count: idx,
    }).await;
    if matches!(mode, RewindMode::Explicit) && matches!(restore_type, RestoreType::FilesAndState) {
        restore_files_to_checkpoint(...).await?;
    }
    Ok(())
}
```

### 5.5 Resume seeding in `tui_runner`

```rust
// app/cli/src/tui_runner.rs
{
    let mut history = runtime.history.lock().await;
    *history = MessageHistory::from_messages(plan.prior_messages.clone());
}

event_tx.send(CoreEvent::Protocol(ServerNotification::SessionResetForResume {
    session_id: plan.session_id,
})).await?;

let history = runtime.history.lock().await;
for arc_msg in history.iter_arcs() {
    event_tx.send(CoreEvent::Protocol(ServerNotification::MessageAppended {
        message: arc_msg.clone(),
    })).await?;
}
```

No `seed_transcript_dedup` / `seed_tool_result_replacement_state` special
paths — the TUI receives the same events it would have received during a
live session.

## 6. TUI Changes (`coco-tui`)

### 6.1 `TranscriptView` — the derived cache

```rust
// app/tui/src/state/transcript_view.rs (new)

/// Derived view of MessageHistory. Authority remains with engine
/// MessageHistory; this struct is rebuilt incrementally from
/// MessageAppended / MessageTruncated events.
pub struct TranscriptView {
    cells: Vec<RenderedCell>,
    by_uuid: HashMap<Uuid, usize>,
    last_viewport_width: u16,
}

pub struct RenderedCell {
    pub message_uuid: Uuid,
    pub kind: CellKind,
    /// Per-viewport cached layout. Invalidated on resize.
    pub cached_lines: Option<Vec<Line<'static>>>,
    pub cached_height: Option<u16>,
}

/// TUI-internal classification used for render dispatch. Different from
/// SystemMessage sub-variants because several System sub-variants may
/// render the same way (e.g. all HookOutcome kinds share a renderer).
pub enum CellKind {
    UserText, UserImage, UserBashInput, UserAttachment,
    AssistantText, AssistantThinking, AssistantRedactedThinking,
    ToolUse, ToolResult, FileEditDiff, FileWriteResult,
    System(SystemCellKind),
}

pub enum SystemCellKind {
    UserInterruption { for_tool_use: bool },
    Informational { level: SystemMessageLevel },
    HookOutcome { kind: HookOutcomeKind },
    PlanEvent,
    CompactBoundary,
    ApiError,
    Coordinator,
    ExternalNotification,
    // ... mirrors but is not identical to SystemMessage sub-variants
}

impl TranscriptView {
    pub fn on_message_appended(&mut self, msg: &Message);
    pub fn on_message_truncated(&mut self, keep_count: usize);
    pub fn on_session_reset(&mut self);
    pub fn cells(&self) -> &[RenderedCell];
    pub fn invalidate_layout(&mut self, new_width: u16);
}
```

The function `message_to_cell(&Message) -> RenderedCell` lives in
`coco-tui/src/state/derive.rs`. It pattern-matches `Message` (including
`Message::System(SystemMessage::*)`) and produces a `RenderedCell` with
`CellKind`. **Pure function of `Message`** — no theme, no viewport, no
selection. Layout (`cached_lines` / `cached_height`) is computed lazily
at render time and stored back on the cell.

### 6.2 `SessionState` shrinks

```rust
// app/tui/src/state/session.rs
pub struct SessionState {
    // pub messages: Vec<ChatMessage>,  // DELETED
    pub transcript: TranscriptView,     // ADDED
    pub tool_executions: Vec<ToolExecution>, // KEPT — UI-only widget state
    // ... other fields unchanged
}
```

`ui.streaming: Option<StreamingState>` (in `state/ui.rs:79`) — unchanged.

### 6.3 Event handlers

In `app/tui/src/server_notification_handler/protocol.rs`:

```rust
ServerNotification::MessageAppended { message } => {
    state.session.transcript.on_message_appended(message);
    // Streaming overlay cleared if this message is the assistant turn
    // that just finalized:
    if message.is_assistant() && state.ui.streaming.as_ref()
        .is_some_and(|s| s.anchor_uuid == message.uuid()) {
        state.ui.streaming = None;
    }
    true
}

ServerNotification::MessageTruncated { keep_count } => {
    state.session.transcript.on_message_truncated(*keep_count);
    // Discard ephemeral state that anchors on dropped messages:
    state.session.tool_executions.retain(|t| t.message_uuid_before_truncate(...));
    state.ui.streaming = None;
    true
}

ServerNotification::SessionResetForResume { session_id } => {
    state.session.transcript.on_session_reset();
    state.session.tool_executions.clear();
    state.ui.streaming = None;
    state.session.conversation_id = Some(session_id.to_string());
    true
}
```

### 6.4 Render order

```
[transcript_view.cells() rendered top-to-bottom]
[ui.streaming overlay if Some]
[tool_executions rendered as widget panes if any Running/Queued]
[input bar]
[modal overlays / toasts on top]
```

Streaming overlay anchors to the assistant turn UUID it belongs to. When
the engine pushes the finalized `Message::Assistant`, the
`MessageAppended` handler atomically appends the cell and clears the
overlay — no flicker.

### 6.5 Auto-restore

```rust
// server_notification_handler/protocol.rs
fn apply_auto_restore(
    state: &mut AppState,
    cell_idx: usize,
    cmd_tx: &Sender<UserCommand>,
) {
    let cell = &state.session.transcript.cells()[cell_idx];
    let target_uuid = cell.message_uuid;
    let input_text = state.session.transcript.text_at(cell_idx);
    let perm_mode = state.session.transcript.permission_mode_at(cell_idx);

    state.ui.input.textarea.set_text(&input_text);
    if let Some(mode) = perm_mode {
        state.session.permission_mode = mode;
    }
    // Ask engine to truncate. Engine will emit MessageTruncated which
    // applies to transcript via the normal handler.
    let _ = cmd_tx.try_send(UserCommand::Rewind {
        message_id: target_uuid,
        restore_type: RestoreType::StateOnly,
        rewound_turn: None,
        mode: RewindMode::AutoRestore,
    });
}
```

No direct mutation of transcript. Truncation comes back via the event
path. Engine and TUI converge by construction; SDK sees the same event.

### 6.6 Old `add_message` sites — by-site disposition

| File:line | What it did | After plan |
|---|---|---|
| `stream.rs:55` queued tool start | Created `ChatMessage::ToolExecuting` row | Becomes a `ToolExecution` entry in `tool_executions`; not a transcript cell. When engine pushes the finalized `Message::ToolResult`, MessageAppended renders the row. |
| `stream.rs:91, 97` tool error / success | `ChatMessage::tool_error` / `tool_success` | Removed. Engine push of `Message::ToolResult` produces the cell via MessageAppended. |
| `protocol.rs:742` SlashCommandStatus | `system_text` row | Engine pushes `Message::System(SystemMessage::Informational { ... })`. |
| `protocol.rs:957` InterruptionMarker | TUI-side push, recomputed `for_tool_use` | **Deleted.** Engine push of `Message::System(SystemMessage::UserInterruption { for_tool_use })` covers it. |
| `protocol.rs:1002` teammate message | `teammate_message` row | Engine pushes `Message::System(SystemMessage::ExternalNotification { kind: Teammate, ... })`. |
| `tui_only.rs:258..413` 6 sites (memory/plan/editor opens) | `system_text` rows for slash command output | Per case: if the row should be in transcript & visible to SDK → engine push via a new `UserCommand::PushSystemMessage { kind, payload }`. If transient toast → `ui.toast` overlay (not transcript). |
| `update.rs:323` Toast → system | `system_text` row | If user-visible-only: `ui.toast`. If should be in transcript: engine push. |
| `update/edit.rs:55, 117` bash input/output, user message | `user_bash_input` / `user_text` | **Deleted.** `UserCommand::SubmitInput` flows through engine → engine pushes `Message::User` → MessageAppended. |
| `update/interaction.rs:325` | inline construction | Per case: `ui.toast` or engine push, decided in Phase 3 audit. |
| `projection.rs:15, 36` streaming flush | `thinking` / `assistant_text` rows | **Deleted.** Streaming progress lives in `ui.streaming`; when the assistant turn finalizes, engine pushes `Message::Assistant` → MessageAppended creates the cell and clears overlay. |

Net: 19 sites collapse to **zero**. Anything that should be persistent in
transcript becomes an engine push (via existing engine code or a new
`UserCommand::PushSystemMessage` for TUI-originated system rows).
Anything ephemeral lives in `ui.toast` / `ui.streaming` /
`tool_executions`.

### 6.7 New `UserCommand::PushSystemMessage`

For TUI-originated content that should appear in transcript (slash
command outputs that document a user action, plan-mode banners, etc.).
The engine receives this command, constructs the appropriate
`Message::System(...)`, pushes to history, emits `MessageAppended`. TUI
does **not** push directly to its own derived view — even for content it
originated.

```rust
UserCommand::PushSystemMessage {
    kind: SystemMessageOriginKind,
    /// Payload validated at the engine boundary against `kind`.
    payload: serde_json::Value,
}
```

This preserves I-1 (Authority): TUI never writes to transcript directly.

## 7. Critical Flows After

### 7.1 Cancel during streaming

```
User Ctrl+C
  → TUI sends UserCommand::Cancel
  → engine loop hits cancel checkpoint, computes in_flight_tool_calls=false
  → finalize_user_cancel(history, false, event_tx)
      → history.push(Message::System(SystemMessage::UserInterruption { for_tool_use: false, ... }))
      → emit MessageAppended { message: Arc<Message> }
  → TUI handler:
      transcript.on_message_appended(&msg) → adds dim cell with CellKind::System(UserInterruption { false })
      ui.streaming = None
```

### 7.2 Cancel during tool execution

Same path, `in_flight_tool_calls=true`. Cell `for_tool_use=true`.
**Computed once, stored on Message, read by TUI.** No race possible.

### 7.3 `/resume` shows scrollback

```
tui_runner loads plan.prior_messages → MessageHistory
  → emit SessionResetForResume → TUI clears TranscriptView, tool_executions, streaming
  → for each message in history:
      emit MessageAppended → TUI on_message_appended
```

TUI scrollback is fully populated with all prior content **including
hooks, plan markers, interrupt markers** because they are all
`Message::System(...)` variants now.

### 7.4 Auto-restore

```
User Ctrl+C with empty input on tail boundary
  → on_turn_interrupted decides auto-restore is applicable
  → apply_auto_restore: send UserCommand::Rewind { mode: AutoRestore }
      (TUI does NOT mutate transcript locally)
  → engine handle_rewind truncates history
  → emit MessageTruncated
  → TUI handler: transcript.on_message_truncated; clears overlays
  → SDK consumer also sees MessageTruncated → consistent
```

Engine is authoritative for truncation. TUI converges by reading the
event. SDK never desyncs.

## 8. Implementation Plan

No dual-write. CLAUDE.md: "No deprecated code. Delete outright." Each
phase compiles and passes quick-check.

### Phase 1 — `coco-messages` and `coco-types` extensions (additive)

- `MessageHistory::messages` becomes `Vec<Arc<Message>>`; `push` returns `Arc<Message>`. Update internal callers (small number).
- Add `SystemMessage::UserInterruption`, `HookOutcome`, `PlanEvent`, `Coordinator`, `ExternalNotification` (final set decided here; some may fold into `Informational`).
- Add `ServerNotification::MessageAppended`, `MessageTruncated`, `SessionResetForResume` in `wire_tagged_enum!` table.
- Add `RewindMode` enum; extend `UserCommand::Rewind` with `mode` field.
- Add `UserCommand::PushSystemMessage`.
- Update `crate-coco-messages.md` and `event-system-design.md` to reflect new variants. Update `audit-gaps.md` to close the `/resume` hydration gap once Phase 5 lands.
- Tests for variant round-trip serde.

### Phase 2 — Engine emits

- `history_push_and_emit` helper.
- Replace all `history.push(msg)` sites in `app/query/src/` with the helper.
- Collapse `engine.rs:708` and `engine.rs:1787` cancel sites into `finalize_user_cancel`.
- Add `handle_rewind` (replaces current explicit rewind handler) reading `mode`.
- Handle `UserCommand::PushSystemMessage` in session runtime.
- Unit + integration tests:
  - Each push site emits exactly one MessageAppended with consistent Arc.
  - Cancel emits one UserInterruption with engine-side for_tool_use, no second push.
  - Rewind { Explicit / AutoRestore } truncates and emits MessageTruncated.

### Phase 3 — TUI rewrite (single atomic landing)

This is the largest single commit. No dual-write phase.

- Add `coco-tui/src/state/transcript_view.rs` (TranscriptView, RenderedCell, CellKind, SystemCellKind).
- Add `coco-tui/src/state/derive.rs` (`message_to_cell`).
- Wire the three new ServerNotification handlers in `protocol.rs`.
- Delete `session.messages` field.
- Delete `ChatMessage` struct and `MessageContent` enum from `coco-tui/src/state/`.
- Delete every `add_message` call site (per §6.6 table).
- Render path: replace per-variant `render_user / render_assistant / render_tool / render_system` dispatch with `match &cell.kind`.
- Regenerate snapshot tests; visual review before accept.

### Phase 4 — `/resume` hydration

- `app/cli/src/tui_runner.rs` emits `SessionResetForResume` + `MessageAppended` per message.
- Remove `seed_transcript_dedup` if it was UI-side only (keep engine-side dedup state).
- Integration test: JSONL with interrupt marker resume → TUI cells include dim interruption row.

### Phase 5 — Auto-restore round-trip

- `apply_auto_restore` sends `UserCommand::Rewind { mode: AutoRestore }`.
- Integration test: cancel mid-turn → engine history truncated, transcript view truncated, SDK NDJSON observer sees MessageTruncated event.

### Phase 6 — Cleanup gate

- Verify no `chat_message` / `MessageContent` / `add_message` survives.
- Verify no `bridge.rs` exists in `coco-messages` (confirms layer hygiene).
- `just quick-check` and `just pre-commit` green.

## 9. Tests

Cross-layer end-to-end:

```rust
// app/query/tests/cancel_unified_marker.rs
#[tokio::test]
async fn cancel_emits_single_marker_consistent_across_engine_and_tui() {
    let (runtime, mut tui_state, event_pipe) = harness().await;
    runtime.submit("hello").await;
    runtime.cancel().await;
    drain(event_pipe, &mut tui_state).await;

    let last = runtime.history.lock().await.last().unwrap().clone();
    assert!(matches!(&*last,
        Message::System(SystemMessage::UserInterruption { for_tool_use: false, .. })));

    let last_cell = tui_state.session.transcript.cells().last().unwrap();
    assert!(matches!(last_cell.kind, CellKind::System(SystemCellKind::UserInterruption { for_tool_use: false })));
}
```

Resume:

```rust
#[tokio::test]
async fn resume_restores_scrollback_with_interrupt_marker() {
    let jsonl = build_jsonl_with_interrupt();
    let (runtime, mut tui_state, event_pipe) = resume_harness(&jsonl).await;
    drain(event_pipe, &mut tui_state).await;
    let cells = tui_state.session.transcript.cells();
    assert!(cells.iter().any(|c| matches!(c.kind,
        CellKind::System(SystemCellKind::UserInterruption { .. }))));
}
```

Auto-restore round-trip:

```rust
#[tokio::test]
async fn auto_restore_truncates_engine_and_tui_and_sdk() {
    let (runtime, mut tui_state, sdk_observer, event_pipe) = harness_with_sdk().await;
    runtime.submit("original prompt").await;
    runtime.cancel().await;
    drain(event_pipe, &mut tui_state).await;
    assert_eq!(tui_state.ui.input.text(), "original prompt");
    let history = runtime.history.lock().await;
    assert!(history.iter().all(|m| !is_user_text(m, "original prompt")));
    assert!(sdk_observer.observed_message_truncated());
}
```

Snapshot tests:

- All 30+ existing TUI snapshots regenerated to render via TranscriptView path.
- Specific new snapshots:
  - Dim `UserInterruption` cell, `for_tool_use=true` variant
  - Hook outcome cell
  - Streaming overlay above last finalized cell
  - Tool execution widget while transcript shows prior finalized result

Smoke matrix (manual):

| Scenario | Expected |
|---|---|
| Ctrl+C during streaming | dim "Interrupted · …" cell appears; no panel banner; streaming overlay clears atomically |
| Ctrl+C during FileEdit tool | tool result cell + dim marker with `for_tool_use=true`; next prompt accepted |
| Ctrl+C twice in row | single marker cell (dedup by predicate) |
| `/resume` after interrupted session | full prior scrollback visible including interrupt marker |
| Auto-restore (cancel, empty input, lossless tail) | input restored, transcript shrunk to before user prompt, engine history truncated, SDK saw MessageTruncated |
| `/clear` followed by Ctrl+C | no interrupt marker (history cleared by SystemPreempt) |
| Stream interrupt during tool execution | tool result cell + dim marker; no double marker |

## 10. Acceptance Criteria

This refactor is complete when:

1. ✅ `coco-tui` contains no `ChatMessage`, no `MessageContent`, no `add_message`.
2. ✅ `coco-messages` contains no `bridge.rs`, no `message_to_rows`, no `MessageRowKind`, no UI rendering state.
3. ✅ `session.messages: Vec<ChatMessage>` is replaced by `session.transcript: TranscriptView`.
4. ✅ Every `MessageHistory::push` in `app/query` is paired with `MessageAppended` emission.
5. ✅ Single `finalize_user_cancel` site computes `for_tool_use` once.
6. ✅ `UserCommand::Rewind` carries `mode: RewindMode`; auto-restore uses `AutoRestore` variant.
7. ✅ `/resume` populates TUI scrollback fully via the same handler path used for live turns.
8. ✅ SDK NDJSON observers receive `MessageTruncated` for both explicit and auto-restore rewinds.
9. ✅ All cross-layer + snapshot tests pass.
10. ✅ `just quick-check` and `just pre-commit` green.

## 11. Out of Scope

- Multi-session shared transcript (`parent_session_id` filtering) — designs cleanly on top of the unified protocol once needed.
- SDK transcript pagination — existing `session/read` API remains; this plan does not change it.
- AgentTeams coordinator merged-timeline view — separate concern; will consume the same event stream.
- Remote mirror behavior — TS pattern still applies; mirror is an outbound view of `MessageHistory`.
- Plan-mode UI re-skin — orthogonal.
- Markdown / diff rendering improvements — handled inside `message_to_cell` and `cached_lines` builder; no protocol surface change.

## 12. Why This Plan Differs from the Two Superseded Plans

The bridge plan (`engine-tui-message-bridge-plan.md`) formalized the
engine/TUI representation split with a new `MessageRowKind` enum in
`coco-messages` plus a 3-category transcript model (`FromMessage` /
`Synthetic` / `Ephemeral`) in `coco-tui`. The economic plan
(`engine-tui-transcript-economical-plan.md`) kept the split and
addressed only the most visible bugs.

This plan rejects the premise that engine and TUI need different
representations:

- **TS reference**: one `Message` type. `HookResultMessage` is a
  `Message`. `createUserInterruptionMessage` returns a `UserMessage`
  used identically for model API and UI render. `<InterruptedByUser />`
  is a stateless presenter that renders from the `Message` array.
- **codex-rs reference**: engine emits display-ready `ThreadItem`s; TUI
  cells are derived. Rewind is `ThreadRolledBack { num_turns: u32 }` —
  delta event, not full-history broadcast. All cells in one
  `Vec<Arc<dyn HistoryCell>>` — no `FromMessage` / `Synthetic` /
  `Ephemeral` split.
- **coco-rs reality**: `Message::System` already has 14 sub-variants;
  `MessageHistory` already has a UUID index; `StreamAccumulator`
  already emits `ItemCompleted { ThreadItem }`. The bridge plan invents
  parallel machinery instead of using what exists.

The unified plan picks the path consistent with both references: one
representation (`Message`), delta events (`MessageAppended` /
`MessageTruncated`), TUI as derived view, UI-only state segregated as
overlays.

## 13. References

- `crate-coco-messages.md` — canonical Message + SystemMessage types
- `crate-coco-tui.md` — canonical SessionState (to be revised per §6)
- `event-system-design.md` — three-layer CoreEvent (Protocol variant added)
- `streaming-metadata-roundtrip-plan.md` — `Arc<…>` event payload precedent
- `audit-gaps.md` — `/resume` TUI hydration gap (closes when Phase 4 lands)
- TS: `utils/messages.ts:545` `createUserInterruptionMessage`;
  `query.ts:1046-1502` cancel flow;
  `REPL.tsx:2629` `setMessages(prev => [...prev, msg])` (single source);
  `conversationRecovery.ts:144-247` resume deserialization;
  `useDeferredHookMessages.ts:28-43` hook messages appended to array
- codex-rs: `app-server-protocol/src/protocol/v2/item.rs:212` `ThreadItem` (16 variants, display-ready);
  `tui/src/history_cell/mod.rs:183` `HistoryCell` trait;
  `protocol/src/protocol.rs:1306` `ThreadRolledBack { num_turns: u32 }`;
  `tui/src/chatwidget/replay.rs:66` `handle_thread_item` dispatch
