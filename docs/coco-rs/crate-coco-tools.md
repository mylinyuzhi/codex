# coco-tools — Crate Plan

TS source: `src/tools/` (40 tool directories + `shared/` + `testing/`)

Each tool implements the `Tool` trait from `coco-tool`.

## Dependencies

```
coco-tools depends on:
  - coco-tool        (Tool trait, ToolUseContext, ToolError, ToolResult, ToolRegistry)
  - coco-types       (Message, PermissionDecision, SandboxMode, TaskId, AgentId)
  - coco-shell       (ShellExecutor — for BashTool)
  - coco-mcp         (McpClient — for MCP tools)
  - coco-lsp         (LspClient — for LSPTool)
  - coco-permissions (permission evaluation — for BashTool, FileWriteTool, etc.)
  - coco-config      (Settings — for ConfigTool)
  - coco-error
  - tokio, tokio-util, serde_json

coco-tools does NOT depend on:
  - coco-commands    (no command knowledge — SkillTool uses callback)
  - coco-skills      (no skill knowledge — SkillTool uses callback)
  - coco-tasks       (no task manager — TaskTools use callback)
  - coco-query       (no query engine)
  - coco-inference   (no direct LLM calls — AgentTool spawns via callback)
  - any app/ crate

Circular dependency prevention:
  SkillTool, TaskCreateTool, AgentTool etc. use callback closures
  injected via ToolUseContext at runtime by coco-cli/coco-query.
```

## Tool Inventory

### File I/O Tools

| Tool | TS source | Input | Concurrency |
|------|-----------|-------|-------------|
| `BashTool` | `tools/BashTool/` (20 files, largest tool) | `{ command: String, timeout_ms: Option<i64> }` | **Unsafe** |
| `FileReadTool` | `tools/FileReadTool/` | `{ file_path: String, offset: Option<i64>, limit: Option<i64> }` | Safe |
| `FileWriteTool` | `tools/FileWriteTool/` | `{ file_path: String, content: String }` | **Unsafe** |
| `FileEditTool` | `tools/FileEditTool/` | `{ file_path: String, old_string: String, new_string: String }` | **Unsafe** |
| `GlobTool` | `tools/GlobTool/` | `{ pattern: String, path: Option<String> }` | Safe |
| `GrepTool` | `tools/GrepTool/` | `{ pattern: String, path: Option<String>, output_mode: Option<String> }` | Safe |
| `NotebookEditTool` | `tools/NotebookEditTool/` | `{ notebook_path: String, cell_index: i32, ... }` | **Unsafe** |

### Web Tools

| Tool | Input | Concurrency |
|------|-------|-------------|
| `WebFetchTool` | `{ url: String }` | Safe |
| `WebSearchTool` | `{ query: String }` | Safe |

### Agent & Task Tools

| Tool | Input | Concurrency |
|------|-------|-------------|
| `AgentTool` | `{ prompt: String, description: String, subagent_type: Option<String> }` | Safe |
| `SkillTool` | `{ skill: String, args: Option<String> }` | Safe |
| `SendMessageTool` | `{ to: String, message: String }` | Safe |
| `TeamCreateTool` | `{ name: String, members: Vec<TeamMember> }` | **Unsafe** |
| `TeamDeleteTool` | `{ name: String }` | **Unsafe** |

### Task Management Tools

| Tool | Input |
|------|-------|
| `TaskCreateTool` | `{ subject: String, description: String }` |
| `TaskGetTool` | `{ task_id: String }` |
| `TaskListTool` | `{}` |
| `TaskUpdateTool` | `{ task_id: String, status: Option<String>, ... }` |
| `TaskStopTool` | `{ task_id: String }` |
| `TaskOutputTool` | `{ task_id: String }` |
| `TodoWriteTool` | `{ todos: Vec<TodoItem> }` |

### Plan & Worktree Tools

| Tool | Input |
|------|-------|
| `EnterPlanModeTool` | `{}` |
| `ExitPlanModeTool` | `{ allowed_prompts: Option<Vec<AllowedPrompt>> }` |
| `EnterWorktreeTool` | `{}` |
| `ExitWorktreeTool` | `{}` |

### Utility Tools

| Tool | Input |
|------|-------|
| `AskUserQuestionTool` | `{ question: String }` |
| `ToolSearchTool` | `{ query: String, max_results: Option<i32> }` |
| `ConfigTool` | `{ action: String }` |
| `BriefTool` | `{ message: String }` |
| `LSPTool` | `{ action: String, path: Option<String>, query: Option<String> }` |

### MCP Tools

| Tool | TS source | Input | Concurrency |
|------|-----------|-------|-------------|
| `MCPTool` | `tools/MCPTool/` | `{ [dynamic]: Value }` (passthrough schema, MCP tools define their own) | Safe |
| `McpAuthTool` | `tools/McpAuthTool/` | `{}` (triggers OAuth flow for MCP server) | **Unsafe** |
| `ListMcpResourcesTool` | `tools/ListMcpResourcesTool/` | `{ server_name: Option<String> }` | Safe |
| `ReadMcpResourceTool` | `tools/ReadMcpResourceTool/` | `{ server_name: String, uri: String }` | Safe |

### Scheduling Tools

| Tool | TS source | Input | Concurrency |
|------|-----------|-------|-------------|
| `CronCreateTool` | `tools/ScheduleCronTool/CronCreateTool.ts` | `{ cron: String, prompt: String, max_age_days: Option<i64> }` | **Unsafe** |
| `CronDeleteTool` | `tools/ScheduleCronTool/CronDeleteTool.ts` | `{ cron_id: String }` | **Unsafe** |
| `CronListTool` | `tools/ScheduleCronTool/CronListTool.ts` | `{}` | Safe |
| `RemoteTriggerTool` | `tools/RemoteTriggerTool/` | `{ trigger_id: String }` | Safe |

### Shell Tools

| Tool | TS source | Input | Concurrency |
|------|-----------|-------|-------------|
| `PowerShellTool` | `tools/PowerShellTool/` (CLM security analysis, command semantics) | `{ command: String, timeout_ms: Option<i64> }` | **Unsafe** |
| `REPLTool` | `tools/REPLTool/` (wraps primitives: Bash+Read+Write+Edit+Glob+Grep+Agent) | `{ command: String }` | **Unsafe** |

### Internal/SDK Tools

| Tool | TS source | Input | Concurrency |
|------|-----------|-------|-------------|
| `SleepTool` | `tools/SleepTool/` | `{ duration_ms: i64 }` | Safe |
| `SyntheticOutputTool` | `tools/SyntheticOutputTool/` (SDK-only, structured output via dynamic JSON schema) | `{ [dynamic]: Value }` | Safe |

## Implementation Pattern

Each tool follows:

```rust
pub struct BashTool;

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str { "Bash" }
    fn aliases(&self) -> &[&str] { &["bash", "sh", "zsh"] }

    async fn description(&self, _input: &Value, _opts: &DescriptionOptions) -> String {
        include_str!("bash/prompt.md").to_string()
    }

    fn input_schema(&self) -> &ToolInputSchema { &BASH_SCHEMA }

    fn is_concurrency_safe(&self, _input: &Value) -> bool { false }
    fn is_read_only(&self, input: &Value) -> bool {
        // delegate to coco-shell read-only validation
        false
    }

    async fn check_permissions(&self, input: &Value, ctx: &ToolUseContext) -> PermissionDecision {
        // delegate to coco-permissions + coco-shell security analysis
        PermissionDecision::Ask { message: "Run command?".into(), .. }
    }

    async fn execute(
        &self, input: Value, ctx: &ToolUseContext, cancel: CancellationToken,
    ) -> Result<ToolResult<Value>, ToolError> {
        let command = input["command"].as_str().unwrap();
        let result = ctx.shell_executor.exec(command, ...).await?;
        Ok(ToolResult { data: json!({ "stdout": result.stdout, ... }), .. })
    }
}
```
