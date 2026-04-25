# coco-subagent

Pure-logic subagent rules: definition catalog, source precedence, AgentTool
prompt rendering, tool filter planning, validation diagnostics.

## TS Source

- `tools/AgentTool/loadAgentsDir.ts` — definition shape, source precedence
- `tools/AgentTool/builtInAgents.ts` + `built-in/*.ts` — built-in roster
- `tools/AgentTool/prompt.ts` — agent line / tool description format
- `tools/AgentTool/agentToolUtils.ts` — `filterToolsForAgent`
- `tools/AgentTool/constants.ts` — `ONE_SHOT_BUILTIN_AGENT_TYPES`,
  `EMPTY_AGENT_OUTPUT_MARKER`
- `permissionSetup.ts:324-325` — `Agent(...)` / `Task(...)` regex

## Key Types

| Type | Purpose |
|------|---------|
| `AgentDefinitionStore` | Loads built-ins + per-source markdown agents; exposes a snapshot |
| `AgentCatalogSnapshot` | Immutable per-turn view of active / all definitions; returned as `Arc<...>` for cheap sharing |
| `AgentLoadReport` | Diagnostics from the most recent load |
| `BuiltinAgentCatalog` | Toggle set for optional built-ins (Explore/Plan, verification, claude-code-guide, SDK disable) |
| `AgentToolPromptRenderer` | TS-parity AgentTool prompt strings |
| `AgentToolFilter` + `ToolFilterPlan` | Pure tool filter computation; `app/state` applies the plan to the child `ToolRegistry` |
| `AllowedAgentTypes` + `parse_allowed_agent_types` | Parse `Agent(...)` / `Task(...)` permission entries |
| `AgentDefinitionValidator` | Structural validation (required `name` / `description`) |
| `parse_agent_markdown` | Frontmatter → `AgentDefinition` (camelCase + snake_case keys) |

Constants: `ONE_SHOT_BUILTIN_AGENT_TYPES = ["Explore", "Plan"]` (case-sensitive),
`EMPTY_AGENT_OUTPUT_MARKER = "(Subagent completed but returned no output.)"`.

## Conventions

- **TS-canonical case is contract.** `Explore` and `Plan` are PascalCase
  everywhere — output, lookup, the one-shot set. `general-purpose`,
  `statusline-setup`, `verification`, `claude-code-guide` are kebab-case
  lowercase. Aliases like `explore` only exist on input parsing; serialization
  always emits canonical case.
- **Source precedence (later wins):** `built-in < plugin < userSettings <
  projectSettings < flagSettings < policySettings`. Same `agent_type` from a
  higher source overrides lower.
- **Snapshots are deterministic.** `AgentCatalogSnapshot` keys by canonical
  `agent_type`; iteration is alphabetical for stable prompt rendering.

## Layer Rule (DO NOT BREAK)

This crate is **pure logic**. Its own `Cargo.toml` must NOT add:

- `tokio`, `tokio-util`, `mpsc`, watcher infrastructure
- `coco-tool`, `coco-tools` — would invert the thin AgentTool boundary
- `coco-query`, `coco-state`, `coco-commands` — those consume the catalog,
  not the other way round

Filesystem access is sync `std::fs` triggered by `AgentDefinitionStore::load()`
/ `reload()`. The watcher that calls `reload()` lives in `app/state`.

**Caveat — transitive tokio:** `cargo tree -p coco-subagent` shows tokio in
the graph because `coco-types` (a required dep) depends on tokio for
`AppStateReadHandle`. The crate itself uses no tokio APIs and adds none of
its own; the transitive pull is structural and predates this crate. Cleanly
removing tokio from the graph requires splitting `AppStateReadHandle` out of
`coco-types`, tracked separately. Do not add tokio APIs here in the meantime.

## Known Phase-1 Gaps (deferred to later phases)

- **No consumer wiring.** `cargo check -p coco-subagent` passes but no other
  crate imports the catalog yet — `AgentTool`, `app/state`, `commands` still
  use the legacy `agent_spawn.rs` / `agent_advanced.rs` paths. Phase 2-9
  wires the new crate.
- **Built-in `whenToUse` strings are short paraphrases**, not the verbatim
  TS strings from `built-in/*.ts`. The model-facing prompt list will read
  slightly differently from TS until the prompt renderers ship in Phase 2.
- **No nested-directory walking and no per-file size cap.** The legacy
  `agent_spawn.rs` walks two levels deep with `walkdir` and rejects files
  over 1 MiB; this crate uses one-level `read_dir` only. Add `walkdir` +
  size cap when the legacy loaders are deleted.
- **`mcpServers` inline `{ name: config }` form is not parsed** — only the
  string-reference form is read from frontmatter. TS accepts both.
- **`extra_allow_list`** on `ToolFilterContext` is a coco-rs extension (no
  TS equivalent), reserved for Phase-8 slash-command tool intersection.
  Pass `None` for TS-parity behavior.
