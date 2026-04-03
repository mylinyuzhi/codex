# coco-hooks — Crate Plan

TS source: `src/schemas/hooks.ts`, `src/utils/hooks/` (15+ files, 3.7K LOC)

## Dependencies

```
coco-hooks depends on:
  - coco-types (HookEventType, HookResult), coco-config (Settings — hooks section as Value)
  - coco-tool (ToolUseContext — for hook execution context)
  - tokio, reqwest (HTTP hooks)

coco-hooks does NOT depend on:
  - coco-tools, coco-query, coco-inference, any app/ crate
```

## Data Definitions

```rust
/// Maps hook event types to their matcher arrays.
/// TS uses `Partial<Record<HookEvent, HookMatcher[]>>` — a sparse map.
/// In Rust: HashMap keyed by HookEventType. All 27 event types are valid keys.
/// Missing entries mean no hooks configured for that event.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HooksSettings {
    #[serde(flatten)]
    pub hooks: HashMap<HookEventType, Vec<HookMatcher>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookMatcher {
    /// Tool name pattern for PreToolUse/PostToolUse. e.g. "Write", "Bash(git *)"
    #[serde(rename = "if", skip_serializing_if = "Option::is_none")]
    pub matcher: Option<String>,
    pub hooks: Vec<HookCommand>,
}

/// TS discriminates via `type` field: "command", "prompt", "http", "agent".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HookCommand {
    Command { command: String, shell: Option<ShellKind>, timeout: Option<i64>, once: bool, r#async: bool },
    Prompt { prompt: String, model: Option<String>, timeout: Option<i64>, once: bool },
    Http { url: String, headers: HashMap<String, String>, timeout: Option<i64>, once: bool },
    Agent { prompt: String, model: Option<String>, timeout: Option<i64>, once: bool },
}
```

## Core Logic

```rust
pub struct HookExecutor;

impl HookExecutor {
    /// Execute hooks matching an event.
    /// 3-phase pipeline:
    ///   1. Parse input: evaluate `if` condition (permission rule syntax), match tool name
    ///   2. Run hook command: bash/prompt/http/agent (parallel if async=true)
    ///   3. Process response: continue, block, modify input, permission decision
    ///
    /// Hook responses can:
    ///   - Allow/deny tool execution (for pre_tool_use)
    ///   - Modify tool input (for pre_tool_use)
    ///   - Inject messages into conversation
    ///   - Return structured permission decisions
    pub async fn run_hooks(
        event: HookEventType,
        tool_id: Option<&ToolId>,
        input: &Value,
        context: &ToolUseContext,
        settings: &HooksSettings,
    ) -> Vec<HookResult>;
}

/// Async hook tracking: hooks with async=true run in background,
/// polled for completion at configurable intervals.
/// Timeout: per-hook timeout field (default: 60s).
/// Once: hooks with once=true run only on first match per session.
pub struct AsyncHookRegistry {
    pending: HashMap<String, AsyncHookState>,
}

impl AsyncHookRegistry {
    pub fn register(&mut self, hook_id: &str, timeout: Duration);
    pub fn poll(&mut self) -> Vec<HookResult>;
    pub fn cancel(&mut self, hook_id: &str);
}

/// Hook scope priority (highest wins):
///   Skill > Plugin > Project (.claude/settings.json) > User (~/.claude/settings.json) > Global
pub enum HookScope { Skill, Plugin, Project, User, Global }
```
