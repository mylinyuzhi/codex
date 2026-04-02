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
| `GrepTool` | `tools/GrepTool/` | `{ pattern: String, path: Option<String>, output_mode: Option<GrepOutputMode> }` | Safe |
| `NotebookEditTool` | `tools/NotebookEditTool/` | `{ notebook_path: String, cell_index: i32, ... }` | **Unsafe** |

### Web Tools

| Tool | Input | Concurrency |
|------|-------|-------------|
| `WebFetchTool` | `{ url: String }` | Safe |
| `WebSearchTool` | `{ query: String }` | Safe |

### Agent & Task Tools

| Tool | Input | Concurrency |
|------|-------|-------------|
| `AgentTool` | `{ prompt, description, subagent_type?, run_in_background?, isolation?, model?, name? }` | Safe |
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
| `TaskUpdateTool` | `{ task_id: String, status: Option<TaskStatus>, ... }` |
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
| `ConfigTool` | `{ action: ConfigAction }` |
| `BriefTool` | `{ message: String }` |
| `LSPTool` | `{ action: LspAction, path: Option<String>, query: Option<String> }` |

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

## Tool Input Enums

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrepOutputMode { Content, FilesWithMatches, Count }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigAction { Get, Set, List, Reset }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LspAction { Definition, References, Diagnostics, Symbols, Hover }
```

## AgentTool Architecture (from `tools/AgentTool/` — 15 files, 3.8K LOC)

Agents ARE tasks — they register as `LocalAgentTaskState` in AppState.tasks.
AgentTool is the largest tool by complexity (spawn routing, isolation, fork).

### Agent Definition Loading

```rust
/// Agent definitions loaded from multiple sources:
/// 1. Built-in agents (Explore, Plan, Review, StatusLine, ClaudeCodeGuide, etc.)
/// 2. Custom agents from ~/.claude/agents/*.md, .claude/agents/*.md
/// 3. Plugin agents (via PluginContributions)
///
/// Each agent has: name, tools (allowlist/blocklist), max_turns, model,
/// identity prompt, permission_mode, isolation, background flag.
///
/// TS source: tools/AgentTool/loadAgentsDir.ts (755 LOC)
pub struct AgentDefinition {
    pub agent_type: AgentTypeId,           // Builtin(SubagentType) or Custom(String)
    pub tools: Option<Vec<String>>,        // Allowlist (glob: "Bash(git *)")
    pub disallowed_tools: Option<Vec<String>>, // Blocklist
    pub max_turns: Option<i32>,
    pub model: Option<String>,             // "inherit" = parent's model
    pub identity: Option<String>,          // System prompt override
    pub permission_mode: Option<PermissionMode>, // "bubble" for forks
    pub isolation: Option<IsolationMode>,   // None, Worktree, Remote
    pub background: bool,                  // Always background
}

pub enum IsolationMode { Worktree, Remote }
```

### Spawn Decision (from `AgentTool.tsx:548-567`)

```rust
/// Agent spawn routing:
///   Fork path: subagent_type omitted + fork experiment enabled → FORK_AGENT
///   Normal path: subagent_type specified → lookup AgentDefinition
///   Teammate path: name + team_name → spawnTeammate()
///
/// Background decision (shouldRunAsync):
///   run_in_background=true OR agent.background=true
///   OR coordinator mode active OR fork subagent enabled
///
/// Foreground: synchronous, holds turn, race loop (agent vs background signal)
/// Background: fire-and-forget, returns immediately with {agentId, outputFile}
```

### Fork Isolation (from `forkSubagent.ts` 211 LOC, `forkedAgent.ts` 690 LOC)

```rust
/// Fork subagent: implicit agent (no subagent_type) that inherits parent's
/// full conversation context for byte-identical prompt cache sharing.
///
/// FORK_AGENT definition:
///   tools: ["*"] (all parent's tools)
///   max_turns: 200
///   model: "inherit"
///   permission_mode: "bubble" (prompts surface to parent terminal)
///   use_exact_tools: true (byte-exact tool defs for cache hits)
///
/// Message construction (buildForkedMessages):
///   1. Clone parent's full assistant message (tool_use blocks, thinking, text)
///   2. Build tool_result blocks with identical placeholder text
///   3. Return: [parent_assistant_msg, user(results + fork_directive)]
///   Key: Byte-identical placeholders enable prompt cache sharing across forks.
///
/// Guard: Fork not available inside fork child (recursive fork prevention).
///
/// TS source: tools/AgentTool/forkSubagent.ts
pub fn build_forked_messages(
    directive: &str,
    assistant_message: &AssistantMessage,
) -> Vec<Message>;
```

### Worktree Isolation (from `utils/worktree.ts` 1519 LOC)

```rust
/// Filesystem isolation via git worktree:
/// 1. git worktree add (isolated branch)
/// 2. Symlink large dirs (node_modules) to parent repo
/// 3. Agent runs inside runWithCwdOverride(worktree_path)
/// 4. On completion: check hasWorktreeChanges()
///    - No changes → remove worktree + branch (cleanup)
///    - Changes → keep worktree, return path in notification
pub struct WorktreeInfo {
    pub worktree_path: PathBuf,
    pub worktree_branch: String,
    pub head_commit: String,
    pub git_root: PathBuf,
}
```

### Tool Filtering

```rust
/// Per-agent tool filtering:
/// - tools: ["Read", "Bash(git *)"] → allowlist (glob patterns on tool name + args)
/// - disallowed_tools: ["Write"] → blocklist
/// - MCP server filtering per agent
/// Allowlist takes precedence if both specified.
/// TS source: tools/AgentTool/agentToolUtils.ts
```

### Agent Lifecycle

```
AgentTool.call()
  → Route: fork / normal / teammate / remote
  → Setup: worktree? + tool pool + system prompt
  → Register: async (isBackgrounded=true) or foreground (isBackgrounded=false)
  ├─ Background: void runAsyncAgentLifecycle() → immediate return {agentId}
  └─ Foreground: race loop (agent turn vs backgroundPromise)
       → Auto-background after timeout (2s default)
  → Agent query loop (runAgent.ts 10.5K LOC → reuses coco-query)
  → Completion: enqueue <task-notification> XML → main agent re-enters
  → Cleanup: worktree removal if no changes
```

---

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
