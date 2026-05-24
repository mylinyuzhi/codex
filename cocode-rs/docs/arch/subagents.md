# Multi-Agent System Design

## Overview

The subagent system allows spawning child agents with different configurations, models, and tool access. This mirrors Claude Code's Task tool and built-in agents.

**Key Design Points:**
- Tool configuration uses `tools: Vec<String>` (empty = all) + `disallowed_tools: Vec<String>`
- **6 built-in agents**: bash, general, explore, plan, guide, statusline
- Additional agents can come from settings/plugins
- **Located in `core/subagent/`**, depends on `core/executor/` for base AgentExecutor
- Subagent lifecycle has dedicated hooks (SubagentStart, SubagentStop)

## Relationship with Executor

```
┌─────────────────────────────────────────────────────────────┐
│                      Entry Points                            │
│  Task tool    CLI --iter      /iter cmd     Collab tools    │
│      │            │               │              │          │
│      ▼            ▼               ▼              ▼          │
├──────┴────────────┴───────────────┴──────────────┴──────────┤
│                                                              │
│  ┌─────────────────┐     ┌────────────────────────────────┐ │
│  │ core/subagent   │     │        core/executor           │ │
│  │                 │     │                                │ │
│  │ SubagentManager │     │  AgentExecutor (base)          │ │
│  │ AgentDefinition │────▶│  IterativeExecutor             │ │
│  │ Context forking │     │  AgentCoordinator              │ │
│  │ Tool filtering  │     │  Collab tools                  │ │
│  └────────┬────────┘     └────────────┬───────────────────┘ │
│           │                           │                     │
│           └───────────┬───────────────┘                     │
│                       ▼                                     │
│               ┌───────────────┐                             │
│               │  core/loop    │                             │
│               │  AgentLoop    │                             │
│               └───────────────┘                             │
└─────────────────────────────────────────────────────────────┘
```

**Key distinction:**
- **Subagent** (Task tool): Inherits parent context, filters tools, spawned by main agent
- **AgentExecutor**: Independent execution, no parent context, used by iterative/collab

See [execution-modes.md](execution-modes.md) for advanced execution patterns.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    SubagentManager                          │
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │              Agent Definitions                       │   │
│  │  - Built-in (bash, general, explore, plan,          │   │
│  │    guide, statusline)                                │   │
│  │  - Custom (from settings/plugins)                    │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
│  ┌─────────────────────┐                                   │
│  │  HashMap<id, AgentInstance>                              │
│  │  (Running/Completed/Failed/Backgrounded)                │
│  └─────────────────────┘                                   │
└─────────────────────────────────────────────────────────────┘
              │
              │ spawn()
              ▼
┌─────────────────────────────────────────────────────────────┐
│                      Child AgentLoop                        │
│  - Forked context (optional, only general agent)            │
│  - Filtered tools (four-layer filtering)                    │
│  - Selected model (via ExecutionIdentity)                   │
│  - Own event channel                                        │
└─────────────────────────────────────────────────────────────┘
```

## Core Types

### AgentDefinition

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    pub name: String,
    pub description: String,
    pub agent_type: String,

    /// Allow-list of tools. Empty = all tools available.
    pub tools: Vec<String>,
    /// Deny-list of tools. Always excluded.
    pub disallowed_tools: Vec<String>,

    /// Model selection via ExecutionIdentity (Role/Inherit/Spec).
    pub identity: Option<ExecutionIdentity>,
    pub max_turns: Option<i32>,
    pub permission_mode: Option<PermissionMode>,

    /// Fork parent conversation context (only general agent uses this).
    pub fork_context: bool,
    /// Display color for TUI (e.g., "cyan", "blue").
    pub color: Option<String>,
    /// Critical reminder injected at start of agent's prompt.
    pub critical_reminder: Option<String>,
    /// Where this definition originates from.
    pub source: AgentSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AgentSource {
    #[default]
    BuiltIn,
    UserSettings,
    ProjectSettings,
    Plugin,
}
```

### Built-in Agents (6 agents)

| Agent | Tools | Disallowed | Identity | Permission | fork_context | Color | Reminder |
|-------|-------|------------|----------|------------|-------------|-------|----------|
| **bash** | `["Bash"]` | - | Inherit | - | false | - | - |
| **general** | `[]` (all) | - | Inherit | - | **true** | - | - |
| **explore** | `[]` (all) | Edit, Write, NotebookEdit | Role(Explore) | Bypass | false | cyan | READ-ONLY |
| **plan** | `[]` (all) | Edit, Write, NotebookEdit | Role(Plan) | - | false | blue | READ-ONLY |
| **guide** | Glob, Grep, Read, WebFetch, WebSearch | - | Role(Fast) | Bypass | false | green | READ-ONLY |
| **statusline** | Read, Edit | - | Role(Fast) | - | false | orange | - |

### Four-Layer Tool Filtering

```rust
/// Layer 1: System-wide blocked (always removed for all subagents)
const SYSTEM_BLOCKED: &[&str] = &[
    "Task", "EnterPlanMode", "ExitPlanMode", "TaskStop", "AskUserQuestion",
];

/// Layer 4: Tools safe for background/async execution
const ASYNC_SAFE_TOOLS: &[&str] = &[
    "Read", "Edit", "Write", "Glob", "Grep", "Bash",
    "WebFetch", "WebSearch", "NotebookEdit", "TaskOutput",
];

pub fn filter_tools_for_agent(
    all_tools: &[String],
    definition: &AgentDefinition,
    background: bool,
) -> Vec<String> {
    // Layer 1: Remove system-blocked tools
    // Layer 2: Apply allow-list (if definition.tools is non-empty)
    // Layer 3: Apply deny-list (definition.disallowed_tools)
    // Layer 4: If background, retain only ASYNC_SAFE_TOOLS
}
```

**Summary:**
1. System-wide blocked: Task, EnterPlanMode, ExitPlanMode, TaskStop, AskUserQuestion
2. Agent-specific allow-list: `tools` (empty = all tools)
3. Agent-specific deny-list: `disallowed_tools`
4. Background filter: only `ASYNC_SAFE_TOOLS` when `run_in_background=true`

### SubagentManager

```rust
pub struct SubagentManager {
    /// Registered agent definitions (builtin + custom)
    definitions: Vec<AgentDefinition>,
    /// All agent instances indexed by ID
    agents: HashMap<String, AgentInstance>,
}

pub enum AgentStatus {
    Running,
    Completed,
    Failed,
    Backgrounded,
}
```

### ChildToolUseContext

```rust
pub struct ChildToolUseContext {
    /// Unique agent identifier
    pub agent_id: String,
    /// Forked messages (if fork_context is true)
    pub messages: Vec<TrackedMessage>,
    /// Cancellation token for this agent
    pub cancel_token: CancellationToken,
}
```

### SpawnInput

```rust
pub struct SpawnInput {
    pub agent_type: String,
    pub prompt: String,
    pub identity: Option<ExecutionIdentity>,
    pub max_turns: Option<i32>,
    pub run_in_background: bool,
    pub allowed_tools: Option<Vec<String>>,
    pub resume_from: Option<String>,
}
```

## Config Overrides

Builtin agent definitions can be overridden via `~/.cocode/builtin-agents.json`:

```json
{
  "explore": {
    "max_turns": 30,
    "identity": "fast",
    "tools": ["Read", "Glob", "Grep", "Bash"],
    "fork_context": false,
    "color": "yellow",
    "critical_reminder": "Custom reminder text"
  }
}
```

Override struct:
```rust
pub struct BuiltinAgentOverride {
    pub max_turns: Option<i32>,
    pub identity: Option<String>,
    pub tools: Option<Vec<String>>,
    pub disallowed_tools: Option<Vec<String>>,
    pub fork_context: Option<bool>,
    pub color: Option<String>,
    pub critical_reminder: Option<String>,
}
```

## Resume Capability

When resuming an agent via `resume_from`, the previous agent's prompt text is prepended to the new prompt. This avoids replaying full conversation history while preserving context.

## Background Agents

See [background.md](background.md) for detailed background mode architecture.

Background agents:
- Use Layer 4 filtering (`ASYNC_SAFE_TOOLS` only)
- Write events to a log file instead of the main event channel
- Can be resumed to foreground via `resume()`
- Status tracked as `AgentStatus::Backgrounded`

## Subagent Hooks

Subagents emit dedicated hooks for lifecycle tracking:
- `SubagentStart`: emitted when an agent is spawned
- `SubagentStop`: emitted when an agent completes or is stopped

## Custom Agents from Plugins

Plugins can register agents via `PluginContribution::Agent`:

```rust
let definition = AgentDefinition {
    name: "code-reviewer".to_string(),
    description: "Review code changes".to_string(),
    agent_type: "code-reviewer".to_string(),
    tools: vec![],
    disallowed_tools: vec!["Edit".to_string(), "Write".to_string()],
    identity: None,
    max_turns: None,
    permission_mode: None,
    fork_context: true,
    color: None,
    critical_reminder: Some("Do not modify files.".to_string()),
    source: AgentSource::Plugin,
};
```
