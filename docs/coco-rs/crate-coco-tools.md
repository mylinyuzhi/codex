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
| `NotebookEditTool` | `tools/NotebookEditTool/` | `{ notebook_path: String, cell_id: String, new_source: String, cell_type: Option<String>, edit_mode: EditMode }` | **Unsafe** |

#### FileReadTool Details

Input (full):
```rust
pub struct FileReadInput {
    pub file_path: String,
    pub offset: Option<i64>,    // 1-based line number to start from
    pub limit: Option<i64>,     // max lines to read (default ~2000)
    pub pages: Option<String>,  // PDF page range, e.g. "1-5", "3", "10-20"
}
```

Output types (five variants based on file type):
- **text** — plain text with `cat -n` line numbering (default for most files)
- **image** — binary image file (PNG/JPG/GIF/WebP/SVG); auto-resized via sharp to fit token budget, returns base64 with dimension metadata (`width`, `height`)
- **notebook** — `.ipynb` Jupyter notebook; parsed into cells, returns all cells with outputs (code + markdown + visualization)
- **pdf** — `.pdf` document; extracts pages as base64 parts with structured error types; respects `pages` param; max 20 pages per request; large PDFs (>10 pages) require `pages` param or request fails
- **file_unchanged** — dedup optimization; if file was previously read in same turn and mtime has not changed, returns stub indicating no change (readFileState mtime check)

Behavioral details:
- Dual token limit: `maxSizeBytes` = 256 KB + `maxTokens` = 25000; env var `CLAUDE_FILE_MAX_TOKENS` overrides `maxTokens`
- Blocked device paths: `/dev/zero`, `/dev/random`, `/dev/urandom`, `/proc/*/fd/*` — rejected before read
- Binary file guard: extension-based detection (`.exe`, `.bin`, `.so`, `.dll`, `.dylib`, etc.) — returns error message, not raw bytes
- UNC path guard: rejects `\\server\share` paths on Windows to prevent NTLM credential leak
- Event hook: `registerFileReadListener` fires after successful read (used by context tracking)
- Skill discovery: `discoverSkillDirsForPaths` triggers on read to detect `.claude/` skill directories in read paths

#### FileWriteTool Details

Input:
```rust
pub struct FileWriteInput {
    pub file_path: String,
    pub content: String,
}
```

Behavioral details:
- Read-before-write enforcement: a `readFileState` entry must exist for the path (tool must have been read first, or the file must be new); returns validation error otherwise
- mtime staleness check: compares current mtime against last-read mtime; if file was modified externally since last read, rejects write with staleness error; Windows fallback: content-comparison when mtime is unreliable
- Auto-creates parent directories: `mkdir -p` equivalent before write
- Encoding preservation: detects original encoding (UTF-8/UTF-16LE) on first read, reuses same encoding on write
- Line ending: unconditional LF normalization (design decision — model's content is authoritative, no CRLF preservation)
- LSP notifications: sends `textDocument/didChange` + `textDocument/didSave` after successful write
- File history: calls `fileHistoryTrackEdit` to create backup entry for undo/rewind support
- Team memory secret guard: blocks writes that would embed secrets from team memory into files
- Settings file validation: if writing to a known settings file (`.claude/settings.json`, etc.), validates JSON structure before commit

#### FileEditTool Details

Input (full):
```rust
pub struct FileEditInput {
    pub file_path: String,
    pub old_string: String,
    pub new_string: String,
    pub replace_all: bool,     // default false; if true, replaces all occurrences
}
```

Behavioral details:
- 1 GiB max file size guard: rejects files exceeding ~1 GiB before attempting edit
- Quote normalization: `findActualString` and `preserveQuoteStyle` handle curly-to-straight quote mapping (`\u{201c}`/`\u{201d}` to `"`, `\u{2018}`/`\u{2019}` to `'`); model output often uses straight quotes while file content has curly — normalizer matches both
- `.ipynb` redirect: if file is a Jupyter notebook, redirects to `NotebookEditTool` with guidance message
- `userModified` flag: output includes boolean indicating whether the edit actually changed file content (false if old_string == new_string)
- Read-before-write enforcement: same readFileState check as FileWriteTool
- LSP + file history: same post-write notifications and backup as FileWriteTool

#### GlobTool Details

Input:
```rust
pub struct GlobInput {
    pub pattern: String,           // glob pattern, e.g. "**/*.rs", "src/**/*.ts"
    pub path: Option<String>,      // directory to search in (default: cwd)
}
```

Behavioral details:
- Results sorted by modification time (most-recent first)
- Hard cap: 100 results maximum; output includes `truncated: true` flag when limit is hit
- Paths relativized: all returned paths are relative to cwd (not absolute)

#### GrepTool Details

Input (full):
```rust
pub struct GrepInput {
    pub pattern: String,                          // regex pattern (ripgrep syntax)
    pub path: Option<String>,                     // file or directory to search (default: cwd)
    pub output_mode: Option<GrepOutputMode>,      // content | files_with_matches (default) | count
    pub glob: Option<String>,                     // file glob filter, e.g. "*.js", "*.{ts,tsx}"
    pub r#type: Option<String>,                   // ripgrep --type, e.g. "js", "py", "rust"
    #[serde(rename = "-B")]
    pub before_context: Option<i64>,              // lines before match (rg -B)
    #[serde(rename = "-A")]
    pub after_context: Option<i64>,               // lines after match (rg -A)
    #[serde(rename = "-C")]
    pub context: Option<i64>,                     // lines before+after match (rg -C)
    #[serde(rename = "-n")]
    pub line_numbers: Option<bool>,               // show line numbers (default true for content mode)
    #[serde(rename = "-i")]
    pub case_insensitive: Option<bool>,           // case-insensitive search
    pub head_limit: Option<i64>,                  // limit output to first N entries (default 250)
    pub offset: Option<i64>,                      // skip first N entries before applying head_limit
    pub multiline: Option<bool>,                  // enable multiline mode (rg -U --multiline-dotall)
}
```

Behavioral details:
- VCS directory auto-exclusion: `.git`, `.svn`, `.hg`, `.bzr`, `.jj`, `.sl` directories always excluded from search
- Max column limit: 500 characters per line; longer lines are truncated
- Permission-based ignore: injects additional ignore patterns from permission settings (e.g., project-level `.gitignore` extensions)

#### NotebookEditTool Details

Input (full):
```rust
pub struct NotebookEditInput {
    pub notebook_path: String,
    pub cell_id: String,           // cell UUID or numeric index as string
    pub new_source: String,        // new cell content
    pub cell_type: Option<String>, // "code" or "markdown" (for insert mode)
    pub edit_mode: EditMode,       // replace | insert | delete
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditMode { Replace, Insert, Delete }
```

Behavioral details:
- Cell lookup: resolves `cell_id` by UUID first; if not found, falls back to parsing as numeric index (0-based)
- Auto-generates UUIDs: new cells get a UUID assigned automatically, but only when notebook format is nbformat >= 4.5 (earlier formats do not use cell IDs)
- On replace: resets `execution_count` to `null` and clears `outputs` array (stale outputs from previous execution are invalid after source change)
- Read-before-edit enforcement: same readFileState + mtime staleness check as FileWriteTool
- LSP + file history: same post-write notifications and backup as FileWriteTool

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

### Plan Mode State Machine

TS source: `tools/EnterPlanModeTool/`, `tools/ExitPlanModeTool/`, `utils/plans.ts`, `utils/permissions/permissionSetup.ts`

**States:**
```
default | acceptEdits | bypassPermissions | dontAsk | auto
    │ EnterPlanMode (stashes current as prePlanMode)
    ▼
  plan ──► ExitPlanMode ──► prePlanMode (restored)
```

**Permission context fields during plan mode:**
```rust
pub struct PlanModeContext {
    pub pre_plan_mode: Option<PermissionMode>,  // stashed on enter, restored on exit
    pub stripped_dangerous_rules: Option<PermissionRules>,  // saved during auto→plan
    pub has_exited_plan_mode: bool,
    pub needs_plan_mode_exit_attachment: bool,
}
```

**Plan file storage:**
- Path: `~/.coco/plans/{slug}.md` (custom: `settings.plans_directory` relative to project root)
- Agent plans: `~/.coco/plans/{slug}-agent-{agent_id}.md`
- Slug: random word slug with collision avoidance (max 10 retries)
- Slug cached per session, reused on resume, forked on fork

**EnterPlanMode flow:**
1. Reject if in agent context (agents cannot enter plan mode)
2. `prepare_context_for_plan_mode()` stashes current mode as `pre_plan_mode`
3. If auto mode active + opted-in: auto stays active during plan (permissions stripped)
4. If auto mode active but not opted-in: deactivate auto, restore permissions
5. Set mode to `plan`

**ExitPlanMode flow:**
1. Validate currently in plan mode (error_code: 1 if not)
2. Read plan from disk via `get_plan(agent_id)`
3. CCR may inject edited plan via `permission_result.updated_input.plan`
4. **Teammate approval path**: if `is_plan_mode_required()`:
   - Generate `request_id` for `plan_approval`
   - Write approval request to team-lead mailbox
   - Return `{ awaiting_leader_approval: true }` (agent waits)
5. **Normal exit**: restore mode from `pre_plan_mode`
6. **Auto mode gate fallback**: if `pre_plan_mode == auto` but gate tripped → restore to `default` instead
7. Set `needs_plan_mode_exit_attachment = true` for system message

**Recovery (3-source, for session resume):**
1. Direct disk read (plan file by slug)
2. CCR file snapshot recovery (search messages for `file_snapshot` type)
3. Message history recovery (search for ExitPlanMode tool_use with `input.plan`)

**Circuit breaker:**
- Auto mode gate checked on exit via `is_auto_mode_gate_enabled()`
- If tripped mid-plan: fallback to 'default', notify user "auto mode unavailable"
- Gate sources: GrowthBook config, incident response killswitch

**Channel gating:** both tools disabled on Kairos/non-terminal channels (no approval dialog)

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

### Background Agent Progress & Output

```rust
/// Progress metadata updated during background agent execution.
pub struct AgentProgress {
    pub tool_use_count: i32,
    pub token_count: i64,
    pub last_activity: Option<ToolActivity>,
    pub recent_activities: Vec<ToolActivity>,
    pub summary: Option<String>,
}

pub struct ToolActivity {
    pub tool_name: String,
    pub input: Value,
    pub activity_description: Option<String>,
    pub is_search: bool,
    pub is_read: bool,
}

/// Output: symlink-based for efficiency.
/// init_task_output_as_symlink(task_id, agent_transcript_path) creates symlink.
/// Fallback to regular file if symlink fails.
/// Delta reads: get_task_output_delta(task_id, from_offset, max_bytes)
///   returns { content, new_offset } — only new bytes since last read.
///   Max 8MB per delta read. Handles ENOENT gracefully.
/// O_NOFOLLOW flag prevents sandbox symlink-following attacks.
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
