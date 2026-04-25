# coco-tools

42 built-in tool implementations (41 static + dynamic `McpTool` wrapper). Each implements `coco_tool_runtime::Tool`; `coco-tool-runtime` defines the trait.

## TS Source
`tools/` (40 tool directories — one per tool, plus `shared/` and `testing/`). Notable:
- **File I/O**: `FileReadTool/`, `FileWriteTool/`, `FileEditTool/`, `GlobTool/`, `GrepTool/`, `NotebookEditTool/`, `BashTool/`
- **Web**: `WebFetchTool/`, `WebSearchTool/`
- **Agent / Team**: `AgentTool/`, `SkillTool/`, `SendMessageTool/`, `TeamCreateTool/`, `TeamDeleteTool/`
- **Task**: `TaskCreateTool/`, `TaskGetTool/`, `TaskListTool/`, `TaskUpdateTool/`, `TaskStopTool/`, `TaskOutputTool/`, `TodoWriteTool/`
- **Plan / Worktree**: `EnterPlanModeTool/`, `ExitPlanModeTool/`, `EnterWorktreeTool/`, `ExitWorktreeTool/`
- **Utility**: `AskUserQuestionTool/`, `ToolSearchTool/`, `ConfigTool/`, `BriefTool/`, `LSPTool/`
- **MCP**: `MCPTool/`, `McpAuthTool/`, `ListMcpResourcesTool/`, `ReadMcpResourceTool/`
- **Scheduling**: `ScheduleCronTool/` (Rust splits into CronCreate/CronDelete/CronList), `RemoteTriggerTool/`
- **Shell variants**: `PowerShellTool/`, `REPLTool/`, `SleepTool/`, `SyntheticOutputTool/`

Also: `tools/shared/`, `tools/utils.ts`, and supporting utils (`utils/worktree.ts`, `utils/editor.ts`, `utils/glob.ts`, `utils/path.ts`, `utils/platform.ts`, `utils/fsOperations.ts`, `utils/dxt/`).

## Key Types

- `register_all_tools(&mut ToolRegistry)` — registers all 41 static tools
- `register_core_tools(&mut ToolRegistry)` — Bash/Read/Write/Edit/Glob/Grep only (lightweight)
- `register_mcp_tools(registry, server_name, tools)` — dynamic registration after MCP server connects (idempotent, deregisters prior tools from the same server first)
- `deregister_mcp_server(registry, server_name)` — on disconnect
- Tool input enums (owned here): `GrepOutputMode`, `ConfigAction`, `LspAction`
- Per-tool structs (`BashTool`, `ReadTool`, `WriteTool`, `EditTool`, `GlobTool`, `GrepTool`, `NotebookEditTool`, `WebFetchTool`, `WebSearchTool`, `AgentTool`, `SkillTool`, `SendMessageTool`, `TeamCreateTool`, `TeamDeleteTool`, `TaskCreateTool`, `TaskGetTool`, `TaskListTool`, `TaskUpdateTool`, `TaskStopTool`, `TaskOutputTool`, `TodoWriteTool`, `EnterPlanModeTool`, `ExitPlanModeTool`, `EnterWorktreeTool`, `ExitWorktreeTool`, `AskUserQuestionTool`, `ToolSearchTool`, `ConfigTool`, `BriefTool`, `LspTool`, `McpAuthTool`, `ListMcpResourcesTool`, `ReadMcpResourceTool`, `CronCreateTool`, `CronDeleteTool`, `CronListTool`, `RemoteTriggerTool`, `PowerShellTool`, `ReplTool`, `SleepTool`, `SyntheticOutputTool`, `McpTool`)

## Cross-Cutting Helpers (crate-private)

- `record_file_read` / `record_file_edit` — updates `FileReadState` for @mention dedup + Read-tool `file_unchanged` detection
- `check_team_mem_secret` — blocks writes containing secrets into team-memory paths (layered detection: authoritative via `coco-memory::team_paths` + substring fallback, gated by `coco-secret-redact`)
- `track_nested_memory_attachment` — pushes read paths into `ctx.nested_memory_attachment_triggers` for next-turn CLAUDE.md loading
- `track_skill_discovery` — discovers `.claude/skills` in file ancestry, pushes to `ctx.dynamic_skill_dir_triggers`
- `track_file_edit` — records edits in `FileHistoryState` for checkpoint/rewind

## Architecture

- MCPTool is the only dynamic tool — schema comes from the connected server at runtime. Re-connection is idempotent: the registry deregisters prior tools for that server first.
- All file-mutation tools (Edit/Write/NotebookEdit/Bash) invoke the team-mem secret guard + file-history tracking helpers before touching disk.
- `AskUserQuestionTool` lives in `ask_user_question.rs` — own module so the full TS prompt (plan-mode guidance + `"Other"` option + preview-feature markdown) stays co-located. Other utility tools (`ToolSearch`/`Config`/`Brief`/`Lsp`/`NotebookEdit`) share `utility.rs`.

## Divergences from TS

### WebSearchTool — client-side backends instead of Anthropic server tool

TS `WebSearchTool.ts:76-84,254-291` routes search as a passthrough to the
Anthropic-only `web_search_20250305` server tool: the query is handed to
Claude, which runs it on Anthropic infrastructure and returns
`server_tool_use` + `web_search_tool_result` content blocks with inline
citations. This is Anthropic-specific — no other provider exposes an
equivalent.

coco-rs must work against every provider (Anthropic, OpenAI, Google,
DeepSeek, xAI, …), so we implement search **client-side** with a
pluggable backend selected via `WebSearchConfig.provider`
(`common/config/src/sections.rs:590-629`). Implementation mirrors
`cocode-rs/core/tools/src/builtin/web_search.rs`:

- **DuckDuckGo HTML scraping** (default) — no API key, no sign-up. POSTs
  to `html.duckduckgo.com/html/`, regex-parses result anchors + snippets,
  decodes the `uddg=` redirect back to the target URL.
- **Tavily REST API** — opt-in via `WebSearchConfig.provider = "tavily"`
  + `api_key` (or `TAVILY_API_KEY` env). Returns structured JSON so no
  HTML scraping required.
- **OpenAI** variant currently falls back to DuckDuckGo (no native
  passthrough implemented — present for future expansion).

Trade-offs vs TS passthrough:

| Aspect | TS native tool | coco-rs client-side |
|--------|----------------|---------------------|
| Citations in reply | Server-injected `citations` blocks | Model builds its own `Sources:` section (prompt requires it) |
| Streaming progress | `server_tool_use` + `web_search_tool_result` deltas | Single blocking fetch |
| Rate limits | Anthropic's (`max_uses: 8` per turn) | Per-backend (DuckDuckGo scraping limits, Tavily plan quota) |
| Domain filters | Server-side, provider-enforced | Client-side, post-fetch host-suffix match |
| Geographic availability | US-only per Anthropic | Anywhere the backend is reachable |

### Cache keys

The search cache is keyed on `(provider, max_results, query)` — not just
`query`. A DuckDuckGo result at `max_results=5` cannot be served to a
Tavily request at `max_results=20`. Error-classification wrapping via
`WebSearchErrorType` lets the model distinguish retryable (`TIMEOUT`,
`NETWORK_ERROR`) from non-retryable (`API_KEY_MISSING`, `PARSE_ERROR`)
failures via the `[TAG] message` prefix.
