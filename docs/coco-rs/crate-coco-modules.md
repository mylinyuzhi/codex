# Root Module Crates — Index

These 6 crates are standalone at workspace root, matching TS's flat top-level layout.
Each has its own plan document:

| Crate | TS Source | Plan Doc |
|-------|-----------|----------|
| `coco-skills` | `src/skills/` | [`crate-coco-skills.md`](crate-coco-skills.md) |
| `coco-hooks` | `src/schemas/hooks.ts`, `src/utils/hooks/` | [`crate-coco-hooks.md`](crate-coco-hooks.md) |
| `coco-tasks` | `src/tasks/`, `src/Task.ts`, `src/utils/task/`, `src/utils/plans.ts` | [`crate-coco-tasks.md`](crate-coco-tasks.md) |
| `coco-memory` | `src/memdir/`, `src/services/extractMemories/`, `src/services/SessionMemory/` | [`crate-coco-memory.md`](crate-coco-memory.md) |
| `coco-plugins` | `src/plugins/`, `src/services/plugins/` | [`crate-coco-plugins.md`](crate-coco-plugins.md) |
| `coco-keybindings` | `src/keybindings/` | [`crate-coco-keybindings.md`](crate-coco-keybindings.md) |

None of these depend on: coco-tools, coco-query, coco-state, coco-session, coco-tui, coco-cli (app layer).
