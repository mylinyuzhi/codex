# coco-system-reminder

Per-turn dynamic `<system-reminder>` injection. Owns the entire reminder subsystem: types, generators, throttle, orchestration, and message injection.

## TS Source

Logic details are **TS-first** — this crate mirrors the behavior, thresholds, and text content from `claude-code`:

- `src/utils/attachments.ts` — `Attachment` union, `getAttachments()` (parallel batch gather), `getTodoReminderAttachments`, `getPlanModeAttachments`, `getVerifyPlanReminderAttachment`, `getCompactionReminderAttachment`, cadence constants (`TODO_REMINDER_CONFIG`, `PLAN_MODE_ATTACHMENT_CONFIG`, `AUTO_MODE_ATTACHMENT_CONFIG`, `VERIFY_PLAN_REMINDER_CONFIG`)
- `src/utils/messages.ts` — `wrapInSystemReminder` (exact format: `<system-reminder>\n{c}\n</system-reminder>`), `wrapMessagesInSystemReminder`, `ensureSystemReminderWrap`, `smooshSystemReminderSiblings`, `normalizeAttachmentForAPI` (per-reminder text templates)
- `src/utils/api.ts` — `prependUserContext` (cwd/platform/git/date block)
- `src/constants/prompts.ts` — system-prompt mention of `<system-reminder>` semantics

## Architecture Source

Crate layout mirrors `cocode-rs/core/system-reminder/` (same file names, same public API shape) so fixes can be cherry-picked. Logic inside each file tracks TS, not cocode-rs.

## Key Types

- `AttachmentType` — **38 variants**, one per TS `Attachment.type` discriminator. Grouped by port phase:
  - **Phase A/B/C (11)**: `plan_mode` / `plan_mode_exit` / `plan_mode_reentry` / `auto_mode` / `auto_mode_exit` / `todo_reminder` / `task_reminder` / `critical_system_reminder` / `compaction_reminder` / `date_change` / `verify_plan_reminder`.
  - **Phase 1 engine-local (5)**: `ultrathink_effort` / `token_usage` / `budget_usd` / `output_token_usage` / `companion_intro`.
  - **Phase 2 history-diff (3)**: `deferred_tools_delta` / `agent_listing_delta` / `mcp_instructions_delta`.
  - **Phase 3 cross-crate (14)**: `hook_success` / `hook_blocking_error` / `hook_additional_context` / `hook_stopped_continuation` / `async_hook_response` / `diagnostics` / `output_style` / `queued_command` / `task_status` / `skill_listing` / `invoked_skills` / `teammate_mailbox` / `team_context` / `agent_pending_messages`.
  - **Phase 4 user-input (5, UserPrompt tier)**: `at_mentioned_files` / `mcp_resources` / `agent_mentions` / `ide_selection` / `ide_opened_file`.
  - `AttachmentType::all()` returns the full catalog; the `all_attachment_type_variants_have_default_generator` parity test asserts every variant has a registered generator.
- `ReminderTier` — `Core` (all agents), `MainAgentOnly` (main-thread only — `verify_plan_reminder`), `UserPrompt` (when user input present). Maps to TS's three parallel batches in `getAttachments`.
- `XmlTag` — `SystemReminder` = `<system-reminder>`, `None` = raw.
- `SystemReminder` — unified generator output: `{ attachment_type, output: ReminderOutput, is_meta, is_silent }`.
- `ReminderOutput::{ Text | Messages | ModelAttachment }` — matches TS `wrapMessagesInSystemReminder` (Text) shape.
- `AttachmentGenerator` trait (`async_trait`) — one impl per reminder type. 4-hook lifecycle: `is_enabled`, `tier`, `throttle_config_for_context`, `generate`.
- `GeneratorContext<'a>` — per-turn state: permission-mode flags, tool list, turn-since-* counters, todos/plan_tasks, context-window metrics, date-change + verify-plan signals, full-content flags pre-computed by the orchestrator.
- `ThrottleManager` / `ThrottleConfig` — central rate limiter keyed by `AttachmentType`. Fields match TS constants 1:1 (`min_turns_between` = `TURNS_BETWEEN_*`, `full_content_every_n` = `FULL_REMINDER_EVERY_N_*`). Presets: `plan_mode` / `auto_mode` / `todo_reminder` / `verify_plan_reminder` / `none`.
- `SystemReminderOrchestrator` — parallel execution with per-generator timeout.
- `TurnReminderInput` + `run_turn_reminders()` — one-call engine entry point; packages every per-turn input as named struct fields.
- `InjectedMessage` / `InjectedBlock` — post-orchestration conversion; `inject_reminders` writes `coco_types::Message::Attachment` with `is_meta=true` + `origin=SystemInjected`.
- `SystemReminderConfig` / `AttachmentSettings` — **live in `coco-config`** (re-exported here). Wired via `Settings.system_reminder` so every reminder can be toggled from `settings.json`.

## Module Layout

```
src/
├── error.rs          SystemReminderError + ErrorExt (codes 13_xxx)
├── types.rs          AttachmentType, ReminderTier, XmlTag, SystemReminder, ReminderOutput
├── xml.rs            wrap_with_tag / extract_system_reminder
├── throttle.rs       ThrottleConfig (+ presets) + ThrottleManager
├── generator.rs      AttachmentGenerator trait + GeneratorContext(Builder)
├── orchestrator.rs   SystemReminderOrchestrator (parallel + timeout)
├── inject.rs         InjectedMessage -> coco_types::Message
├── context_builder.rs app_state → GeneratorContext mapping helpers
├── turn_counting.rs  count_assistant_turns_since_tool/any_tool, count_human_turns
├── turn_runner.rs    TurnReminderInput + run_turn_reminders (engine entry)
├── generators/       plan_mode, auto_mode(_enter), todo_reminders, task_reminders,
│                     critical_system_reminder, compaction_reminder, date_change,
│                     verify_plan (MainAgentOnly)
└── lib.rs            module declarations + re-exports (SystemReminderConfig from coco-config)
```

## Key Invariants

- **TS-first**: if `cocode-rs` and TS disagree on cadence, text content, or trigger conditions, follow TS. cocode-rs provides the crate shape, not the logic.
- **Human-turn UUID throttle**: plan-mode cadence counts non-meta user messages, not LLM iterations (TS `getPlanModeAttachmentTurnCount`). The engine tracks `last_human_turn_uuid_seen` on `ToolAppState` and advances the throttle counter only on a new UUID; `PlanModeReminder::turn_start_side_effects_only` writes it.
- **is_meta=true on all reminders**: hidden from UI transcripts, sent to the API wrapped in `<system-reminder>`. Source of truth: TS `createAttachmentMessage` + `wrapMessagesInSystemReminder`.
- **Per-generator timeout**: `SystemReminderConfig::timeout_ms` (default 1000ms, matches TS `attachments.ts:767` per-batch AbortController). Timed-out generators produce zero reminders; the turn continues.
- **Typed `ToolName` throughout**: no hand-written tool-name strings in gates or cadence helpers. `TASK_MANAGEMENT_TOOLS` + `count_assistant_turns_since_tool(ToolName::X)` thread typed references through so a `ToolName` rename propagates automatically.
- **Auto-mode gate**: `is_auto_mode == (mode == Auto) || (mode == Plan && is_auto_classifier_active)`. The engine reads `AutoModeState::is_active()` from `core/permissions` and threads it into `TurnReminderInput`.
- **Cross-run cadence**: each `run_session_loop` invocation constructs a fresh orchestrator. Engine seeds `ThrottleManager::seed_state` from `app_state.plan_mode_attachment_count` + `turns_since_last_attachment` so cadence survives. Post-emit bookkeeping mirrors the throttle back onto `app_state`.

## What this crate does NOT own

- **File / image / PDF / memory attachments** — those stay in `core/context::Attachment` (user-input-side, token-budgeted + deduped). Phase 4's `at_mentioned_files` generator emits a *reminder* (listing display paths); the **file content** still flows through `core/context`.
- **Static system prompt assembly** — that's `core/context::build_system_prompt`. This crate handles *dynamic per-turn* injection only.
- **Compaction summarization** — `services/compact` owns that. Per TS parity, `plan_file_reference` is emitted **by** the compaction pipeline (not this crate) so it survives the context-bust.
- **Cross-crate data sources for Phase 3/4 snapshots** — each owning crate (`services/lsp`, `hooks`, `tasks`, `skills`, `app/state/swarm`, `app/query::CommandQueue`, `bridge`) populates a typed `*Snapshot` / `*Info` struct on `GeneratorContext`. The generators in `src/generators/` only *render* — they never call into a sibling crate.
