# Subagent Trigger Mechanisms - Comprehensive Comparison

**Date**: 2025-11-11
**Author**: Analysis based on codebase exploration
**Purpose**: Development guide for subagent invocation patterns across three AI coding assistants

**Important Note**: This document compares three independent projects:
- **gemini-cli** - Google's Gemini CLI implementation
- **Kode-cli** - Multi-model AI coding assistant
- **codex** - Rust-based CLI tool (this project)

---

## Table of Contents

1. [Executive Summary](#executive-summary)
2. [Quick Comparison Table](#quick-comparison-table)
3. [gemini-cli: LLM-Driven Function Calling](#gemini-cli-llm-driven-function-calling)
4. [Kode-cli: Explicit @ Mention System](#kode-cli-explicit--mention-system)
5. [codex: Slash Command System](#codex-slash-command-system)
6. [Architecture Philosophy Comparison](#architecture-philosophy-comparison)
7. [Implementation Details](#implementation-details)
8. [Best Practices & Recommendations](#best-practices--recommendations)

---

## Executive Summary

This document analyzes three distinct approaches to subagent invocation in AI coding assistants:

- **gemini-cli**: Pure LLM-driven function calling with heuristic guidance
- **Kode-cli**: Explicit user-controlled @ mention system with event-driven injection
- **codex**: Slash command system with explicit user triggers

Each approach represents a different philosophy on the balance between user control and AI autonomy.

---

## Quick Comparison Table

| Dimension | gemini-cli | Kode-cli | codex |
|-----------|------------|----------|-------|
| **Trigger Type** | Heuristic (LLM-driven) | Explicit Command (@mention) | Explicit Command (slash) |
| **User Syntax** | None (natural language) | `@run-agent-xxx` or `@agent-xxx` | `/review`, `/compact` |
| **Certainty** | Low (depends on LLM) | High (forced trigger) | High (explicit command) |
| **Intelligence** | High (LLM autonomous) | Low (manual user) | Low (manual user) |
| **Implementation** | Function Calling | System Reminder + Event | Slash Command → Op enum |
| **Config System** | TypeScript definitions | 5-tier Markdown config | 3-tier TOML config |
| **Sub-conversation Isolation** | AgentExecutor loop | TaskTool subprocess | run_codex_conversation_one_shot |
| **Approval Delegation** | ❌ Not supported | ❌ Not supported | ✅ Parent session routes approvals |
| **Special Syntax** | ❌ None | ✅ `@` prefix required | ✅ `/` prefix required |
| **User Learning Curve** | Minimal | Moderate | Low (familiar slash commands) |
| **Determinism** | Non-deterministic | Fully deterministic | Fully deterministic |
| **Custom Agents UI** | N/A (built-in only) | ✅ Full UI support | ⚠️ Protocol ready, UI pending |

---

## gemini-cli: LLM-Driven Function Calling

### Overview

gemini-cli implements subagents as standard Gemini API function declarations. The LLM autonomously decides when to invoke them based on context and system prompt guidance.

### Core Philosophy

> **"Trust the LLM completely"**
> Minimize user learning curve by leveraging Gemini's understanding capabilities.

### Trigger Mechanism

**Type**: Heuristic / Automatic
**Method**: Standard Gemini API function calling
**User Interface**: Natural language (no special syntax)

### Implementation Architecture

#### 1. Subagent Registration

**File**: `gemini-cli/packages/core/src/config/config.ts` (Lines 1350-1372)

```typescript
// Subagents registered as tools in tool registry
if (this.getCodebaseInvestigatorSettings().enabled) {
  const definition = this.agentRegistry.getDefinition('codebase_investigator');
  if (definition) {
    const allowedTools = this.getAllowedTools();
    const isAllowed = !allowedTools || allowedTools.includes(definition.name);

    if (isAllowed) {
      const wrapper = new SubagentToolWrapper(
        definition,
        this,
        messageBusEnabled ? this.getMessageBus() : undefined,
      );
      registry.registerTool(wrapper);
    }
  }
}
```

**Key Points**:
- Subagents wrapped in `SubagentToolWrapper` extending `BaseDeclarativeTool`
- Only enabled subagents registered (via `CodebaseInvestigatorSettings.enabled`)
- Must pass allowlist/exclude checks like regular tools

#### 2. Tool Schema Definition

**File**: `gemini-cli/packages/core/src/agents/subagent-tool-wrapper.ts` (Lines 37-57)

```typescript
constructor(
  private readonly definition: AgentDefinition,
  private readonly config: Config,
  messageBus?: MessageBus,
) {
  // Dynamically generate JSON schema from agent definition
  const parameterSchema = convertInputConfigToJsonSchema(
    definition.inputConfig,
  );

  super(
    definition.name,                    // Tool name
    definition.displayName ?? definition.name,
    definition.description,             // LLM sees this
    Kind.Think,                         // Tool category
    parameterSchema,                    // JSON schema
    /* isOutputMarkdown */ true,
    /* canUpdateOutput */ true,
    messageBus,
  );
}
```

#### 3. System Prompt Guidance

**File**: `gemini-cli/packages/core/src/core/prompts.ts` (Lines 143-150)

```typescript
primaryWorkflows_prefix_ci: `
# Primary Workflows

## Software Engineering Tasks
When requested to perform tasks like fixing bugs, adding features,
refactoring, or explaining code, follow this sequence:

1. **Understand & Strategize:** Think about the user's request.
   When the task involves **complex refactoring, codebase exploration
   or system-wide analysis**, your **first and primary tool** must be
   'codebase_investigator'.

   For **simple, targeted searches** (like finding a specific function name),
   you should use 'grep' or 'glob' directly.
```

**Heuristic Rules**:
- Complex refactoring → Use Codebase Investigator
- Codebase exploration → Use Codebase Investigator
- System-wide analysis → Use Codebase Investigator
- Simple searches → Use direct tools (Grep, Glob)

### Execution Flow

```
User Request
    ↓
System Prompt (with heuristic guidance)
    ↓
LLM Receives Tool Declarations (SubagentToolWrapper)
    ↓
LLM Decides to Call Tool (based on context)
    ↓
Function Call in Response
  { name: 'codebase_investigator', args: { objective: '...' } }
    ↓
Turn.handlePendingFunctionCall() extracts call
    ↓
CoreToolScheduler.schedule() creates ToolCall
    ↓
SubagentInvocation.execute() runs
    ↓
AgentExecutor.run() executes agent loop
    ↓
Tool Results returned to LLM as function response
    ↓
LLM continues with subagent output
```

### Key Files Reference

| Purpose | File Path | Key Components |
|---------|-----------|----------------|
| Subagent Registration | `gemini-cli/packages/core/src/config/config.ts` | Lines 1350-1372 |
| Tool Wrapper | `gemini-cli/packages/core/src/agents/subagent-tool-wrapper.ts` | Lines 20-78 |
| Invocation Logic | `gemini-cli/packages/core/src/agents/invocation.ts` | Lines 68-136 |
| Agent Executor | `gemini-cli/packages/core/src/agents/executor.ts` | Lines 75-498 |
| Function Call Handling | `gemini-cli/packages/core/src/core/turn.ts` | Lines 284-385 |
| Tool Scheduling | `gemini-cli/packages/core/src/core/coreToolScheduler.ts` | Lines 668-1250 |
| System Prompts | `gemini-cli/packages/core/src/core/prompts.ts` | Lines 143-150 |
| Agent Definition | `gemini-cli/packages/core/src/agents/codebase-investigator.ts` | Lines 44-90 |

### Advantages

✅ **Seamless UX**: No special syntax to learn
✅ **Intelligent**: LLM makes context-aware decisions
✅ **Standard API**: Leverages Gemini's native function calling
✅ **Minimal Friction**: Users communicate naturally

### Disadvantages

❌ **Non-deterministic**: LLM may not trigger when expected
❌ **No User Control**: Cannot force specific subagent
❌ **Prompt Engineering**: Requires careful system prompt design
❌ **Debugging Difficulty**: Hard to predict trigger behavior

---

## Kode-cli: Explicit @ Mention System

### Overview

Kode-cli implements a user-controlled @ mention system where specific syntax (`@run-agent-xxx` or `@agent-xxx`) explicitly triggers subagents through event-driven system reminder injection.

### Core Philosophy

> **"User has complete control"**
> Provide explicit commands for deterministic triggering with clear UI affordances.

### Trigger Mechanism

**Type**: Explicit Command
**Method**: @ Mention → Event Emission → System Reminder → TaskTool
**User Interface**: `@run-agent-xxx` or `@agent-xxx` syntax

### Implementation Architecture

#### 1. Agent Configuration System

**Format**: Markdown files with YAML frontmatter
**Locations**: 5-tier priority system

**File**: `Kode-cli/src/utils/agentLoader.ts` (Lines 74-80)

```typescript
// Priority order (later overrides earlier)
1. Built-in (code-embedded)
2. ~/.claude/agents/ (Claude Code user directory)
3. ~/.kode/agents/ (Kode user directory)
4. ./.claude/agents/ (Claude Code project directory)
5. ./.kode/agents/ (Kode project directory) ← Highest priority
```

#### 2. Mention Pattern Detection

**File**: `Kode-cli/src/services/mentionProcessor.ts` (Lines 31-36)

```typescript
private static readonly MENTION_PATTERNS = {
  runAgent: /@(run-agent-[\w\-]+)/g,    // Preferred: @run-agent-xxx
  agent: /@(agent-[\w\-]+)/g,            // Legacy: @agent-xxx
  askModel: /@(ask-[\w\-]+)/g,          // Model queries: @ask-xxx
  file: /@([a-zA-Z0-9/._-]+(?:\.[a-zA-Z0-9]+)?)/g  // File refs
} as const
```

#### 3. System Reminder Injection

**File**: `Kode-cli/src/services/systemReminder.ts` (Lines 371-380)

```typescript
// Event listener injects HIGH priority system reminder
this.addEventListener('agent:mentioned', context => {
  this.createMentionReminder({
    type: 'agent_mention',
    key: `agent_mention_${context.agentType}_${context.timestamp}`,
    category: 'task',
    priority: 'high',  // ← Forces LLM attention
    content: `The user mentioned @${context.originalMention}.
      You MUST use the Task tool with subagent_type="${context.agentType}"
      to delegate this task to the specified agent.`,
    timestamp: context.timestamp
  })
})
```

### Execution Flow

```
User Input: "@run-agent-security Please review this code"
    ↓
MentionProcessor.processMessage()
    ↓
Extract agent mentions via regex
    ↓
eventEmitter.emit('agent:mentioned', { agentType: 'security' })
    ↓
SystemReminder Service listens for event
    ↓
Inject HIGH priority system reminder
  "You MUST use Task tool with subagent_type='security'"
    ↓
LLM sees system reminder in context
    ↓
LLM invokes TaskTool({ subagent_type: 'security', prompt: '...' })
    ↓
TaskTool.execute() loads agent config
    ↓
Apply: system prompt + tool filtering + model override
    ↓
Execute agent with modified context
    ↓
Return results to main conversation
```

### Key Files Reference

| Purpose | File Path | Key Components |
|---------|-----------|----------------|
| Agent Loading | `Kode-cli/src/utils/agentLoader.ts` | Lines 18-224 |
| Mention Processing | `Kode-cli/src/services/mentionProcessor.ts` | Lines 31-249 |
| Message Processing | `Kode-cli/src/utils/messages.tsx` | Lines 259-378 |
| TaskTool | `Kode-cli/src/tools/TaskTool/TaskTool.tsx` | Lines 37-127 |
| System Reminders | `Kode-cli/src/services/systemReminder.ts` | Lines 371-380 |

### Advantages

✅ **Deterministic**: Guaranteed trigger when @ mentioned
✅ **User Control**: Precise selection of which agent to invoke
✅ **Clear UX**: Intuitive @ mention interface with autocomplete
✅ **Flexible Config**: 5-tier priority system for agent definitions

### Disadvantages

❌ **Learning Curve**: Users must learn @ mention syntax
❌ **Manual Invocation**: No automatic intelligent triggering
❌ **Context Pollution**: System reminders inject into conversation
❌ **Verbosity**: Requires explicit mention for every invocation

---

## codex: Slash Command System

### Overview

codex implements subagents through an explicit slash command system. Users trigger subagents via commands like `/review` and `/compact`. The Op protocol provides infrastructure for custom agents, though UI integration is currently in development.

### Core Philosophy

> **"Explicit commands with strong protocol foundation"**
> Provide familiar slash command interface with infrastructure ready for extensibility.

### Trigger Mechanism

**Type**: Explicit Command (Slash-based)
**Method**: Slash Command → Op enum → Task spawning
**User Interface**: `/review`, `/compact` (custom agents: protocol ready, UI pending)

### Implementation Architecture

#### 1. Protocol Definition

**File**: `codex-rs/protocol/src/protocol.rs` (Lines 178-193)

```rust
pub enum Op {
    // ... other operations ...

    /// Request the agent to summarize the current conversation context.
    Compact,

    /// Request a code review from the agent.
    Review { review_request: ReviewRequest },

    /// Request a custom configured agent task.
    /// The agent_name must match an agent defined in ~/.codex/agents/ or .codex/agents/
    CustomAgent {
        /// Name of the custom agent (e.g., "security-reviewer")
        agent_name: String,
        /// Initial prompt/task for the custom agent
        prompt: String,
    },

    // ... other operations ...
}
```

**Key Points**:
- **Op::Compact** - Built-in summarization agent (no parameters)
- **Op::Review** - Built-in code review agent (with ReviewRequest parameter)
- **Op::CustomAgent** - File-configured custom agents (with agent_name and prompt)

#### 2. Slash Command Definitions

**File**: `codex-rs/tui/src/slash_command.rs` (Lines 12-32)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, EnumIter, AsRefStr, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum SlashCommand {
    Model,
    Approvals,
    Review,      // ← Maps to Op::Review
    New,
    Init,
    Compact,     // ← Maps to Op::Compact
    Undo,
    Diff,
    Mention,
    Status,
    Mcp,
    Logout,
    Quit,
    Exit,
    Feedback,
    // ... etc
}

impl SlashCommand {
    pub fn description(self) -> &'static str {
        match self {
            SlashCommand::Compact => "summarize conversation to prevent hitting the context limit",
            SlashCommand::Review => "review my current changes and find issues",
            // ...
        }
    }
}
```

#### 3. Slash Command Handling

**File**: `codex-rs/tui/src/chatwidget.rs` (Lines 1237-1248)

```rust
// In ChatWidget::handle_slash_command()
match command {
    SlashCommand::Compact => {
        self.clear_token_usage();
        // Directly send Op::Compact
        self.app_event_tx.send(AppEvent::CodexOp(Op::Compact));
    }
    SlashCommand::Review => {
        // Open popup for user to choose review type
        self.open_review_popup();
    }
    SlashCommand::Model => {
        self.open_model_popup();
    }
    // ...
}
```

#### 4. Review Popup Implementation

**File**: `codex-rs/tui/src/chatwidget.rs` (Lines 2499-2529)

```rust
pub(crate) fn open_review_popup(&mut self) {
    let mut items: Vec<SelectionItem> = Vec::new();

    // Option 1: Review against base branch
    items.push(SelectionItem {
        name: "Review against a base branch".to_string(),
        description: Some("(PR Style)".into()),
        actions: vec![Box::new({
            let cwd = self.config.cwd.clone();
            move |tx| {
                tx.send(AppEvent::OpenReviewBranchPicker(cwd.clone()));
            }
        })],
        dismiss_on_select: false,
        ..Default::default()
    });

    // Option 2: Review uncommitted changes
    items.push(SelectionItem {
        name: "Review uncommitted changes".to_string(),
        actions: vec![Box::new(
            move |tx: &AppEventSender| {
                // Send Op::Review with ReviewRequest
                tx.send(AppEvent::CodexOp(Op::Review {
                    review_request: ReviewRequest {
                        prompt: "Review the current code changes (staged, unstaged, and untracked files) and provide prioritized findings.".to_string(),
                        user_facing_hint: "current changes".to_string(),
                    },
                }));
            },
        )],
        dismiss_on_select: true,
        ..Default::default()
    });

    // ... more options
}
```

#### 5. Op Handler Dispatch

**File**: `codex-rs/core/src/codex.rs` (Lines 1327-1349)

```rust
// In Session::submission_loop()
match sub.op {
    Op::Compact => {
        handlers::compact(&sess, sub.id.clone()).await;
    }
    Op::Review { review_request } => {
        handlers::review(&sess, &config, sub.id.clone(), review_request).await;
    }
    Op::CustomAgent { agent_name, prompt } => {
        handlers::custom_agent(&sess, sub.id.clone(), agent_name, prompt).await;
    }
    _ => {} // Ignore unknown ops
}
```

#### 6. Handler Implementation

**File**: `codex-rs/core/src/codex.rs` (Lines 1647-1664)

```rust
// handlers::custom_agent implementation
pub async fn custom_agent(
    sess: &Arc<Session>,
    sub_id: String,
    agent_name: String,
    prompt: String
) {
    use crate::tasks::CustomAgentTask;
    use codex_protocol::user_input::UserInput;

    let turn_context = sess
        .new_turn_with_sub_id(sub_id, SessionSettingsUpdate::default())
        .await;

    // Seed the custom agent with the prompt as initial user message
    let input: Vec<UserInput> = vec![UserInput::Text { text: prompt }];

    // Spawn the custom agent task
    sess.spawn_task(
        Arc::clone(&turn_context),
        input,
        CustomAgentTask { agent_name },
    )
    .await;
}
```

#### 7. Custom Agent Task

**File**: `codex-rs/core/src/tasks/custom_agent.rs` (Lines 15-94)

```rust
/// Task that executes a custom configured agent.
///
/// Custom agents are defined in ~/.codex/agents/ or .codex/agents/ as TOML files.
#[derive(Clone)]
pub(crate) struct CustomAgentTask {
    /// Name of the custom agent (must match a *.toml file)
    pub agent_name: String,
}

#[async_trait]
impl SessionTask for CustomAgentTask {
    fn kind(&self) -> TaskKind {
        TaskKind::CustomAgent
    }

    async fn run(
        self: Arc<Self>,
        session: Arc<SessionTaskContext>,
        ctx: Arc<TurnContext>,
        input: Vec<UserInput>,
        cancellation_token: CancellationToken,
    ) -> Option<String> {
        // Start sub-codex conversation with custom agent configuration
        let receiver = match start_custom_agent_conversation(
            session.clone(),
            ctx.clone(),
            input,
            cancellation_token.clone(),
            &self.agent_name,
        )
        .await
        {
            Some(receiver) => receiver,
            None => return None,
        };

        // Forward events from subagent to parent session
        while let Ok(event) = receiver.recv().await {
            if cancellation_token.is_cancelled() {
                break;
            }

            // Forward all events (approvals already handled by codex_delegate.rs)
            let sess = session.clone_session();
            sess.send_event(ctx.as_ref(), event.msg).await;
        }

        None
    }
}

async fn start_custom_agent_conversation(
    session: Arc<SessionTaskContext>,
    ctx: Arc<TurnContext>,
    input: Vec<UserInput>,
    cancellation_token: CancellationToken,
    agent_name: &str,
) -> Option<async_channel::Receiver<Event>> {
    let config = ctx.client.config();
    let sub_agent_config = config.as_ref().clone();

    // Configuration loaded and applied by codex_delegate.rs
    (run_codex_conversation_one_shot(
        sub_agent_config,
        session.auth_manager(),
        input,
        session.clone_session(),
        ctx.clone(),
        cancellation_token,
        None,
        codex_protocol::protocol::SubAgentSource::Other(agent_name.to_string()),
    )
    .await)
        .ok()
        .map(|io| io.rx_event)
}
```

#### 8. Agent Registry

**File**: `codex-rs/core/src/agent_registry.rs`

```rust
use std::collections::HashMap;
use std::sync::LazyLock;

/// Global registry of agent definitions loaded from configuration files.
/// Uses LazyLock for one-time initialization with caching.
pub static AGENT_REGISTRY: LazyLock<HashMap<String, AgentDefinition>> =
    LazyLock::new(|| {
        let mut registry = HashMap::new();

        // Load built-in agents (Review, Compact)
        load_builtin_agents(&mut registry);

        // Load user agents from ~/.codex/agents/
        if let Some(user_dir) = get_user_agent_dir() {
            load_agents_from_directory(&user_dir, &mut registry);
        }

        // Load project agents from .codex/agents/ (highest priority)
        if let Some(project_dir) = get_project_agent_dir() {
            load_agents_from_directory(&project_dir, &mut registry);
        }

        registry
    });
```

**3-Tier Priority**:
1. Built-in (Review, Compact)
2. User directory (`~/.codex/agents/`)
3. Project directory (`.codex/agents/`) ← Highest priority

#### 9. Agent Definition Schema

**File**: `codex-rs/protocol/src/agent_definition.rs`

```rust
/// Configuration for a custom agent loaded from TOML files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    /// Unique identifier for the agent
    pub name: String,

    /// Human-readable description
    pub description: String,

    /// Custom system prompt
    #[serde(default)]
    pub system_prompt: Option<String>,

    /// Model override (e.g., "opus", "sonnet", "haiku")
    #[serde(default)]
    pub model: Option<String>,

    /// Tool filtering (list of allowed tools or "*" for all)
    #[serde(default)]
    pub tools: Option<Vec<String>>,

    /// Maximum number of turns
    #[serde(default)]
    pub max_turns: Option<i32>,

    /// Thinking budget for extended thinking models
    #[serde(default)]
    pub thinking_budget: Option<i32>,
}
```

#### 10. Approval Delegation (Unique Feature)

**File**: `codex-rs/core/src/codex_delegate.rs`

codex's unique feature: subagents can delegate approval requests back to the parent session.

```rust
/// Forward approval requests from subagent to parent session
pub async fn forward_events(
    parent_session: Arc<Session>,
    sub_rx: async_channel::Receiver<Event>,
) {
    while let Ok(event) = sub_rx.recv().await {
        match &event.msg {
            EventMsg::ExecApprovalRequest(req) => {
                // Forward to parent for approval
                parent_session.handle_approval_request(req).await;
            }
            EventMsg::ApplyPatchApprovalRequest(req) => {
                // Forward to parent for approval
                parent_session.handle_patch_approval_request(req).await;
            }
            _ => {
                // Forward all other events normally
                parent_session.send_event(&turn_context, event.msg).await;
            }
        }
    }
}
```

### Execution Flow

```
User Types: "/compact"
    ↓
ChatWidget.handle_slash_command()
  (tui/src/chatwidget.rs:1237)
    ↓
Send AppEvent::CodexOp(Op::Compact)
    ↓
Session.submission_loop() receives Op::Compact
  (core/src/codex.rs:1327)
    ↓
handlers::compact() called
    ↓
Spawn CompactTask
    ↓
run_codex_conversation_one_shot() with SubAgentSource::Compact
    ↓
Execute subagent conversation
    ↓
Forward events to parent session
    ↓
Return results to user
```

**For /review**:

```
User Types: "/review"
    ↓
open_review_popup()
  (tui/src/chatwidget.rs:2499)
    ↓
User Selects "Review uncommitted changes"
    ↓
Send Op::Review { review_request: ReviewRequest { ... } }
    ↓
handlers::review() called
  (core/src/codex.rs:1344)
    ↓
Spawn ReviewTask
    ↓
run_codex_conversation_one_shot() with SubAgentSource::Review
    ↓
Execute review subagent
    ↓
Return findings to parent
```

**For CustomAgent** (protocol ready, UI pending):

```
[Future UI or Programmatic Call]
    ↓
Send Op::CustomAgent { agent_name: "security-reviewer", prompt: "..." }
    ↓
handlers::custom_agent() called
  (core/src/codex.rs:1347)
    ↓
Spawn CustomAgentTask { agent_name }
    ↓
start_custom_agent_conversation()
  (core/src/tasks/custom_agent.rs:69)
    ↓
Load agent definition from AGENT_REGISTRY
    ↓
run_codex_conversation_one_shot() with SubAgentSource::Other(agent_name)
    ↓
Apply: system_prompt + model + tools + max_turns from TOML config
    ↓
Execute custom agent
    ↓
Forward events (including approvals) to parent
    ↓
Return results
```

### Key Files Reference

| Purpose | File Path | Key Components |
|---------|-----------|----------------|
| Protocol Definition | `codex-rs/protocol/src/protocol.rs` | Lines 178-193 (Op enum) |
| Slash Commands | `codex-rs/tui/src/slash_command.rs` | Lines 12-32 (enum), 36-55 (descriptions) |
| Command Handling | `codex-rs/tui/src/chatwidget.rs` | Lines 1237-1248 (dispatch), 2499-2529 (review popup) |
| Op Dispatch | `codex-rs/core/src/codex.rs` | Lines 1327-1349 (match), 1647-1664 (handler) |
| Custom Agent Task | `codex-rs/core/src/tasks/custom_agent.rs` | Lines 15-94 |
| Agent Registry | `codex-rs/core/src/agent_registry.rs` | LazyLock registry, file loading |
| Agent Definition | `codex-rs/protocol/src/agent_definition.rs` | TOML schema |
| Approval Delegation | `codex-rs/core/src/codex_delegate.rs` | Event forwarding |
| Integration Tests | `codex-rs/core/tests/suite/codex_delegate.rs` | Lines 227-258 (custom agent test) |

### Current Implementation Status

#### ✅ **Fully Implemented**:
- Protocol infrastructure (`Op::Compact`, `Op::Review`, `Op::CustomAgent`)
- Agent registry with LazyLock caching
- TOML configuration loading
- Task execution framework
- Approval delegation system
- Built-in agents (Review, Compact)
- Slash command UI for built-in agents

#### ⚠️ **Partially Implemented**:
- Custom agent infrastructure complete
- Custom agent UI **not yet implemented** in TUI
- Can be triggered via:
  - ✅ Programmatic API calls
  - ✅ Integration tests
  - ❌ TUI slash commands (pending)

### Example Agent Configuration

**File**: `~/.codex/agents/security-reviewer.toml`

```toml
name = "security-reviewer"
description = "Specialized agent for security code review and vulnerability analysis"
system_prompt = """
You are a security expert specializing in code review for vulnerabilities.
Focus on: OWASP Top 10, authentication/authorization, input validation,
crypto, secrets management, and secure coding practices.
"""
model = "opus"  # Use more powerful model for security analysis
tools = ["FileRead", "Grep", "Bash"]  # Restricted tool access
max_turns = 20
thinking_budget = 10000
```

### Design Characteristics

#### **No Automatic Triggering**

codex does **NOT** implement automatic/heuristic agent selection:
- ❌ No semantic matching of user intent to agent descriptions
- ❌ No LLM-driven automatic agent invocation
- ❌ No "smart suggestions" for which agent to use

#### **Fully Explicit Control**

All agent invocations are **user-initiated**:
- User types `/compact` → Op::Compact sent
- User types `/review` → popup shown → user selects → Op::Review sent
- Custom agents require explicit Op::CustomAgent (UI pending)

#### **Strong Protocol Foundation**

The infrastructure is protocol-first:
- Op enum defines all possible operations
- Handlers are decoupled from UI
- Can be triggered via:
  - TUI slash commands
  - CLI arguments (potential)
  - Programmatic API
  - Tests

### Advantages

✅ **Deterministic**: Explicit commands guarantee predictable behavior
✅ **Familiar UX**: Slash commands are familiar to users (Discord, Slack, etc.)
✅ **Strong Infrastructure**: Protocol-first design enables multiple frontends
✅ **Approval Delegation**: Unique hierarchical approval system
✅ **Type-Safe**: Rust enum ensures compile-time correctness
✅ **Extensible**: TOML configuration for custom agents

### Disadvantages

❌ **Manual Invocation**: No automatic intelligent triggering
❌ **Learning Curve**: Users must learn slash commands
❌ **UI Incomplete**: Custom agent UI not yet in TUI
❌ **No Smart Suggestions**: No help choosing which agent to use

### Future Potential

Given the strong protocol foundation, codex could add:

1. **Slash command for custom agents**: `/agent <name> <prompt>`
2. **Agent list command**: `/agents` to show available custom agents
3. **Smart suggestions**: Analyze user input and suggest agents
4. **Hybrid triggering**: Keep explicit commands, add optional LLM-driven suggestions

---

## Architecture Philosophy Comparison

### gemini-cli: "Trust the LLM Completely"

**Design Core**:
```typescript
// Simply register as tool, let Gemini decide
allTools.push(new SubagentToolWrapper(subagent));
```

**Philosophy**:
- Minimal user friction
- Leverage strong LLM understanding
- Standard API patterns
- System prompt engineering

**Best For**:
- Google AI Studio ecosystem
- Users who prefer natural conversation
- Scenarios with strong LLM support

---

### Kode-cli: "User Has Complete Control"

**Design Core**:
```typescript
// User explicitly triggers via @mention
eventEmitter.on('agent:mentioned', ({ agentName }) => {
  forceInvokeTaskTool(agentName);  // Forced invocation
});
```

**Philosophy**:
- Explicit commands prioritized
- Clear @ mention interface
- Deterministic behavior
- User empowerment

**Best For**:
- Enterprise environments
- Multi-agent collaboration
- Explicit workflows
- Users who prefer precise control

---

### codex: "Explicit Commands with Strong Protocol"

**Design Core**:
```rust
// Slash commands mapped to Op enum
match command {
    SlashCommand::Compact => Op::Compact,
    SlashCommand::Review => Op::Review { review_request },
    // Custom agents: protocol ready, UI pending
}
```

**Philosophy**:
- Familiar slash command interface
- Protocol-first design
- Type-safe operations
- Extensible via configuration

**Best For**:
- Users familiar with slash commands (Discord, Slack)
- Scenarios requiring deterministic behavior
- Multi-frontend architectures (TUI, CLI, web)
- Type-safe Rust ecosystems

---

## Implementation Details

### Configuration Systems Compared

#### gemini-cli

**Format**: TypeScript code definitions
**Location**: Embedded in codebase
**Example**: `gemini-cli/packages/core/src/agents/codebase-investigator.ts`

```typescript
export const CodebaseInvestigatorAgent: AgentDefinition = {
  name: 'codebase_investigator',
  displayName: 'Codebase Investigator Agent',
  description: `...`,
  inputConfig: { ... },
  modelConfig: { ... },
  runConfig: { ... },
};
```

**Pros**: Type-safe, IDE support
**Cons**: Requires code changes, compilation

---

#### Kode-cli

**Format**: Markdown with YAML frontmatter
**Location**: 5-tier directory system
**Example**: `.kode/agents/code-reviewer.md`

```yaml
---
name: code-reviewer
description: "Code review specialist"
tools: ["FileRead", "Grep"]
model_name: gpt-4
---

System prompt here...
```

**Pros**: Hot-reload, user-editable, compatible with Claude ecosystem
**Cons**: Less type-safe, parsing overhead

---

#### codex

**Format**: TOML configuration
**Location**: 3-tier directory system
**Example**: `~/.codex/agents/security-reviewer.toml`

```toml
name = "security-reviewer"
description = "Security review specialist"
system_prompt = "..."
model = "opus"
tools = ["FileRead", "Grep", "Bash"]
max_turns = 20
thinking_budget = 10000
```

**Pros**: Type-safe parsing, concise, standard format, Rust-native
**Cons**: Requires TOML parser

---

### Trigger Determinism

| System | Determinism Level | Reason |
|--------|-------------------|---------|
| gemini-cli | Low | Depends on LLM interpretation |
| Kode-cli | High | Forced via system reminder |
| codex | High | Explicit slash commands |

---

### Context Pollution

| System | Pollution Level | Method |
|--------|-----------------|---------|
| gemini-cli | None | Standard function calling |
| Kode-cli | High | System reminders injected |
| codex | None | Direct Op enum, no context injection |

---

### User Learning Curve

| System | Learning Curve | Reason |
|--------|----------------|---------|
| gemini-cli | Minimal | Natural language only |
| Kode-cli | Moderate | Must learn @ mention syntax |
| codex | Low | Familiar slash commands |

---

## Best Practices & Recommendations

### When to Use Each Approach

#### Choose **gemini-cli** style if:

✅ You have a strong, reliable LLM (e.g., Gemini 1.5 Pro+)
✅ You want minimal user friction
✅ You're building for users who prefer natural conversation
✅ You can invest in system prompt engineering
✅ Non-determinism is acceptable

**Example Use Case**: Consumer-facing AI assistant with strong Google ecosystem integration.

---

#### Choose **Kode-cli** style if:

✅ You need deterministic, reliable triggering
✅ Users are technical and prefer explicit control
✅ You have multiple specialized agents
✅ You need clear audit trails of agent invocations
✅ Enterprise environment with compliance requirements

**Example Use Case**: DevOps automation platform with multiple specialized agents for different tasks.

---

#### Choose **codex** style if:

✅ You want familiar slash command UX
✅ You need deterministic behavior
✅ You're building multi-frontend architectures
✅ You value type-safety and compile-time correctness
✅ You want protocol-first design for extensibility

**Example Use Case**: Rust-based coding assistant with TUI, CLI, and potential web frontends.

---

### Implementation Recommendations

#### For Slash Command Systems

1. **Command Discoverability**: Implement `/help` to list all commands
2. **Autocomplete**: Show suggestions as user types `/`
3. **Clear Descriptions**: Provide inline help for each command
4. **Familiar Patterns**: Follow conventions (Discord, Slack, IRC)

#### For Protocol-First Design

1. **Decouple UI from Logic**: Op enum should be UI-agnostic
2. **Type Safety**: Use strongly-typed enums/variants
3. **Versioning**: Plan for protocol evolution
4. **Multiple Frontends**: Design for TUI, CLI, web, API

#### For Configuration Systems

1. **Hot Reload**: Support runtime config updates when possible
2. **Validation**: Validate configs at load time
3. **Priority System**: Clear precedence rules (user > project > built-in)
4. **Defaults**: Sensible defaults for all optional fields

#### For Subagent Execution

1. **Isolation**: Run subagents in isolated conversation contexts
2. **Streaming**: Stream intermediate thoughts for transparency
3. **Cancellation**: Support aborting long-running agents
4. **Error Recovery**: Graceful handling of agent failures
5. **Approval Forwarding**: Delegate approvals to parent if needed

---

## Conclusion

Each approach represents valid trade-offs:

- **gemini-cli**: Optimal for LLM-first, minimal-friction experiences
- **Kode-cli**: Optimal for control, determinism, and explicit workflows
- **codex**: Optimal for type-safe, protocol-first, multi-frontend architectures

The choice depends on your:
- **Target users**: Novice users prefer natural language; power users prefer explicit commands
- **LLM capabilities**: Strong LLMs enable heuristic triggering; explicit commands work with any LLM
- **Determinism requirements**: Enterprise/compliance needs deterministic triggering
- **Architecture**: Protocol-first designs enable multiple frontends
- **Type safety**: Rust/typed languages benefit from strong enum-based designs

For **codex specifically**:

The current implementation provides:
- ✅ Strong protocol foundation with Op enum
- ✅ Familiar slash command UX
- ✅ Type-safe Rust implementation
- ✅ Unique approval delegation
- ✅ Extensible TOML configuration

Future enhancements could add:
- 🔄 Custom agent slash commands in TUI
- 🔄 Agent discovery UI (`/agents` command)
- 🔄 Optional smart suggestions (while keeping explicit commands)
- 🔄 Web-based frontend leveraging the same Op protocol

---

**Document Version**: 2.0
**Last Updated**: 2025-11-11
**Maintenance**: Update when subagent implementations change significantly

**Note**: This document is based on code analysis as of 2025-11-11. Always verify against current codebase when implementing new features.
