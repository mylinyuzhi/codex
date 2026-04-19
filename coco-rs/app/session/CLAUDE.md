# coco-session

Session persistence, transcript history, cost recovery, and title generation.
One `Session` JSON per session id under the configured sessions dir; transcript
and cost accounting kept alongside.

## TS Source

- `history.ts` — PromptHistory (user-typed prompt ring)
- `utils/cleanup.ts` — terminal teardown, autosave on exit
- `bootstrap/state.ts` — session identity (id, cwd, model) + cost accumulators
- `setup.ts` — TS setup flow (trust / onboarding / migrations); Rust equivalent lives in `coco-cli` + `coco-memory`
- `utils/sessionStorage.ts` — session JSONL transcript IO (readFileTail, append, list, delete)

## Key Types

| Type | Purpose |
|------|---------|
| `Session` | `{id, created_at, updated_at?, model, working_dir, title?, message_count, total_tokens}` |
| `SessionManager` | `create` / `save` / `load` / `resume` / `list` / `delete` / `most_recent` / `cleanup(keep_count)` — JSON files under `sessions_dir` |
| `TranscriptStore`, `TranscriptEntry`, `TranscriptMetadata`, `TranscriptUsage` | Append-only transcript (JSONL) with per-entry usage |
| `Entry`, `MetadataEntry`, `ModelCostEntry`, `RestoredCostSummary`, `restore_cost_from_transcript` | Cost recovery on resume — walks transcript, folds per-model usage, rebuilds `CostTracker` |
| `PromptHistory`, `HistoryEntry` | Ring of user-typed prompts (for up-arrow recall) |
| `recovery::*` | Crash recovery — partial transcript repair + last-good-state detection |
| `storage::*` | Low-level JSON / JSONL IO |
| `title_generator::*` | Auto-titling via `ModelRole::Fast` (short session label after first turn) |

## Layout

```
<sessions_dir>/
├── <session-id>.json           # Session metadata (SessionManager)
├── transcripts/<session-id>.jsonl  # Append-only TranscriptStore
└── history.json                # PromptHistory (global, not per-session)
```

`sessions_dir` defaults to `~/.coco/sessions/`. CLI controls it; this crate is
agnostic.

## Title Generation

`title_generator` calls the `Fast` model role after the first assistant turn
with a short prompt summarizer. The user toggles via `session.auto_title: bool`
in settings (see CLAUDE.md multi-provider model references rule — never a bare
model string).
