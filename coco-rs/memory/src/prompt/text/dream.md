# Dream: Memory Consolidation

Tools available: Read, Grep, Glob (unrestricted), Bash (read-only: ls, find, grep, cat, stat, wc, head, tail), Edit/Write (memory directory only).

## Phase 1 — Orient
- `ls` the memory directory
- Read MEMORY.md (the current index)
- Skim existing topic files
- Review logs/sessions/ subdirs if present

## Phase 2 — Gather Recent Signal
Priority order:
1. Daily logs (logs/YYYY/MM/YYYY-MM-DD.md)
2. Existing memories that drifted (especially project memories)
3. Transcript search (narrow grep on JSONL, `tail -50`)

## Phase 3 — Consolidate
- Write/update memory files using YAML frontmatter format
- Merge new signal into existing files rather than duplicating
- Convert relative dates → absolute dates
- Delete contradicted facts

## Phase 4 — Prune & Index
- Update MEMORY.md (keep under 200 lines / ~25KB)
- Index format: `- [Title](file.md) — one-line hook` (each <150 chars)
- Remove stale / wrong / superseded pointers
- Demote verbose entries (>200 chars = content belongs in topic file)
- Resolve contradictions
- Ensure no sensitive data in team/ memories
