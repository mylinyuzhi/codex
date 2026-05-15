# coco-tools

43 static built-in tools plus the dynamic `McpTool` wrapper. Each implements `coco_tool_runtime::Tool`; `coco-tool-runtime` defines the trait.

## TS Source
`tools/` (40 tool directories — one per tool, plus `shared/` and `testing/`). Notable:
- **File I/O**: `FileReadTool/`, `FileWriteTool/`, `FileEditTool/`, `GlobTool/`, `GrepTool/`, `NotebookEditTool/`, `BashTool/`, `ApplyPatchTool/`
- **Web**: `WebFetchTool/`, `WebSearchTool/`
- **Agent / Team**: `AgentTool/`, `SkillTool/`, `SendMessageTool/`, `TeamCreateTool/`, `TeamDeleteTool/`
- **Task**: `TaskCreateTool/`, `TaskGetTool/`, `TaskListTool/`, `TaskUpdateTool/`, `TaskStopTool/`, `TaskOutputTool/`, `TodoWriteTool/`
- **Plan / Worktree**: `EnterPlanModeTool/`, `ExitPlanModeTool/`, `VerifyPlanExecutionTool/`, `EnterWorktreeTool/`, `ExitWorktreeTool/`
- **Utility**: `AskUserQuestionTool/`, `ToolSearchTool/`, `ConfigTool/`, `BriefTool/`, `LSPTool/`
- **MCP**: `MCPTool/`, `McpAuthTool/`, `ListMcpResourcesTool/`, `ReadMcpResourceTool/`
- **Scheduling**: `ScheduleCronTool/` (Rust splits into CronCreate/CronDelete/CronList), `RemoteTriggerTool/`
- **Shell variants**: `PowerShellTool/`, `REPLTool/`, `SleepTool/`, `SyntheticOutputTool/`

Also: `tools/shared/`, `tools/utils.ts`, and supporting utils (`utils/worktree.ts`, `utils/editor.ts`, `utils/glob.ts`, `utils/path.ts`, `utils/platform.ts`, `utils/fsOperations.ts`, `utils/dxt/`).

## Key Types

- `register_all_tools(&mut ToolRegistry)` — registers all 43 static tools
- `register_core_tools(&mut ToolRegistry)` — Bash/Read/Write/Edit/Glob/Grep only (lightweight)
- `register_mcp_tools(registry, server_name, tools)` — dynamic registration after MCP server connects (idempotent, deregisters prior tools from the same server first)
- `deregister_mcp_server(registry, server_name)` — on disconnect
- Tool input enums (owned here): `GrepOutputMode`, `ConfigAction`, `LspAction`
- Per-tool structs (`BashTool`, `ReadTool`, `WriteTool`, `EditTool`, `GlobTool`, `GrepTool`, `NotebookEditTool`, `WebFetchTool`, `WebSearchTool`, `AgentTool`, `SkillTool`, `SendMessageTool`, `TeamCreateTool`, `TeamDeleteTool`, `TaskCreateTool`, `TaskGetTool`, `TaskListTool`, `TaskUpdateTool`, `TaskStopTool`, `TaskOutputTool`, `TodoWriteTool`, `EnterPlanModeTool`, `ExitPlanModeTool`, `VerifyPlanExecutionTool`, `EnterWorktreeTool`, `ExitWorktreeTool`, `AskUserQuestionTool`, `ToolSearchTool`, `ConfigTool`, `BriefTool`, `LspTool`, `McpAuthTool`, `ListMcpResourcesTool`, `ReadMcpResourceTool`, `CronCreateTool`, `CronDeleteTool`, `CronListTool`, `RemoteTriggerTool`, `PowerShellTool`, `ReplTool`, `SleepTool`, `SyntheticOutputTool`, `McpTool`)

## Cross-Cutting Helpers (crate-private)

- `record_file_read` / `record_file_edit` — updates `FileReadState` for @mention dedup + Read-tool `file_unchanged` detection
- `check_team_mem_secret` — blocks writes containing secrets into team-memory paths (layered detection: authoritative via `coco-memory::team_paths` + substring fallback, gated by `coco-secret-redact`)
- `track_nested_memory_attachment` — pushes read paths into `ctx.nested_memory_attachment_triggers` for next-turn CLAUDE.md loading
- `track_skill_discovery` — discovers `.claude/skills` in file ancestry, pushes to `ctx.dynamic_skill_dir_triggers`
- `track_file_edit` — records edits in `FileHistoryState` for checkpoint/rewind

## Architecture

- MCPTool is the only dynamic tool — schema comes from the connected server at runtime. Re-connection is idempotent: the registry deregisters prior tools for that server first.
- All file-mutation tools (Edit/Write/NotebookEdit/Bash) invoke the team-mem secret guard + file-history tracking helpers before touching disk.
- One file per tool. Utility tools live in their own modules: `ask_user_question.rs`, `tool_search.rs`, `config.rs`, `brief.rs`, `lsp_tool.rs`, `notebook_edit.rs`. (`lsp_tool.rs` is suffixed because `lsp.rs` holds the shared DTOs + formatters that the tool consumes.)

### LSP tool — TS-mirror dispatch

`LspAction` (9 variants: `goToDefinition` / `findReferences` / `hover` /
`documentSymbol` / `workspaceSymbol` / `goToImplementation` /
`prepareCallHierarchy` / `incomingCalls` / `outgoingCalls`) mirrors TS
`tools/LSPTool/schemas.ts` exactly. Wire format is **camelCase** so the
model's tool calls validate identically across runtimes. Diagnostics are
**not** an `LspAction` — they flow through the passive `system_reminder`
pipeline (`coco-lsp::DiagnosticsStore` → `app/query::reminder_adapters`)
exactly like TS `passiveFeedback.ts`.

`LspTool::is_enabled` is double-gated: `Feature::Lsp` enabled **and**
`ctx.lsp.is_connected()` (adapter reports running state after
bootstrap prewarm). Without either gate the tool is filtered out of
the model's tool list.

Dispatch flow:
1. `LspTool::execute` parses input + resolves relative paths against
   `ctx.cwd_override` (worktree-aware) → fall back to process cwd.
2. `validate_lsp_file` rejects UNC paths (`\\…` / `//…`) for Windows
   NTLM safety (TS parity) and files larger than 10MB.
3. `build_params(action, uri, line, character)` produces 0-based LSP
   `Position` from 1-based input.
4. `ctx.lsp.send_request(path, method, params)` → adapter
   (`coco_cli::lsp_handle_adapter::LspManagerAdapter`) routes via
   `LspServerManager::get_client(path)` which walks up to find
   `.git` / `Cargo.toml` — auto-routing per worktree.
5. For `incomingCalls` / `outgoingCalls`, dispatch runs the TS
   two-step pattern: `prepareCallHierarchy` → pick first item →
   `callHierarchy/{incomingCalls,outgoingCalls}`.
6. Location-returning ops (`goToDefinition` / `findReferences` /
   `goToImplementation` / `workspaceSymbol`) are filtered through
   `coco_file_ignore::PathChecker` — TS uses `git check-ignore`
   subprocess; coco-rs uses the in-process unified path (see
   user memory `feedback_unified_ignore_service`).
7. Typed formatters in `tools::lsp::format_*` produce the
   markdown-ish `LspOutput` returned to the model.

`Write` / `Edit` / `NotebookEdit` / `ApplyPatch` all call
`ctx.lsp.notify_save(path)` after a successful write — TS parity
(`FileWriteTool.ts` etc.). The adapter forwards to `client.notify_save`
(sends `textDocument/didSave` only if the file is already in the
server's `opened` tracker) AND clears the file's entries from
`DiagnosticsStore.delivered_for_file` so re-published diagnostics for
the edited file are not suppressed by cross-turn dedup.

## Per-tool Result Persistence Thresholds

`Tool::max_result_size_chars()` overrides — TS-aligned, read by the executor
(`core/tool-runtime/src/execution.rs`) per Level 1 of the
[Tool Result Budget plan](../../../docs/coco-rs/tool-result-budget-plan.md):

| Tool | Value | TS source | Note |
|---|---|---|---|
| BashTool | 30_000 | `BashTool.tsx:424` | bursty shell output |
| PowerShellTool | 30_000 | `PowerShellTool.tsx:275` | same as Bash |
| GrepTool | 20_000 | `GrepTool.ts` | match dumps grow superlinearly |
| GlobTool | 100_000 | `GlobTool.ts` | path lists tolerate larger windows |
| FileReadTool | trait default `100_000` ⚠️ | TS `Infinity` (opt-out) | Rust cannot express `Infinity` with `i32`; Phase 1.B of the plan migrates to `ResultSizeBound::{Chars,Unbounded}` |
| (other tools) | trait default `100_000` | trait default | inherit |

`bash.rs::maybe_persist_oversized_output` is a stub of Level 1 (Bash-only,
`temp_dir()` storage, parallel JSON fields instead of `<persisted-output>`
content replacement). It will be replaced by delegation to the generic
`coco-tool-runtime::tool_result_storage::maybe_persist_large_tool_result`
in Phase 1.E.

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
