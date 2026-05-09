# coco-memory

Persistent cross-session memory: per-project memory directory + 4-type
taxonomy (User / Feedback / Project / Reference) + auto-extraction +
auto-dream consolidation + per-session 9-section memory + KAIROS daily
logs + LLM-ranked recall.

## TS Source

- `memdir/{memdir,memoryTypes,paths,memoryScan,memoryAge,findRelevantMemories,teamMemPaths,teamMemPrompts}.ts`
- `services/extractMemories/{extractMemories,prompts}.ts`
- `services/SessionMemory/{sessionMemory,sessionMemoryUtils,prompts}.ts`
- `services/autoDream/{autoDream,config,consolidationLock,consolidationPrompt}.ts`
- `utils/memoryFileDetection.ts`

## Crate Layout

```
src/
├── store/                pure data: types, frontmatter parse, MEMORY.md index + truncate
├── path/                 git-canonical resolve, validate, scope, symlink walk
├── scan.rs               single Scanner (200-cap, 30-line frontmatter read, mtime sort, manifest fmt)
├── lock.rs               PID + mtime CAS lock (auto-dream); 1h dead-PID reclaim, rollback
├── recall.rs             relevant-memory selection (LLM ranker prompt + heuristic fallback + PrefetchState)
├── compact_truncate.rs   pure session-memory section truncation
├── prompt/
│   ├── builders.rs       compose system / extract / dream / session-update prompts
│   └── text/*.md         verbatim include_str! blocks (taxonomy, what-not-to-save, …)
├── service/
│   ├── extract.rs        ExtractService — turn-end fork via AgentHandle (fork_context_messages, max_turns=5, memdir-only fence, stash + trailing run, 60s drain)
│   ├── dream.rs          DreamService — 3-gate scheduler (24h/5-session/10-min throttle), PID lock, 4-phase fork
│   └── session.rs        SessionMemoryService — 10k/5k/3 trigger gates, 9-section template, 15s wait_for_extraction, file 0o600
├── runtime.rs            MemoryRuntime + Builder; owns services, recall_state, optional SideQueryHandle for LLM ranker
├── config.rs             thin runtime adapter over `coco_config::MemoryConfig`
├── telemetry.rs          MemoryEvent enum + MemoryTelemetryEmitter trait + OtelEmitter adapter
└── lib.rs                module declarations + re-exports
```

## Key Types

| Type | Purpose |
|------|---------|
| `MemoryEntry` / `MemoryEntryType` / `MemoryFrontmatter` | parsed memory file (closed 4-type taxonomy) |
| `MemoryIndex` / `MemoryIndexEntry` | parsed `MEMORY.md` pointer list |
| `EntrypointTruncation` | line-then-byte truncation of `MEMORY.md` |
| `MemoryDir` | resolved personal + team directory pair |
| `PathValidationError` | path-validation taxonomy (null / UNC / drive-root / tilde / fullwidth / traversal) |
| `Scanner` (free fns) | `scan_memory_files`, `format_memory_manifest`, `memory_age_string`, `file_mtime_ms` |
| `PrefetchState` | per-session already-surfaced + byte-budget tracker for recall |
| `RelevantMemory` | path + truncated content + freshness header |
| `ExtractService` / `DreamService` / `SessionMemoryService` | the three async services |
| `MemoryRuntime` / `MemoryRuntimeBuilder` | session-level composer |
| `SessionEnumerator` | `Arc<dyn Fn() -> Vec<String>>` — TranscriptStore-backed lazy session lister wired by `install_session_enumerator`; consumed by `tick_dream` |
| `MemoryEvent` / `OtelEmitter` | telemetry taxonomy + OTel adapter |

## Multi-Provider Notes

- All LLM calls go through `coco-tool-runtime::SideQuery` or `AgentHandle`. **Never hardcode a model_id.**
- The recall ranker uses `SideQueryRequest::with_model_role(ModelRole::Memory)` so the operator picks provider+model in `settings.models.memory`.
- The forked extraction / dream agents inherit the parent session's `tool_overrides`, `features`, `parent_tool_filter`. The constraints layer (`AgentSpawnConstraints`) only narrows: `max_turns: 5`, `allowed_write_roots: [memdir]`.
- `MemoryConfig` (in `coco-config`) is the single source of truth for sub-toggles (extraction / team / dream / session-memory / kairos). Sub-toggles never become `Feature` variants — `Feature::AutoMemory` is the one upstream gate.

## Invariants

- `MEMORY.md` is **model-curated**; the runtime never auto-regenerates it. We only read + truncate (line 200 / 25 KB caps).
- `is_team_memory_path` uses an authoritative `MemoryDir` resolution + a `**/memory/team/**` substring fallback (gated by the secret detector).
- Path resolution is anchored to `coco_git::find_canonical_git_root` so worktrees of one repo share one memdir.
- `ExtractService::run` always sets `isolation = "fork"` + `fork_context_messages` so the child sees the parent's slice — TS parity (`AgentTool.tsx:622-632`).
- The write fence resolves relative paths against `ToolUseContext::cwd_override` before checking, so a model passing `./notes.md` lands inside the fence as expected.
- `DreamService::maybe_consolidate` checks gates in TS-parity order (time → scan throttle → session) and accepts `enumerate_sessions` as `FnOnce()` — the closure runs **only** after the time + scan gates pass so callers don't pay enumeration cost on every turn.
- `DreamService` rolls the lock mtime back on failure so the time-gate doesn't reset to "now"; the failure path also emits `MemoryEvent::AutoDreamFailed` for telemetry parity with TS `tengu_auto_dream_failed`.
- `MemoryRuntime::tick_dream` is the engine's per-turn entry point: it calls the installed `SessionEnumerator` (TranscriptStore-backed) lazily and threads the runtime's `transcript_dir`. Engine fans this out alongside extract + session-memory in `engine_finalize_turn`.
- `ExtractService` emits `ExtractionCoalesced` when a request stashes for a trailing run and `ExtractionError` on subagent failure — full telemetry coverage of TS `tengu_extract_memories_*` events.
- `SessionMemoryService` writes the 9-section template if missing, then asks the agent to Edit-only — never overwrites the file wholesale.
- `MemoryEvent::ExtractionCompleted::files_written` sums real `Write + Edit + NotebookEdit` invocations from `AgentSpawnResponse::tool_use_counts` — no fabricated counts.

## What this crate does NOT own

- The system-prompt assembly seam (`coco-context::build_system_prompt`) — memory only renders its block via `prompt::build_system_prompt_section` and hands it through.
- LLM client construction — see `coco-inference`.
- Session storage, transcripts — see `coco-session`.
- Team-memory HTTP sync — deferred (the v1 `team_sync.rs` skeleton was deleted; v2 will port `services/teamMemorySync/` properly).
- Compaction logic — `coco-compact` reads session-memory off disk via `MemoryRuntime::session_memory.current_content().await` and uses our pure `compact_truncate::truncate_session_memory_for_compact` helper.

## Cargo deps

`coco-config`, `coco-types`, `coco-tool-runtime`, `coco-frontmatter`, `coco-git`, `coco-otel`,
`tokio`, `serde`, `serde_json`, `thiserror`, `tracing`, `filetime`, `libc` (cfg(unix)).

`coco-messages` / `coco-inference` are intentionally not deps — services use the `AgentHandle`
and `SideQuery` traits from `coco-tool-runtime` instead, which keeps the layer rules clean.
