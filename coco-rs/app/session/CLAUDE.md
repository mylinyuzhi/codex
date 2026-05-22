# coco-session

JSONL-canonical session persistence, transcript history, cost recovery, title
generation, and per-process concurrent-session registry. No `<session-id>.json`
sidecar — every session-level fact (title, tags, model, created/updated_at,
message counts) is derived from the transcript's first entry plus trailing
metadata. This matches TS Claude Code and removes a class of state-drift bugs
the pre-refactor sidecar enabled.

## TS Source

- `history.ts` — PromptHistory (user-typed prompt ring)
- `utils/cleanup.ts` — terminal teardown, autosave on exit
- `bootstrap/state.ts` — session identity (id, cwd, model)
- `setup.ts` — TS setup flow (trust / onboarding / migrations); Rust equivalent
  lives in `coco-cli` + `coco-memory`
- `utils/sessionStorage.ts` — JSONL transcript IO (readFileTail, append, list,
  delete, lite metadata)
- `utils/sessionStoragePortable.ts` — `resolveSessionFilePath`, cross-project
  enumeration, worktree fallback
- `utils/concurrentSessions.ts` — per-PID registry for `claude ps`
- `services/sessionTitle.ts` — Haiku-based auto-title

## Key Types

| Type | Purpose |
|------|---------|
| `Session` | Derived view `{id, created_at, updated_at?, model, working_dir, title?, message_count, total_tokens, tags}` — built from `TranscriptMetadata`, never persisted as its own file |
| `SessionManager` | `create` (in-memory only) / `save` (no-op shim) / `load` / `resume` / `list` / `delete` / `most_recent` / `cleanup(keep_count)` / `cleanup_older_than` |
| `TranscriptStore`, `TranscriptEntry`, `TranscriptMetadata`, `TranscriptUsage` | Append-only JSONL transcript with per-entry usage. Path layout via `Arc<ProjectPaths>` |
| `Entry`, `MetadataEntry` | Tagged union: transcript message vs metadata entry (custom-title, tag, last-prompt, summary, file-history-snapshot, marble-origami-{commit,snapshot}, content-replacement, …) |
| `ModelCostEntry`, `RestoredCostSummary`, `restore_cost_from_transcript` | Cost recovery on resume — walks transcript, folds per-model usage, rebuilds `CostTracker` |
| `PromptHistory`, `HistoryEntry` | Ring of user-typed prompts (for up-arrow recall) |
| `AgentMetadata` | Sidecar for AgentTool spawns at `<sid>/subagents/agent-<id>.meta.json` |
| `recovery::*` | Crash recovery — partial transcript repair + last-good-state detection |
| `storage::*` | Low-level JSON / JSONL IO + cross-project enumeration |
| `title_generator::*` | Auto-titling via `ModelRole::Fast` (short session label after first turn) |
| `SessionRegistry`, `SessionRegistration`, `SessionKind`, `SessionStatus` | PID-file registry for `coco ps` — drop the guard to deregister, write-lock-serialized live patches |
| `count_concurrent_sessions`, `is_bg_session`, `read_session_registration` | Cross-process enumeration helpers |

## Layout

All session artifacts live under `<memory_base>/projects/<slug>/` (resolved via
`coco_paths::ProjectPaths`). `memory_base` defaults to
`coco_config::config_home()` and is overridable via `COCO_REMOTE_MEMORY_DIR`
(CCR / swarm leader). The slug is the `[a-zA-Z0-9]→-` sanitized + NFC-
normalised canonical git root of the cwd, with a djb2 suffix for paths over
200 bytes — see `coco-paths::ProjectSlug`.

```
<memory_base>/
├── projects/
│   └── <slug>/                              # per-project root
│       ├── <session-id>.jsonl               # append-only transcript
│       └── <session-id>/                    # per-session artifacts
│           ├── subagents/
│           │   ├── agent-<id>.jsonl         # bg agent transcript
│           │   └── agent-<id>.meta.json     # AgentMetadata sidecar
│           ├── remote-agents/
│           │   └── remote-agent-<tid>.meta.json
│           ├── tool-results/                # persisted tool-result blobs
│           └── session-memory/
│               └── summary.md               # 9-section per-session memory
└── sessions/                                # cross-PID registry
    ├── <pid>.json                           # SessionRegistration (claude ps)
    └── ...
```

`history.json` (PromptHistory) lives under `<config_home>` directly, not in
`<memory_base>/projects/...` — it's user-typed input recall, not session
state.

## Canonical-path invariant

`storage::resolve_session_file_path` and `coco_memory::path::MemoryDir::resolve`
both anchor on `coco_git::find_canonical_git_root(cwd)`. This is **load-bearing**:
if the two diverge (e.g. one passes a worktree path), the session transcript
and its memory dir land under different `<slug>`s and the session's memory is
invisible to the session. Any new caller computing a project root MUST go
through `coco_git::find_canonical_git_root`.

## Concurrent session registry

`SessionRegistry` writes one `<pid>.json` file per top-level session under
`<config_home>/sessions/`. Subagents (TS `getAgentId() != null`) intentionally
do NOT register — counting them would conflate swarm activity with real
concurrency. Live patches (`update_session_name`, `update_session_bridge_id`,
`update_session_activity`) are serialised via the registry's internal write
lock so a teleport rename racing with the per-turn activity update can't lose
fields. The stale-PID sweep in `count_concurrent_sessions` uses a strict
`^\d+\.json$` filename guard so unrelated `*.json` files in the same dir are
never collected.

## Title Generation

`title_generator` calls the `Fast` model role after the first assistant turn
with a short prompt summarizer. The user toggles via `session.auto_title: bool`
in settings (see root CLAUDE.md multi-provider model-references rule — never a
bare model string).

## Timestamps

All timestamps in this crate (Session.created_at, TranscriptMetadata.{created,
modified}_at) are **milliseconds-since-epoch** as ASCII decimal strings —
`format!("{}", systemtime.as_millis())`. The cross-project newest-first sort
in `list_all_sessions` parses these as `u128` and compares; mixing seconds
with milliseconds silently mis-sorts. `timestamp_now()` is the canonical
emitter and MUST stay milliseconds.
