# coco-context

System context assembly: environment info, CLAUDE.md discovery, attachments, file history, plan mode, memory files, mentions, prompt building.

## TS Source
- `context.ts` — system/user context injection
- `utils/claudemd.ts` (46K) — CLAUDE.md discovery + loading
- `utils/attachments.ts` (4K) — file/PDF/memory/hook attachments
- `utils/cwd.ts` — working directory management
- `utils/systemPromptType.ts` — system prompt building
- `utils/fileHistory.ts` (~1110) — per-turn file edit tracking + content-addressed snapshots
- `utils/toolResultStorage.ts` — ContentReplacementState
- `utils/fileStateCache.ts` — LRU file read cache
- `utils/pasteStore.ts`, `utils/filePersistence/`
- `services/AgentSummary/` — agent activity summary
- `services/awaySummary.ts` — away summary

## Key Types

- **Environment**: `EnvironmentInfo`, `Platform`, `ShellKind`, `GitStatus`, `get_environment_info`
- **CLAUDE.md**: `ClaudeMdFile`, `ClaudeMdSource`, `discover_claude_md_files`
- **Attachments**: `Attachment`, `AttachmentBatch`, `AttachmentBudget`, `AttachmentDeduplicator`, `AttachmentSource`, `ReminderType`, `collect_batched_attachments`, `generate_all_attachments_async`
- **Plan mode**: `PlanModeAttachment`, `PlanModeExitAttachment`, `PlanWorkflow`, `Phase4Variant`, `PlanVerificationOutcome`, plan file management (`get_plan`, `set_plan_slug`, `write_plan`, `delete_plan`, `verify_plan_was_edited`, `recover_plan_for_resume`, `render_plan_mode_reminder`, `render_plan_mode_exit_reminder`, `render_auto_mode_exit_reminder`, `resolve_plans_directory`, `generate_word_slug`, etc.)
- **File history**: `FileHistoryState`, `FileHistorySnapshot`, `FileHistoryBackup`, `DiffStats`, `backup_dir`, `copy_file_history_for_resume`
- **File read state / cache**: `FileReadState`, `FileReadEntry`, `FileReadCache`, `file_mtime_ms`
- **Changed files**: `detect_changed_files`
- **Memory**: `MemoryFileInfo`, `MemoryType`
- **Mentions**: `MentionResolveOptions`, `resolve_mentions`
- **User input**: `ProcessedInput`, `Mention`, `MentionType`, `process_user_input`
- **Prompt**: `SystemPrompt`, `SystemPromptBlock`, `build_system_prompt`, `build_minimal_prompt`
- **Tokens**: `estimate_tokens`, `estimate_tokens_for_messages`, `is_over_threshold`

## Module Layout

`attachment`, `changed_files`, `claudemd`, `environment`, `file_cache`, `file_history`, `file_read_state`, `git_operations`, `git_utils`, `memory`, `mention_resolver`, `plan_mode`, `prompt`, `prompt_suggestion`, `suggestions`, `token_estimation`, `user_input`, `vim_mode`, `worktree`.

## Architecture

- `Platform` + `ShellKind` enums owned here (cross-crate env types).
- File history uses ordered `Vec` + content-addressed files on disk (TS-aligned; NOT HashMap).
- Plan mode scoped by session-local `plan_slug` for fork/resume isolation.
- Phase-4 + Interview plan workflows exposed via `settings.json` (`plan_mode.phase4_variant`, `plan_mode.workflow`) — no GrowthBook / `USER_TYPE=ant` env vars. Ultraplan (CCR web UI) intentionally skipped.
