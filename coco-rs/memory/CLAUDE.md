# coco-memory

Persistent cross-session knowledge: CLAUDE.md management, auto-extraction, session memory, auto-dream consolidation, KAIROS daily logs, team memory sync.

## TS Source
- `src/memdir/` (CLAUDE.md management, scanning, paths, staleness)
- `src/memdir/findRelevantMemories.ts` (LLM-based memory selection)
- `src/services/extractMemories/` (background extraction agent)
- `src/services/SessionMemory/` (session memory persistence)
- `src/services/autoDream/` (KAIROS consolidation + daily logs)
- `src/services/teamMemorySync/` (team memory sync)
- `src/utils/memoryFileDetection.ts` (file classification)

## Modules

| Module | Purpose |
|--------|---------|
| `lib.rs` | Core types (MemoryEntry, MemoryManager, extraction) |
| `config.rs` | MemoryConfig (env vars, feature gates, custom dirs) |
| `security.rs` | Path validation (traversal, null bytes, Unicode) |
| `classify.rs` | File classification (scope, managed check, bypass) |
| `staleness.rs` | Time-based freshness (age, warnings, drift caveat) |
| `scan.rs` | Enhanced scanning (mtime sort, manifest formatting) |
| `prompt.rs` | System prompt section + extraction/KAIROS prompts |
| `prefetch.rs` | Relevant memory selection (LLM + heuristic) |
| `hooks.rs` | Agent loop integration (turn-end extraction trigger) |
| `permissions.rs` | Extraction agent tool permissions (whitelist) |
| `kairos.rs` | KAIROS mode (daily logs, consolidation lock, gates) |
| `team_paths.rs` | Team memory directory management |
| `team_prompts.rs` | Combined personal+team prompt building |
| `telemetry.rs` | Memory analytics events (7 event types) |
| `session_memory.rs` | Per-session insights with category taxonomy |
| `auto_dream.rs` | Consolidation triggers and merge heuristics |
| `team_sync.rs` | Cross-agent memory sync operations |
| `memdir.rs` | Memory directory layout and file management |

## Key Types
MemoryManager, MemoryEntry, MemoryConfig, ExtractionHook, PrefetchState, StalenessInfo, ScannedMemory, MemoryEvent
