# coco-tools

All 40+ built-in tool implementations.

## TS Source
- `src/tools/` (43 directories -- BashTool, FileReadTool, FileWriteTool, FileEditTool, GlobTool, GrepTool, WebFetchTool, WebSearchTool, AgentTool, SkillTool, NotebookEditTool, AskUserQuestionTool, LSPTool, ConfigTool, ToolSearchTool, TaskCreate/Update/Get/List/Stop/Output, TodoWriteTool, SendMessageTool, TeamCreate/DeleteTool, McpTools, CronTool, RemoteTriggerTool, BriefTool, etc.)
- `src/utils/worktree.ts`, `src/utils/editor.ts`, `src/utils/glob.ts`
- `src/utils/path.ts`, `src/utils/platform.ts`, `src/utils/fsOperations.ts`
- `src/utils/dxt/`, `src/utils/github/`

## Key Types
Per-tool structs implementing Tool trait. Tool input enums: GrepOutputMode, ConfigAction, LspAction.
