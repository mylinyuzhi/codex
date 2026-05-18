# Engine/TUI Transcript Sync Economic Plan

Date: 2026-05-18

## Summary

This plan fixes the current TUI/engine transcript sync bugs without doing the
larger `HistoryAppended` / `HistoryReplaced` bridge refactor.

Recommended path:

- Keep engine/model history and TUI display state separate.
- Seed TUI scrollback explicitly on `/resume`.
- Synchronize engine truncation only for TUI auto-restore after user cancel.
- Preserve the cancel-marker boundary: engine owns model-visible markers, TUI
  owns display-only markers.
- Leave SDK transcript schema and AgentTeams shared transcript as future work.

Non-goals:

- Do not add `HistoryAppended` or `HistoryReplaced`.
- Do not expose `coco_messages::Message` through the SDK protocol.
- Do not split the `MessageContent` enum.
- Do not introduce a dual-write migration period.
- Do not broadcast large history payloads through protocol notifications.

Success criteria:

- After `/resume`, TUI scrollback shows prior user, assistant, and tool-result
  messages.
- After auto-restore, TUI display state and engine runtime history are both
  truncated before the same user message.
- The cancel marker visible to the model is written only by the engine.
- The TUI interruption marker remains display-only.
- SDK behavior is unchanged.

## Current State

coco-rs has three separate history surfaces with different jobs:

- `coco_messages::MessageHistory` / `runtime.history`: engine/model authority.
- `coco_tui::SessionState.messages`: terminal display state.
- SDK `SessionHandle.history` plus JSONL transcript: model continuity across
  SDK turns and session resume.

These should not be collapsed into one internal type. The economical fix is to
add explicit synchronization at the few lifecycle points where the separation is
currently observable.

codex-rs uses the same shape at a high level:

- Core thread storage owns thread/turn history.
- TUI owns display cells.
- Resume and rollback use explicit seed/replay APIs.
- Public API uses stable display-oriented models such as `Thread`, `Turn`, and
  `ThreadItem`, not internal prompt messages.

TS `claude-code` mirror behavior is also an external view:

- Mirror is outbound-only.
- First connection sends initial messages.
- Message UUIDs are used for dedupe.
- Mirror does not write internal authoritative history.

## Change 1: `/resume` Scrollback Seed

Keep the existing engine seed in `app/cli/src/tui_runner.rs`:

- `runtime.start_new_session(plan.session_id)`
- `runtime.history = plan.prior_messages`
- `seed_transcript_dedup`
- `seed_tool_result_replacement_state`

Add a TUI display seed immediately after `App::new(...)`:

- Convert `plan.prior_messages` into `coco_tui::state::ChatMessage`.
- Append the converted rows to `app.state_mut().session.messages`.
- Keep the converter in `app/cli`, so `coco-tui` does not depend on
  `coco-messages`.

Lossy mapping rules:

- `Message::User` text -> `ChatMessage::user_text(uuid, text)`
- interrupt literal -> `ChatMessage::interruption_marker(uuid, tool_use_flag)`
- `Message::Assistant` text -> `ChatMessage::assistant_text(uuid, text)`
- `Message::ToolResult` -> `ChatMessage::tool_success` or
  `ChatMessage::tool_error`
- skip internal messages that cannot be displayed safely

Preserve UUIDs as `ChatMessage.id`. This keeps rewind and auto-restore aligned
with engine history.

## Change 2: Auto-Restore Engine Truncate

Current auto-restore in `on_turn_interrupted` mutates only TUI state. Add a
small private synchronization path.

Proposed flow:

- Add internal `UserCommand::AutoRestoreTruncate { message_id: String }`.
- In `apply_auto_restore(state, idx)`, capture the target user
  `message_id` before truncating TUI display state.
- Store that id in a pending field on `SessionState`.
- After `server_notification_handler::handle_core_event` runs,
  `App::handle_core_event` drains the pending field and sends
  `UserCommand::AutoRestoreTruncate`.
- In `tui_runner`, handle the command by locking `runtime.history`, finding
  `Message::User.uuid == message_id`, and calling `truncate(idx)`.

Important boundaries:

- Do not emit `RewindCompleted`.
- Do not emit a protocol event.
- Do not restore files.
- Treat missing message ids as no-op.

Auto-restore is a private user-cancel cleanup path, not explicit rewind.

## Change 3: Cancel Marker Authority

Keep the current boundary:

- `app/query` is the only writer of model-visible interrupt marker messages.
- `MessageContent::InterruptionMarker` is a TUI-only row.
- The TUI marker must not be converted back into `coco_messages::Message`.

Do not remove the TUI `for_tool_use` field in this pass. It can remain as a
rendering hint to avoid snapshot churn, but it is not semantically
authoritative. Rendering should stay generic.

Regression coverage should verify:

- User cancel appends at most one model-visible interrupt marker to engine
  final history.
- TUI displays at most one visible interruption marker.
- System preempt does not auto-restore and does not append a visible user-cancel
  marker.

## Change 4: SDK And Mirror Boundary

Do not change SDK public protocol in this pass.

Explicitly avoid:

- `ServerNotification::HistoryReplaced { messages }`
- Protocol broadcasts of `Vec<coco_messages::Message>`
- SDK reads from TUI-only transcript state
- `coco_messages::Message` as public transcript schema

Current SDK model-history continuity is handled by `SessionHandle.history` and
JSONL resume. That path does not need this TUI sync work.

## Future Extension: SDK Transcript View

If SDK, IDE, or web clients need a renderable transcript, design a separate
public schema:

- `Thread`: session/thread metadata
- `Turn`: one user prompt through agent completion
- `ThreadItem`: stable display units such as user message, assistant message,
  reasoning, tool call, file change, web search, and subagent event

Possible API shapes:

- `session/read(include_messages)`
- `session/turns/list`

The schema may be lossy, but it must be stable, SDK-friendly, and decoupled
from engine prompt history.

Trigger conditions:

- SDK clients start rendering transcript history.
- `session/read.messages` moves beyond the current empty implementation.
- IDE or mirror clients require historical replay.

## Future Extension: AgentTeams Shared Transcript

Current AgentTeams needs per-agent JSONL transcript persistence for background
agent resume. That already exists.

A shared transcript is a future coordinator UX feature:

- merged leader + teammate timeline
- agent status and message aggregation
- optional shared readable context across agents

This belongs in AgentTeams/coordinator design, not in the current
resume/cancel/auto-restore fix.

## Future Extension: Mirror Behavior

Future remote mirrors should follow the TS mirror boundary:

- mirror is an external view, not an internal writer
- initial connection receives initial transcript
- UUIDs dedupe replayed messages
- mirror does not arbitrate engine/TUI state

## Test Plan

Run from `coco-rs/`.

TUI protocol tests:

- user cancel with lossless tail auto-restores and produces pending truncate
  command
- system preempt does not auto-restore
- auto-restore restores input, truncates TUI messages, and does not append a
  visible interruption marker

CLI runner tests:

- `AutoRestoreTruncate` truncates `runtime.history` before the target user
  message
- missing target id is no-op

Resume tests:

- resume plan seeds both engine history and TUI scrollback
- interrupt marker literals render as UI interruption markers in resumed
  scrollback

Query tests:

- cancel during tool use and non-tool cancel append at most one engine
  interrupt marker

SDK regression:

- event stream does not contain history replacement payloads
- `session/read` result shape and current empty `messages` behavior remain
  unchanged

Suggested verification:

```bash
just quick-check
just test-crate coco-tui
just test-crate coco-cli
just pre-commit
```

Use `just pre-commit` only once, at the final commit gate.

## Assumptions

- The immediate goal is to fix TUI/engine sync bugs, not design a public SDK
  transcript API.
- `/resume` scrollback can use lossy replay for now.
- Auto-restore is private user-cancel behavior and should not notify SDK
  clients.
- AgentTeams shared transcript is future coordinator work.
