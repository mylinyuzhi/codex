---
allowed-tools: Read, Glob, Grep, Write, Bash(ls *), Bash(wc *)
description: Regenerate CLAUDE.md from workspace discovery (zero hardcoded content)
argument-hint: [target-file-path]
---

## Context

- Target file: $ARGUMENTS (default: CLAUDE.md in project root)
- Workspace root: !`ls cocode-rs/Cargo.toml`
- Existing CLAUDE.md line count: !`wc -l CLAUDE.md`

## Goal

Regenerate CLAUDE.md by **discovering** all project structure at runtime. This command contains **zero hardcoded crate names, layer names, or architecture content** — everything is derived from the workspace.

## Principles

- **Concise but informative**: tables over prose; include key types for non-trivial crates, but cover every crate
- **No arbitrary line limit**: completeness over brevity. A 56-crate workspace needs more room than a 10-crate one
- **Progressive disclosure**: main file gives overview, links to detailed docs
- **Single source of truth**: reference AGENTS.md for conventions, don't duplicate
- **No inline code examples**: link to source instead

## Procedure

### Step 1: Discover Workspace

1. **Parse crate list**: Read `cocode-rs/Cargo.toml`, extract all paths from `[workspace] members` (resolve globs by listing matching directories)

2. **Gather crate metadata**: For each crate path, determine its name, purpose, and key types:
   - First: read the crate's `Cargo.toml` for `name` and `description` fields
   - Fallback: read `src/lib.rs` (or `src/main.rs`) first 10 lines for Rust crate-level doc comments
   - Last resort: infer purpose from the crate name itself
   - **Key types discovery** (non-Utils crates only): scan `src/lib.rs` for `pub struct`, `pub trait`, `pub enum`, and `pub use` (re-exports) to identify the primary public types. Record the most important ones (aim for 3-8 per crate) to populate the `Key Types` column in crate tables. Include brief parenthetical notes for the most important types (e.g. `ConfigManager (thread-safe, RwLock)`).

3. **Auto-derive layers**: Group crates by their first path component (e.g. `common/`, `core/`, `app/`, `provider-sdks/`, `utils/`, `exec/`, `features/`, `mcp/`). Crates at the top level of `cocode-rs/` (not in a subdirectory group) form a "Standalone" layer. Derive layer display names from the path prefix (e.g. `provider-sdks` → "Provider SDKs"). Sort layers in dependency order: Common → Provider SDKs → Core → App, with others (Features, Exec, MCP, Standalone, Utils) in between as appropriate.

4. **Find specialized docs**: Glob for `cocode-rs/**/CLAUDE.md` — these get linked in a "Specialized Documentation" table.

5. **Read dev commands**: Read `cocode-rs/justfile` to extract the key development commands (fmt, pre-commit, test, help, etc.).

6. **Scan error patterns**: For each layer, check whether crates use `snafu` or `anyhow` by grepping their `Cargo.toml` dependencies. Summarize as a table.

7. **Preserve human-authored sections**: Read the existing CLAUDE.md. Identify sections that contain human-authored design decisions, references, or notes that are NOT auto-generated crate tables. Preserve their content in the regenerated file.

8. **Discover data flows**: Read key orchestration files to identify the major request lifecycle paths through the system. Focus on:
   - The agent loop driver (in `core/loop`) — trace the multi-turn cycle from user input through system reminders, prompt building, tool definitions, API streaming, tool execution, hooks, message history, compaction, and loop events
   - The configuration resolution chain (in `common/config`) — trace from JSON files + env vars through loading, resolving, and runtime overrides to config snapshots consumed by core
   - The provider call chain (from `common/config` through `provider-sdks/hyper-sdk` to `core/api`) — trace how a provider is resolved, a model instantiated, and a streaming request made with retry/fallback
   - The shell execution flow (from tools through `exec/shell`) — trace command safety analysis, sandbox checking, shell spawning, and CWD tracking

   For each flow, produce an ASCII flow diagram showing crate names and key types at each stage. These diagrams should be **discovered from the actual code**, not hardcoded.

9. **Discover design patterns**: Scan the workspace for cross-cutting design patterns by grepping for characteristic signatures:
   - `Arc<Mutex` / `Arc<RwLock` → shared state pattern (note which crates use it heavily)
   - `CancellationToken` → cancellation pattern
   - `mpsc::Sender` / `watch::Sender` → event-driven pattern
   - Builder structs (types ending in `Builder`) → builder pattern
   - `#[async_trait]` + `pub trait` → trait abstraction pattern
   - `is_meta` / meta messages → meta message pattern
   - Callback function types (`Fn`, `Box<dyn Fn`) → callback decoupling pattern
   - Facade types → facade pattern

   Summarize as a "Key Design Patterns" table with columns: Pattern | Where | Details. Only include patterns that are actually found in the codebase.

### Step 2: Generate Architecture Diagram

Build an ASCII art layer diagram **from the discovered layers and crate names**. Do NOT use a static template. The diagram should:
- Show layers as horizontal bands, ordered by dependency (bottom = foundational)
- List crate names within each layer
- Show key dependency arrows where obvious (e.g. loop → executor → api)
- For layers with many crates (e.g. Utils), show a count instead of listing all names

### Step 3: Compose CLAUDE.md

Assemble the file in this section order:

1. **Title + one-liner** — e.g. "Multi-provider LLM SDK and CLI. All development in `cocode-rs/`."
2. **AGENTS.md reference** — "Read `AGENTS.md` for Rust conventions."
3. **Commands** — from justfile discovery (Step 1.5)
4. **Architecture** — generated diagram (Step 2)
5. **Key Data Flows** — ASCII flow diagrams for agent turn lifecycle, configuration resolution, provider call chain, and shell execution flow (Step 1.8)
6. **Crate Guide** — one table per layer. For non-Utils layers, use columns: `Crate | Purpose | Key Types` (from Step 1.2). For Utils layer, use columns: `Crate | Purpose` (too many small crates for type enumeration). Include layer crate count in heading.
7. **Key Design Patterns** — table from Step 1.9 with columns: Pattern | Where | Details
8. **Error Handling** — from error pattern scan (Step 1.6)
9. **Specialized Documentation** — table linking all discovered CLAUDE.md files (Step 1.4)
10. **Preserved sections** — Design Decisions, References, or any other human-authored sections from Step 1.7
11. **References** — links to AGENTS.md, error docs, user docs

### Step 4: Write and Verify

Write the file, then verify:

1. **Crate coverage**: count of crates in generated file == count of workspace members (exact match). List any missing crates as errors.
2. **Link check**: every file path referenced in the CLAUDE.md actually exists on disk.
3. **CLAUDE.md coverage**: every discovered `**/CLAUDE.md` file is linked in the Specialized Documentation table.
4. **Data flow coverage**: at least 3 data flow diagrams are present in the Key Data Flows section.
5. **Key types coverage**: every non-Utils crate table has a `Key Types` column with entries for each crate.

If any check fails, fix the issue and re-verify before proceeding.

### Step 5: Report

Output a summary:

```
## CLAUDE.md Regeneration Complete

| Metric | Before | After |
|--------|--------|-------|
| Line count | X | Y |
| Crates documented | X/N | Y/N |
| Specialized docs linked | X | Y |

### Verification
- [ ] All workspace crates documented (exact match)
- [ ] All file links valid
- [ ] All CLAUDE.md files linked
- [ ] At least 3 data flow diagrams present
- [ ] Non-Utils crate tables include Key Types column
```

## Important Rules

- Do NOT hardcode any crate names, layer names, or architecture content in this command
- Do NOT duplicate content from AGENTS.md
- Do NOT include inline code examples (link to source files)
- Do NOT use prose for crate descriptions (use tables; concise but informative entries with key types for non-Utils crates)
- Do NOT skip any workspace crate (complete coverage is mandatory)
- Do NOT impose an arbitrary line limit — let completeness drive length
- DO preserve human-authored sections (Design Decisions, etc.) from the existing file
