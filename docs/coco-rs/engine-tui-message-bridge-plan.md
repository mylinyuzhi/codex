# Engine ↔ TUI Message Bridge Refactor Plan

Recommended path: use
[`engine-tui-transcript-economical-plan.md`](engine-tui-transcript-economical-plan.md)
first. This larger bridge refactor remains a future option, but the immediate
resume/cancel/auto-restore bugs should be handled by the economic plan.

Date: 2026-05-18

Owner: this doc owns the **architectural plan** for unifying engine
`MessageHistory` and TUI `session.messages` via a typed bridge. It does NOT
redefine the canonical types — `crate-coco-messages.md` owns `Message`,
`crate-coco-tui.md` owns `ChatMessage`/`SessionState`, and
`event-system-design.md` owns `CoreEvent` and its three layers.

Cross-references:
- `crate-coco-tui.md` — current TUI state model
- `crate-coco-messages.md` — engine message types
- `event-system-design.md` — three-layer CoreEvent
- `ui/codex-rs-tui-comparison.md` — already calls for "typed transcript cells
  derived from `SessionState.messages`" (target direction)
- `streaming-metadata-roundtrip-plan.md` — companion plan for the
  engine-internal `AssistantTurnSnapshot` flow
- `audit-gaps.md` — `/resume` TUI hydration is a long-standing P2 gap

## 0. TL;DR

| Layer | Today | After plan |
|---|---|---|
| Engine | `MessageHistory: Vec<Message>` (`coco-messages`) | unchanged |
| TUI | `session.messages: Vec<ChatMessage>` derived ad-hoc from per-event handlers | `session.transcript: Vec<TranscriptRow>` with **three categorized origins** |
| Cancel marker | engine pushes literal `[Request interrupted by user]` user msg; TUI **independently** appends `InterruptionMarker` ChatMessage | engine pushes; TUI mirrors via single adapter — **one source of truth** |
| `/resume` | engine seeds `runtime.history`; TUI shows **empty chat** | replays engine messages as synthetic `HistoryAppended` events; TUI rebuilds transcript via same adapter |
| Auto-restore | truncates TUI display only; engine history retains everything | round-trips through `UserCommand::RewindTo` so engine truncates authoritatively; emits `HistoryReplaced` |

## 1. Refactor Background

### 1.1 Trigger — the cancel-marker fix exposed structural debt

A previous fix added a `[Request interrupted by user]` UI marker on Ctrl+C
(mirrors TS `createUserInterruptionMessage` + `<InterruptedByUser />`).
Implementation pushed the marker in **two independent places**:

| Site | What gets pushed |
|---|---|
| `app/query/src/engine.rs:1789` | `Message::User { text: INTERRUPT_MESSAGE_FOR_TOOL_USE }` into `MessageHistory` (model-facing) |
| `app/tui/src/server_notification_handler/protocol.rs:944` | `ChatMessage::interruption_marker { for_tool_use }` into `session.messages` (UI-facing) |

Both compute `for_tool_use` from **different sources**:
- Engine: `!discarded_early.is_empty()` from `StreamingHandle::discard()`
- TUI: `tool_executions.iter().any(t.status == Running\|Queued)`

These can disagree (race: `ToolUseStarted` event delivered after cancel-token
fires; or `pending_serial` populated but no `ToolUseStarted` emitted yet).
**Symptom-class bug**: engine sends `_FOR_TOOL_USE` to model, TUI shows
non-tool-use marker, user/model context diverges silently.

### 1.2 Bridge is ad-hoc — same smell, broader surface

The cancel marker is one of **~26 synthetic message types** that get
constructed independently on engine and TUI sides. Today's TUI ingestion:

- 20 distinct `state.session.add_message(...)` call sites
  (`stream.rs:55,91,97`, `protocol.rs:742,957,1002`, `tui_only.rs:258,…,413`,
  `update/edit.rs:55,117`, `update/interaction.rs:325`, `update.rs:323`,
  `projection.rs:15,36`).
- Each handler **manually composes** a `ChatMessage` from event payloads.
- No central conversion. Adding a new message type means: ① extend
  `Message`/`AttachmentKind` engine-side, ② emit a `CoreEvent` for it,
  ③ write a per-event handler in TUI, ④ define the `MessageContent` variant,
  ⑤ thread it through ~5 match arms.

This violates "single source of truth" at the boundary the user can detect.

### 1.3 Pre-existing gaps amplified

Two long-standing issues surfaced during the cancel-marker work:

1. **`/resume` shows empty chat scrollback.** `tui_runner.rs:325-343` seeds
   `runtime.history = plan.prior_messages` but does **nothing** for
   `app.state_mut().session.messages`. The TUI subscribes to `CoreEvent`
   only — no replay path exists. Confirmed by grep: zero call sites of
   anything resembling `for msg in prior_messages: emit(...)`.

2. **Auto-restore is TUI-only.** `protocol.rs:971` `apply_auto_restore`
   truncates `state.session.messages` but documents (line 968) that it
   does NOT round-trip through the backend. Engine `MessageHistory`
   retains the truncated content + the new interrupt marker. Next API
   call sends content the user thinks was rewound.

Both pre-existed the cancel fix. The cancel marker makes (1) more visible
(marker in engine history is invisible in resumed TUI) and (2) more
incorrect (extra marker in engine history that TUI auto-restore can't
remove).

## 2. Technical Decisions

### 2.1 The split is real and stays

`MessageHistory` (engine) and `session.messages` (TUI) cannot be collapsed
into one Vec because coco-rs supports modes where one side does not exist:

| Mode | Engine | TUI |
|---|---|---|
| Interactive TUI | ✓ | ✓ |
| `coco --print` (non-interactive) | ✓ | ✗ |
| SDK NDJSON server | ✓ | ✗ |
| Daemon background session | ✓ | ✗ |
| `coco ps`-attached read-only | per-session engines, distinct PIDs | one per session, isolated |

TS sidesteps this with React's single in-process state. coco-rs is multi-
process/multi-mode and **must** keep an explicit boundary. Re-litigating
this is wasted effort.

### 2.2 The real bad smell is the bridge, not the split

The smells the user can observe:
- Two writers per logical event.
- Two formats for the same data (multi-variant `Message` enum vs flat
  38-variant `MessageContent` enum).
- No canonical converter — every event handler ad-hoc-builds `ChatMessage`.
- `/resume` falls through a hole because nobody wrote the replay adapter.

All of these are bridge problems, not layer problems. The fix is to
**formalize the bridge contract**, not to merge the layers.

### 2.3 Three-category transcript model

TUI rows fall into three distinct origin classes. Today this is implicit
and mixed inside one flat enum:

| Origin class | Count today | What it is | Who creates it |
|---|---|---|---|
| **FromMessage** | ~12 variants | 1:1 derived from `coco_messages::Message` (User text, Assistant text/thinking/tool_use, ToolResult success/error/rejected/cancelled, InterruptionMarker, …) | Engine pushes Message; adapter converts |
| **Synthetic** | ~22 variants | UI-originated events without a model-facing Message backing (PlanMarker, Hook*, RateLimit, Shutdown, CompactBoundary, Advisor, TaskAssignment, ChannelMessage, ResourceUpdate, Attachment, ApiError, …) | TUI directly from `CoreEvent::Protocol`/`Tui` handlers |
| **Ephemeral** | 0 variants (kept separately in `ui.streaming` + `tool_executions`) | Live during a turn, replaced when finalized (StreamingText, ToolExecuting, partial thinking) | TUI from `CoreEvent::Stream` |

Making the three origins **explicit in the type system** is the cleanup.

### 2.4 What the adapter is and is not

**Is:** A pure function `message_to_rows(&Message) -> Vec<MessageRowKind>`,
defined in `coco-messages` (the natural owner — only it knows the message
type taxonomy). Returns `Vec` because one `Message::Assistant` can contain
text + thinking + tool_use blocks, each becoming a row.

**Is not:**
- Not a transformation that runs on `Synthetic` rows (they have no
  `Message` source).
- Not a transformation that runs on `Ephemeral` rows (streaming text
  hasn't been finalized into a `Message` yet — by definition mid-stream).
- Not bidirectional. The adapter is one-way; engine is authoritative for
  `FromMessage` content, TUI only mirrors.

### 2.5 The event protocol delta

Two **new** `CoreEvent::Protocol(ServerNotification::*)` variants, defined
in `coco-types`:

```rust
// coco-types::event
ServerNotification::HistoryAppended { message: Arc<Message> }
ServerNotification::HistoryReplaced { messages: Arc<Vec<Message>> }
```

`Arc` (not `Clone`) because `Message` carries `LlmMessage` and content
parts — KB-scale per message. Workspace already uses `Arc<AssistantTurnSnapshot>`
on `StreamEvent::Finish` (`streaming-metadata-roundtrip-plan.md` v6) so
the pattern is precedent.

One **new** `UserCommand` variant in the TUI→engine direction:

```rust
// coco-types::client_request
UserCommand::RewindTo { message_uuid: Uuid }
```

These are **purely additive** — no existing event semantics change.

## 3. Architectural Abstraction & Extensibility

### 3.1 Layered ownership

```
┌─────────────────────────────────────────────────────────────────────┐
│ coco-messages                                                       │
│   pub enum Message { ... }                       ← unchanged        │
│   pub fn message_to_rows(&Message)               ← NEW              │
│       -> Vec<MessageRowKind>                                        │
│   pub enum MessageRowKind { ... 12 variants ... } ← NEW             │
├─────────────────────────────────────────────────────────────────────┤
│ coco-types::event                                                    │
│   ServerNotification::HistoryAppended { Arc<Message> } ← NEW         │
│   ServerNotification::HistoryReplaced { Arc<Vec<Message>> } ← NEW    │
│   ClientRequest::RewindTo { Uuid }                       ← NEW       │
├─────────────────────────────────────────────────────────────────────┤
│ coco-query                                                          │
│   - cancel_finalize → push Message, emit HistoryAppended  ← UPDATED  │
│   - RewindTo handler → truncate, emit HistoryReplaced     ← NEW      │
├─────────────────────────────────────────────────────────────────────┤
│ coco-tui                                                            │
│   pub enum TranscriptRow { FromMessage / Synthetic / Ephemeral } ← NEW │
│   pub enum MessageRowKind (re-export from coco-messages)              │
│   pub enum SyntheticRow { ... 22 variants ... }   ← split from old    │
│   pub enum EphemeralRow { ... 3 variants ... }    ← formalize streaming │
│                                                                       │
│   on HistoryAppended  → append FromMessage         ← single adapter  │
│   on HistoryReplaced  → rebuild transcript          ← single adapter  │
│   on hook/system/MCP  → append Synthetic            ← per-event       │
│   on TextDelta/etc    → upsert Ephemeral            ← per-event       │
└─────────────────────────────────────────────────────────────────────┘
```

### 3.2 Abstraction benefits — what the model buys us

1. **Single bridge contract.** "Engine state changes flow to TUI through
   `HistoryAppended`/`HistoryReplaced`. TUI synthesizes its own rows for
   events with no Message backing." One sentence describes the rule;
   anyone reviewing a new event knows where it goes.

2. **Resume becomes trivial.** Hydration is one loop:
   ```rust
   for msg in &plan.prior_messages {
       event_tx.send(CoreEvent::Protocol(
           ServerNotification::HistoryAppended { message: Arc::new(msg.clone()) }
       )).await?;
   }
   ```
   No new TUI logic — the same handler that processes live messages
   processes resumed ones.

3. **Auto-restore is round-trip-correct.** `apply_auto_restore` sends
   `UserCommand::RewindTo { uuid }`. Engine truncates authoritatively.
   Engine emits `HistoryReplaced` with the new tail. TUI rebuilds
   transcript. **Engine and TUI converge by construction**, not by hope.

4. **Per-category invariants enforced by the type system.**
   - `FromMessage` rows carry `message_uuid: Uuid` — guarantees the row
     has an engine-side anchor. Render code can rely on that for transcript
     reflow.
   - `Synthetic` rows never carry a UUID — guarantees they don't pretend
     to be model context.
   - `Ephemeral` rows carry `anchor_uuid: Uuid` (the UUID of the finalized
     `Message` they'll be replaced by). Promotion is deterministic.

5. **Adding new message types is one place.** Future `Message::ComputerUse`
   or `Message::Voice` only needs:
   ① variant in `coco-messages`, ② match arm in `message_to_rows`,
   ③ render arm for the new `MessageRowKind`. The 20 ad-hoc TUI handlers
   never need to know about new variants.

### 3.3 Extension points

| Future feature | Where it plugs in |
|---|---|
| Multi-agent shared transcript view | Engine emits `HistoryAppended` with `parent_session_id`; TUI filters by session |
| Transcript persistence on TUI side (e.g. for crash recovery) | `Vec<TranscriptRow>` is `Serialize`; dump+reload via the same adapter |
| Side-by-side resume diff | Two `Vec<TranscriptRow>` from two `plan.prior_messages` — pure data |
| Stream throughput debug overlay | `EphemeralRow` carries delta count; render path can show it |
| SDK mode adding a TUI later | SDK's `Vec<Message>` feeds same adapter to materialize a TUI |

## 4. Is This Over-Designed?

### 4.1 Three honest comparisons

| Option | What it touches | Solves cancel bug? | Solves resume? | Solves auto-restore? | TLOC delta |
|---|---|---|---|---|---|
| **A. Minimal patch** — special-case `[Request interrupted by user]` in TUI ingestion, treat as InterruptionMarker | TUI ingestion (~30 LOC) | yes (after engine emits via #1 below) | no | no | +30 |
| **B. Adapter + event, keep flat enum** — add `message_to_rows`, `HistoryAppended`, route cancel marker through it, leave `ChatMessage`/`MessageContent` flat | coco-messages, coco-types/event, app/query cancel paths, 1-2 TUI handlers | yes | partial (cancel only) | no | +150 |
| **C. Full 3-category model** — split `MessageContent` into `MessageRowKind`/`SyntheticRow`/`EphemeralRow`, migrate all 20 add_message sites, implement HistoryAppended/HistoryReplaced, RewindTo, resume hydration | coco-messages, coco-types/event, coco-query, all of coco-tui's session model, all 20 add_message sites, render path, tests | yes | yes (fully) | yes | +800 net, ~2000 LOC churn |

### 4.2 The honest answer

**For just the cancel marker, Option C is over-designed.** Option A
suffices.

**For the broader gaps (resume scrollback empty, auto-restore one-way),
Option C is the minimum.** Option B sneaks in 50% of the complexity
without earning the resume/auto-restore fixes.

**The choice depends on whether resume scrollback and auto-restore
correctness are in scope right now.** If yes → C. If no → A, and revisit
when those become P1.

### 4.3 Why C is not over-engineered when in scope

The three-tier split is not invented complexity — **it is making implicit
categorization explicit**. The current flat `MessageContent` already has
the three classes (FromMessage / Synthetic / Ephemeral) mixed into one
enum. The classes are real; the type system just doesn't enforce them.
Splitting respects existing data structure.

Specifically:
- 12 of 38 variants are already "derived from a `Message`" today — anyone
  resuming a session needs them populated from disk, which is the resume
  bug. The adapter does this exactly once.
- 22 of 38 variants are already "UI-originated events with no Message" —
  changes nothing about their flow; they just move to a sibling enum.
- 3 ephemeral kinds already live OUTSIDE `MessageContent` (in
  `ui.streaming`, `tool_executions`) — the new `EphemeralRow` brings them
  into the same shape so the render path treats them uniformly.

**No new abstraction is invented; existing abstractions are named.**

### 4.4 What would be over-engineered

- A `trait Renderable` with `Box<dyn>` for each row — pays heap+vtable
  cost on the hot render path; nothing requires open extension.
- A separate `ChatMessage` *trait* on top of the enum — flat enum is the
  right call for tui because all renderers want exhaustive match.
- Bidirectional adapter (`ChatMessage → Message`) — TUI never originates
  authoritative message content; one direction is the rule.
- A workflow engine for transcript mutations — direct event handling is
  fine; rows are append-only except for `HistoryReplaced`.

These are tempting Rust-idiomatic generalizations but earn no concrete
benefit. The plan stays plain enums + functions.

## 5. Engine Layer Changes

Following docs own canonical types; this section lists deltas only.

### 5.1 `coco-messages`

`creation.rs` and a new `bridge.rs`:

```rust
// coco-messages::bridge (new file)

/// Pure adapter. One `Message::Assistant` with text+thinking+tool_use blocks
/// produces multiple `MessageRowKind`. Returns empty Vec for messages that
/// should not appear in the transcript (e.g. some Progress kinds).
pub fn message_to_rows(msg: &Message) -> Vec<MessageRowKind>;

/// Predicate used by engine cancel dedup AND by TUI synthetic-tail check.
pub fn is_user_interruption_marker(msg: &Message) -> bool;
```

`MessageRowKind` enum lives in `coco-messages` (so the adapter's return
type doesn't force a `coco-tui` dep on `coco-messages`-internals). About 12
variants — see §5.4.

### 5.2 `coco-types` event protocol

`event.rs`:

```rust
ServerNotification::HistoryAppended { message: Arc<Message> }
ServerNotification::HistoryReplaced {
    messages: Arc<Vec<Message>>,
    /// Reason carried so TUI handlers can decide whether to reset
    /// ephemeral state (cancel: keep streaming; rewind: clear).
    reason: HistoryReplacedReason,
}

pub enum HistoryReplacedReason { Rewind, Compact, Clear, Resume }
```

`client_request.rs`:

```rust
ClientRequest::RewindTo { message_uuid: Uuid }
```

Both added to the `wire_tagged_enum!` macro table — no manual serde glue.

### 5.3 `coco-query` (engine.rs)

Three change clusters:

**(a) cancel_finalize** — already implemented in current refactor, now
also emits the event:

```rust
// engine.rs (top-of-loop cancel exit at line ~708)
if self.cancel.is_cancelled() {
    if !last_message_is_interrupt_marker(history) {
        let msg = coco_messages::create_user_interruption_message(false);
        let arc = Arc::new(msg.clone());
        history.push(msg);
        emit_protocol(&event_tx, ServerNotification::HistoryAppended {
            message: arc,
        }).await;
    }
    return Ok(make_query_result(...));
}
```

`last_message_is_interrupt_marker` is replaced by the canonical
`coco_messages::is_user_interruption_marker`. Same change at the
mid-stream cancel block (line ~1781).

**(b) RewindTo handler** — new file `app/query/src/rewind.rs`:

```rust
pub async fn handle_rewind_to(
    history: &mut MessageHistory,
    target_uuid: Uuid,
    event_tx: &mpsc::Sender<CoreEvent>,
) -> Result<(), QueryError> {
    let idx = history.messages.iter()
        .position(|m| m.uuid() == Some(&target_uuid))
        .ok_or(QueryError::UnknownMessageId)?;
    history.messages.truncate(idx);
    emit_protocol(&event_tx, ServerNotification::HistoryReplaced {
        messages: Arc::new(history.messages.clone()),
        reason: HistoryReplacedReason::Rewind,
    }).await;
    Ok(())
}
```

Wired into `SessionRuntime::handle_user_command` next to existing
`UserCommand::Rewind` (which keeps explicit-modal-state semantics; this
is the new synchronous path for auto-restore).

**(c) Engine push points that need to emit** — finalized assistant
message push (engine.rs:1976+), tool-result push from completed tools
(via `outcome.ordered_messages`), and the cancel synthesis path. Each
adds one `emit_protocol(HistoryAppended)` after the existing
`history.push`. **No new push points; only emissions on existing pushes.**

Engine push sites that need an emit are bounded (audit shows ~8 call
sites in engine.rs + a handful in engine_finalize_turn.rs). Each pairing
is mechanical; one wrapper helper (`history_push_and_emit`) consolidates
it.

### 5.4 New `MessageRowKind` variants

```rust
// coco-messages::bridge
pub enum MessageRowKind {
    UserText { text: String, permission_mode: Option<PermissionMode> },
    UserImage { path: String },
    AssistantText { markdown: String, model: String },
    AssistantThinking { content: String, duration_ms: Option<i64>,
                        reasoning_tokens: Option<i64> },
    AssistantRedactedThinking,
    ToolUse { call_id: String, tool_name: String, input: Value },
    ToolResultSuccess { call_id: String, output: String },
    ToolResultError { call_id: String, error: String },
    ToolResultRejected { call_id: String, reason: String },
    ToolResultCancelled { call_id: String },
    FileEditDiff { path: String, diff: String,
                   old_content: Option<String>, new_content: Option<String> },
    FileWriteResult { path: String, bytes_written: i64 },
    InterruptionMarker { for_tool_use: bool },
}
```

12 variants. Every variant is a faithful projection of a single
`Message`'s content block. **No UI-only metadata** (`is_meta`,
`is_compact_summary`, etc.) lives here — that goes to the surrounding
`TranscriptRow::FromMessage { ... }` wrapper.

## 6. UI Layer Changes

### 6.1 New `TranscriptRow` enum (coco-tui)

```rust
// coco-tui/state/transcript.rs (new file)

pub struct TranscriptRow {
    pub id: TranscriptRowId,
    pub created_at_ms: i64,
    pub origin: RowOrigin,
}

pub enum RowOrigin {
    FromMessage {
        message_uuid: Uuid,
        kind: MessageRowKind,         // re-export from coco-messages
        is_meta: bool,
        is_compact_summary: bool,
        is_visible_in_transcript_only: bool,
        permission_mode: Option<PermissionMode>,
    },
    Synthetic(SyntheticRow),
    Ephemeral {
        /// UUID of the future Message that will replace this row.
        anchor_uuid: Uuid,
        kind: EphemeralRow,
    },
}

pub enum TranscriptRowId {
    /// Stable for the lifetime of the entry (Message UUID).
    Message(Uuid),
    /// Stable for the lifetime of the synthetic event (sequence number).
    Synthetic(u64),
    /// Stable for the lifetime of the in-flight item (Message UUID +
    /// content-block index).
    Ephemeral(Uuid, u16),
}

pub enum SyntheticRow {
    BashInput { command: String },
    BashOutput { output: String, exit_code: i32 },
    PlanMarker { action: PlanAction },
    AgentNotification { agent_id: String, summary: String },
    TeammateMessage { teammate: String, content: String },
    ChannelMessage { source: String, user: Option<String>, content: String },
    ResourceUpdate { kind: ResourceUpdateKind, server: String,
                     target: String, reason: Option<String> },
    Attachment { attachment_type: String, preview: String },
    ApiError { error: String, retryable: bool, status_code: Option<i32> },
    RateLimit { message: String, resets_at: Option<i64> },
    Shutdown { reason: String },
    ShutdownRequest { from: String, reason: Option<String> },
    ShutdownRejected { from: String, reason: String },
    HookSuccess { hook_name: String, output: String },
    HookNonBlockingError { hook_name: String, error: String },
    HookBlockingError { hook_name: String, error: String, command: String },
    HookCancelled { hook_name: String },
    HookSystemMessage { hook_name: String, message: String },
    HookAdditionalContext { hook_name: String, context: String },
    HookStoppedContinuation { hook_name: String, reason: String },
    HookAsyncResponse { hook_name: String, output: String },
    PlanApproval { plan: String, request_id: String },
    CompactBoundary,
    CompactSummary { summary: String,
                     messages_summarized: Option<i32>,
                     user_context: Option<String>,
                     trigger: CompactTrigger },
    Advisor { advisor_id: String, content: String },
    TaskAssignment { task_id: String, assignee: String, description: String },
    SystemText(String),  // currently the catch-all; kept for migration
}

pub enum EphemeralRow {
    StreamingText { partial: String },
    StreamingThinking { partial: String,
                        reasoning_tokens: Option<i64> },
    ToolExecuting { tool_name: String, input: Value,
                    started_at: Instant, call_id: String },
}
```

The 38-variant `MessageContent` decomposes into 12 + 27 + 3 = **42
variants total** across three enums. Total complexity is similar; the
**boundary** is what changes.

### 6.2 `session.messages` becomes `session.transcript`

```rust
// SessionState
pub struct SessionState {
    pub transcript: Vec<TranscriptRow>,
    // ... all other fields unchanged
}
```

`tool_executions: Vec<ToolExecution>` and `ui.streaming: Option<StreamingState>`
**stay** for the per-tool/per-stream state-machine logic that drives
permission prompts and stall detection. The `EphemeralRow` projection is
the view layer over them, mutated by the same handlers that update those
state machines.

### 6.3 Event-handler migration

The 20 `add_message` call sites split as follows:

| File | Current variant | Becomes |
|---|---|---|
| `protocol.rs:742` SlashCommandStatus | `system_text` | `Synthetic::SystemText` (preserve) |
| `protocol.rs:957` InterruptionMarker | `interruption_marker` | **REMOVED** — engine push triggers HistoryAppended |
| `protocol.rs:1002` teammate_message | `teammate_message` | `Synthetic::TeammateMessage` |
| `projection.rs:15,36` streaming flush | `thinking`/`assistant_text` | **REMOVED** — engine push of finalized `Message::Assistant` triggers HistoryAppended; Ephemeral rows pre-existed |
| `stream.rs:55` queued tool start | inline ChatMessage construction | **REMOVED** — replaced by `Ephemeral::ToolExecuting` (lifecycle handled by tool execution state machine) |
| `stream.rs:91,97` tool error/success | `tool_error`/`tool_success` | **REMOVED** — engine push of `Message::ToolResult` triggers HistoryAppended |
| `tui_only.rs:258..413` memory/plan/editor opens | `system_text` | `Synthetic::SystemText` (preserve, with structured fields) |
| `update.rs:323` Toast→system | `system_text` | `Synthetic::SystemText` |
| `update/edit.rs:55,117` bash input/output, user message | `user_bash_input` etc. | **REMOVED** — `UserCommand::SubmitInput` flows through engine, engine push triggers HistoryAppended |
| `update/interaction.rs:325` | inline | `Synthetic::SystemText` |

**Net effect**: ~10 of 20 sites disappear (replaced by single
`HistoryAppended` handler). The remaining ~10 are synthetic-only events
that retain direct ingestion — but they call into the typed
`SyntheticRow` constructor, not raw struct literals.

### 6.4 Render path

`widgets/chat/render_*.rs`:

```rust
match &row.origin {
    RowOrigin::FromMessage { kind, .. } => render_message_row(kind, ...),
    RowOrigin::Synthetic(s) => render_synthetic_row(s, ...),
    RowOrigin::Ephemeral { kind, .. } => render_ephemeral_row(kind, ...),
}
```

`render_message_row` is the only routine that needs to handle MessageRowKind.
The previously-split render_user/render_assistant/render_tool/render_system
fold into one function per **row category**, not per **role**. Cleaner.

### 6.5 `/resume` hydration

```rust
// app/cli/src/tui_runner.rs (after the existing prior-messages seed)
for msg in &plan.prior_messages {
    let _ = notification_tx.send(CoreEvent::Protocol(
        ServerNotification::HistoryAppended { message: Arc::new(msg.clone()) }
    )).await;
}
```

The TUI's single `HistoryAppended` handler — already exercised by live
turns — replays the messages into `session.transcript`. Resume now shows
prior chat scrollback. **The resume bug closes via the bridge itself**;
no extra code in TUI.

### 6.6 Auto-restore round-trip

```rust
// server_notification_handler/protocol.rs::apply_auto_restore
fn apply_auto_restore(state: &mut AppState, idx: usize,
                      command_tx: &mpsc::Sender<UserCommand>) {
    let target = match &state.session.transcript[idx].id {
        TranscriptRowId::Message(uuid) => *uuid,
        _ => return, // shouldn't happen — selectable rows are FromMessage
    };
    // Take input from the target user message
    let input_text = state.session.transcript[idx].text_content().to_string();
    let perm = state.session.transcript[idx].permission_mode();
    if !input_text.is_empty() { state.ui.input.textarea.set_text(&input_text); }
    if let Some(mode) = perm { state.session.permission_mode = mode; }
    state.session.conversation_id = Some(uuid::Uuid::new_v4().to_string());
    // ... clear paste, scroll, suggestions
    // Ask engine to truncate
    let _ = command_tx.try_send(UserCommand::RewindTo { message_uuid: target });
}
```

`HistoryReplaced` then arrives at the TUI handler:

```rust
ServerNotification::HistoryReplaced { messages, reason } => {
    state.session.transcript.retain(|r| !r.is_from_message()); // drop FromMessage
    for msg in messages.iter() {
        // append FromMessage rows via adapter
    }
    // Synthetic rows preserved across rewind (they're UI history, not model history)
    // Ephemeral cleared (lifecycle change)
    if matches!(reason, HistoryReplacedReason::Rewind) {
        state.session.transcript.retain(|r| !r.is_ephemeral());
    }
    true
}
```

Engine is now authoritative. TUI converges by reading.

## 7. Implementation Plan

Six phases. Each phase compiles and passes existing tests; only phase 6
adds new tests.

### Phase 1 — Foundations (additive, no behavior change)

Goal: introduce types and adapter without using them yet.

| Step | Files | Outcome |
|---|---|---|
| 1.1 Add `MessageRowKind` + `message_to_rows` + `is_user_interruption_marker` | `core/messages/src/bridge.rs` (new) | adapter exists, unit-tested |
| 1.2 Re-export from coco-messages | `core/messages/src/lib.rs` | downstream visible |
| 1.3 Add `ServerNotification::HistoryAppended` / `HistoryReplaced` / `HistoryReplacedReason` | `common/types/src/event.rs` (extend `wire_tagged_enum!`) | events declared |
| 1.4 Add `ClientRequest::RewindTo` | `common/types/src/client_request.rs` | command declared |
| 1.5 No emit, no handle | — | quick-check stays green |

### Phase 2 — Engine emits

Goal: engine emits the new events on every history mutation; no consumer
yet.

| Step | Files | Outcome |
|---|---|---|
| 2.1 `history_push_and_emit` helper | `app/query/src/helpers.rs` | single push+emit site |
| 2.2 Replace `history.push(msg)` at the 8 engine call sites | `engine.rs`, `engine_finalize_turn.rs` | each push now emits |
| 2.3 cancel path uses canonical `is_user_interruption_marker` | `engine.rs:708,1789` | dedup centralized |
| 2.4 Add `handle_rewind_to` and wire it to `UserCommand::RewindTo` | `app/query/src/rewind.rs`, `app/cli/src/session_runtime.rs` | engine truncation works |

### Phase 3 — TUI types

Goal: introduce `TranscriptRow`/`RowOrigin`/`SyntheticRow`/`EphemeralRow`
in parallel with the existing `ChatMessage`/`MessageContent`. Existing
code unchanged. Tests build against both.

| Step | Files | Outcome |
|---|---|---|
| 3.1 Add `TranscriptRow` types | `app/tui/src/state/transcript.rs` (new) | types defined |
| 3.2 Add `session.transcript: Vec<TranscriptRow>` alongside `session.messages` | `app/tui/src/state/session.rs` | dual state during migration |
| 3.3 `chat_message_to_row` shim | `app/tui/src/state/transcript_shim.rs` (new) | bridges `ChatMessage` → `TranscriptRow` during migration |
| 3.4 Each `add_message` site also calls `add_transcript_row` (parallel write) | 20 call sites | dual write |

### Phase 4 — TUI consumers

Goal: read paths switch from `session.messages` to `session.transcript`.
`session.messages` is reduced to a write-only shadow.

| Step | Files | Outcome |
|---|---|---|
| 4.1 Render path: dispatch on `RowOrigin` | `widgets/chat/{mod,render_user,...}.rs` | renderer reads `transcript` |
| 4.2 Transcript modal | `widgets/transcript_modal.rs` | reads `transcript` |
| 4.3 `messages_after_are_only_synthetic` and friends use `TranscriptRow` | `update_rewind.rs` | typed predicates |
| 4.4 Snapshot tests updated | `widgets/snapshots/*` | new snapshots |

### Phase 5 — HistoryAppended consumer

Goal: TUI ingests engine `HistoryAppended` events.

| Step | Files | Outcome |
|---|---|---|
| 5.1 Handler for `HistoryAppended` | `server_notification_handler/protocol.rs` | adapter integrated |
| 5.2 Remove the 10 redundant `add_message` sites that now flow through HistoryAppended | `protocol.rs`, `stream.rs`, `projection.rs`, `update/edit.rs` | dedup |
| 5.3 Handler for `HistoryReplaced` | same | engine-truncate visible |
| 5.4 `apply_auto_restore` switches to `UserCommand::RewindTo` | `protocol.rs` | round-trip |
| 5.5 `/resume` replay | `app/cli/src/tui_runner.rs` | scrollback restored |

### Phase 6 — Cleanup & validation

| Step | Files | Outcome |
|---|---|---|
| 6.1 Delete `session.messages` field + `MessageContent` enum + `ChatMessage` constructors | `app/tui/src/state/session.rs` (-~300 LOC) | single source |
| 6.2 Delete `chat_message_to_row` shim | — | migration scaffolding gone |
| 6.3 Cross-layer integration test: cancel → assert both engine history tail and TUI transcript tail show the marker with consistent `for_tool_use` | `app/query/tests/cancel_synthesis.rs` | regression-guards Critical 1 |
| 6.4 Resume integration test: load JSONL with prior interrupt → assert TUI transcript renders dim marker | new test | regression-guards Critical 3 |
| 6.5 Auto-restore integration test: cancel + auto-restore → assert engine history truncated to user prompt | new test | regression-guards Critical 2 |
| 6.6 Snapshot review: confirm visual parity with current TUI | `widgets/snapshots/*` | no regression |

## 8. Migration Risks

### 8.1 SDK mode interaction

SDK NDJSON consumers receive `CoreEvent::Protocol`. `HistoryAppended` is a
**Protocol** event so SDK clients see it. This is intentional — SDK
consumers building their own transcript view get the same source of
truth. Audit: existing SDK clients use `ItemCompleted` events for
"message finalized" signals (`StreamAccumulator` converts there); we
preserve that path. `HistoryAppended` is a parallel, higher-level signal
SDK clients can opt into.

**Compatibility**: SDK is versioned by feature flags. Adding event
variants is non-breaking (clients ignore unknown variants per the
`wire_tagged_enum!` contract).

### 8.2 `/resume` JSONL format

Adapter reads canonical `Message` shapes — same as engine. JSONL on-disk
format is unchanged. Backward compatibility with older sessions is the
adapter's responsibility (handle missing fields with sensible defaults).

### 8.3 Snapshot test drift

~30 existing snapshot tests in `widgets/snapshots/` exercise the
`MessageContent` renderer. Phase 4 must update them simultaneously. Risk:
visual differences slip in unnoticed.

**Mitigation**: before Phase 4 lands, diff-compare old vs new snapshot
output (separate verification commit). Land snapshot updates and code
together.

### 8.4 Performance

- `Arc<Message>` in `HistoryAppended` events: one extra Arc clone per
  message push. Negligible.
- TUI dual-write during phases 3-5: `Vec<TranscriptRow>` and
  `Vec<ChatMessage>` both grow. Temporary memory ~2x; auto-resolved at
  Phase 6.
- Adapter call cost: `message_to_rows` allocates a `Vec` per Message.
  Bounded by content blocks (typically 1-3). Negligible.

### 8.5 Cancel-marker dedup during rollout

During Phases 2-5, engine emits `HistoryAppended` for the cancel marker
**AND** TUI still has the old `add_message(ChatMessage::interruption_marker)`
in `on_turn_interrupted`. Risk: double marker until Phase 5.2.

**Mitigation**: Phase 5.2 deletes the redundant TUI-side append in the
same commit that wires the `HistoryAppended` handler. Both sides flip
atomically.

## 9. Verification

### 9.1 Cross-layer end-to-end tests

```rust
// app/query/tests/cancel_synthesis.rs
#[tokio::test]
async fn cancel_marker_consistent_engine_and_tui() {
    let (mut runtime, engine_history, tui_state, event_pipe) = harness();
    runtime.submit("hello").await;
    runtime.cancel().await;
    runtime.drain_events_to_tui(event_pipe).await;

    // Engine: last Message is User with INTERRUPT_MESSAGE_FOR_TOOL_USE
    assert!(is_user_interruption_marker(engine_history.messages.last().unwrap()));
    // TUI: last TranscriptRow is FromMessage::InterruptionMarker
    let row = tui_state.session.transcript.last().unwrap();
    let RowOrigin::FromMessage { kind, .. } = &row.origin else { panic!() };
    assert!(matches!(kind, MessageRowKind::InterruptionMarker { .. }));
    // Both for_tool_use values agree (from same engine source)
}
```

### 9.2 Resume integration

```rust
#[tokio::test]
async fn resume_restores_chat_scrollback_with_interrupt_marker() {
    let jsonl = build_jsonl_with_interrupt();
    let runtime = resume_from(&jsonl).await;
    runtime.drain_events_to_tui().await;
    let rows = &runtime.tui_state.session.transcript;
    // Prior messages rendered as FromMessage rows
    assert!(rows.iter().any(|r| matches!(&r.origin,
        RowOrigin::FromMessage { kind: MessageRowKind::InterruptionMarker { .. }, .. })));
}
```

### 9.3 Auto-restore round-trip

```rust
#[tokio::test]
async fn auto_restore_truncates_engine_history() {
    let runtime = harness_with_prompt_and_partial_response().await;
    runtime.cancel().await;
    runtime.drain_events_to_tui().await;
    // auto_restore should have fired → input has prompt back
    assert_eq!(runtime.tui_state.ui.input.text(), "original prompt");
    // engine history truncated to before that prompt
    assert!(runtime.engine_history.messages.iter()
        .all(|m| !matches!(m, Message::User(u) if u.text() == "original prompt")));
}
```

### 9.4 Snapshot review

- 30 existing TUI snapshots updated to render via `TranscriptRow` path.
- Three new snapshots for: interrupt marker dim row, ephemeral
  streaming-text row, synthetic-row (e.g. hook success).

### 9.5 Smoke matrix

| Scenario | Pass criteria |
|---|---|
| Ctrl+C during streaming text | `Interrupted · …` dim row appears; no panel `! Interrupted` |
| Ctrl+C during FileEdit | tool result row + dim marker; next prompt accepted |
| Ctrl+C twice in a row | single marker in transcript |
| `/resume` after interrupt session | dim marker visible in scrollback; new prompt works |
| Auto-restore (Ctrl+C, empty input, no surface) | input populated, transcript empties to last user prompt; engine history truncated |
| `/clear` then Ctrl+C | no marker (history was cleared by SystemPreempt) |
| Stream interrupt during tool execution | tool_result rows visible; no double marker |

## 10. Acceptance Criteria

This refactor lands when:

1. ✅ `coco-messages::message_to_rows` exists, fully unit-tested for all
   `Message` variants.
2. ✅ Engine emits `HistoryAppended` on every history push.
3. ✅ Engine emits `HistoryReplaced` on every history truncation/clear.
4. ✅ `UserCommand::RewindTo` truncates engine history and replays.
5. ✅ TUI `session.transcript` is the only source the renderer reads from.
6. ✅ `session.messages` field removed.
7. ✅ `/resume` shows prior scrollback including any prior interrupt markers.
8. ✅ Auto-restore truncates engine history (round-trip verified by test).
9. ✅ Cross-layer cancel test passes.
10. ✅ All existing TUI snapshot tests pass with updated golden outputs.
11. ✅ `just quick-check` green; `just pre-commit` green.

## 11. Out of Scope

- `apply_auto_restore` semantics (when to fire) — unchanged from current
  TS-parity rules.
- StreamingState machine internals — `Ephemeral` is a view over it, not a
  rewrite.
- SDK client API surface — adding events is non-breaking; SDK clients
  don't need code changes.
- Permission-prompt rendering — independent system.
- Coordinator/multi-agent transcript merging — `parent_session_id` field
  is a future extension point, not implemented here.

## 12. References

- `event-system-design.md` — three-layer CoreEvent
- `crate-coco-messages.md` — Message variants
- `crate-coco-tui.md` — SessionState model
- `crate-coco-query.md` — engine loop
- `ui/codex-rs-tui-comparison.md` §1 — target "typed transcript cells"
- `streaming-metadata-roundtrip-plan.md` — `Arc<…>` pattern precedent
- `audit-gaps.md` — long-standing `/resume` hydration gap
- TS reference: `query.ts:1015-1052`, `REPL.tsx:2793-3022`,
  `MessageSelector.tsx:799`, `InterruptedByUser.tsx`,
  `utils/messages.ts:545` `createUserInterruptionMessage`
