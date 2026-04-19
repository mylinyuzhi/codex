# coco-tools

42 built-in tool implementations (41 static + dynamic `McpTool` wrapper). Each implements `coco_tool::Tool`; `coco-tool` defines the trait.

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
