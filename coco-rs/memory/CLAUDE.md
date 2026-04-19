# coco-memory

Persistent cross-session knowledge: CLAUDE.md management, per-project MEMORY.md index, 4-type taxonomy (User/Feedback/Project/Reference), two-phase LLM auto-extraction (fast candidate extraction + capable-model consolidation), session memory, auto-dream consolidation with three-gate scheduling, KAIROS daily logs, team memory sync.

## TS Source
- `memdir/memdir.ts`, `memoryTypes.ts`, `paths.ts`, `memoryScan.ts`, `memoryAge.ts` — directory layout, taxonomy, scan, staleness
- `memdir/findRelevantMemories.ts` — LLM-based recall selector (5 file cap)
- `memdir/teamMemPaths.ts`, `teamMemPrompts.ts` — team memory layout
- `services/extractMemories/extractMemories.ts`, `prompts.ts` — forked extraction agent
- `services/SessionMemory/sessionMemory.ts`, `sessionMemoryUtils.ts`, `prompts.ts` — 9-section template
- `services/autoDream/autoDream.ts`, `config.ts`, `consolidationLock.ts`, `consolidationPrompt.ts` — KAIROS consolidation
- `services/teamMemorySync/index.ts`, `watcher.ts`, `secretScanner.ts`, `teamMemSecretGuard.ts`, `types.ts` — team sync + secret-scan gate
- `utils/memoryFileDetection.ts` — file classification

Paths relative to `/lyz/codespace/3rd/claude-code/src/`.

## Key Types
- `MemoryEntry` — name, description, `memory_type` (`User`/`Feedback`/`Project`/`Reference`), content, file_path
- `MemoryEntryType` — snake_case serde with `as_str()`
- `MemoryFrontmatter` — parsed YAML header
- `MemoryIndex` / `MemoryIndexEntry` — MEMORY.md pointer list
- `MemoryManager` — CRUD on `.claude/memory/*.md` + `MEMORY.md` regeneration
- `extraction::MemoryCandidate` — name, description, type, content, confidence (0..1)
- `extraction::ExtractionConfig` — `max_candidates` (10), `min_confidence` (0.5)
- `extraction::extract_memories()` / `consolidate_memories()` — generic callback-driven LLM calls

## Modules

| Module | Purpose |
|--------|---------|
| `lib.rs` | Core types, `MemoryManager`, `extraction` (Phase 1/2 helpers) |
| `config.rs` | `MemoryConfig` — env-var overrides, feature gates, custom dirs |
| `security.rs` | Path validation: traversal, null bytes, UNC, drive roots, Unicode |
| `classify.rs` | File classification (scope, managed check, bypass) |
| `staleness.rs` | Age formatting, freshness caveat ("may be stale"), drift wrapper |
| `scan.rs` | Scan cap (200 files), frontmatter-only read, mtime sort, manifest |
| `prompt.rs` | System-prompt section + extraction/consolidation/KAIROS prompts |
| `prefetch.rs` | LLM + heuristic relevant-memory selection |
| `hooks.rs` | Turn-end extraction trigger from agent loop |
| `permissions.rs` | Extraction-agent tool whitelist (Read/Grep/Glob + scoped Write/Edit) |
| `kairos.rs` | KAIROS daily logs (`logs/YYYY/MM/YYYY-MM-DD.md`), consolidation lock, mode gates |
| `team_paths.rs` | Dual-directory (private `~/.coco/projects/<hash>/` + team `.coco/memory/`) |
| `team_prompts.rs` | Combined personal + team prompt assembly |
| `telemetry.rs` | 7 analytics event types for memory lifecycle |
| `session_memory.rs` | 9-section per-session insights with category taxonomy + budgets |
| `auto_dream.rs` | Three-gate schedule (time/scan-throttle/sessions), four-phase prompt |
| `team_sync.rs` | Cross-agent sync + secret-scan guard |
| `memdir.rs` | Memory directory layout helpers |

## Auto-Dream / Extraction Notes
- Lock file: `.consolidate-lock`; mtime = `lastConsolidatedAt`, body = holder PID; dead-PID reclaim after 1h stale
- Consolidation phases: Orient → Gather → Consolidate → Prune-and-index
- Disabled in KAIROS mode and remote mode
- Extraction agent runs forked with hard-cap of 5 turns, writes restricted to memdir
