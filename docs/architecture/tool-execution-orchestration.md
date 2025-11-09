# Tool Execution Orchestration: Detailed Flow

**Status:** Living document
**Last Updated:** 2025-11-10
**Audience:** Codex developers

## Overview

This document provides a detailed walkthrough of how tool execution flows through Codex's approval and sandbox systems. For architectural overview, see [`sandbox-approval-integration.md`](./sandbox-approval-integration.md).

---

## Complete Execution Flow

### Entry Point: User Turn

**Location:** `codex-rs/core/src/conversation.rs`

```
User sends message
│
├─ TUI: User types in prompt pane
├─ CLI: codex exec "prompt"
└─ API: POST /conversations/{id}/turns
│
▼
CodexConversation.process_turn()
│
├─ Build turn context (cwd, env, policies)
├─ Send to LLM provider
└─ Process response stream
    │
    ├─ Text chunks → Stream to user
    ├─ Thinking blocks → Store/display
    └─ Function calls → Route to tool execution
        │
        ▼
     Tool Router
```

### Tool Router

**Location:** `codex-rs/tools/router.rs`

```
Function call received
│
├─ Parse call_id and tool name
├─ Lookup tool in registry
│   └─ Registry: Arc<RwLock<HashMap<String, Arc<dyn ToolHandler>>>>
│
├─ Check parallel execution support
│   └─ supports_parallel_tool_calls flag per tool
│
├─ Multiple calls in same response?
│   ├─ All support parallel → Concurrent execution (read lock)
│   ├─ Any non-parallel → Serial execution (write lock)
│   └─ Mixed → Safe serial fallback
│
└─ For each call:
    │
    ▼
  ToolOrchestrator.run()
```

---

## ToolOrchestrator Detailed Flow

**Location:** `codex-rs/core/src/tools/orchestrator.rs:22-187`

This is the heart of approval and sandbox integration.

### Phase 1: Initialization

```rust
pub async fn run<T, Req>(
    tool: &T,
    request: Req,
    approval_policy: AskForApproval,
    sandbox_policy: &SandboxPolicy,
    services: &SessionServices,
) -> Result<T::Output, CodexErr>
where
    T: ToolHandler + Approvable<Req>,
```

**Inputs:**
- `tool`: The tool handler (exec, patch, etc.)
- `request`: Tool-specific request struct
- `approval_policy`: From config or CLI flags
- `sandbox_policy`: From config or CLI flags
- `services`: Session-wide services (approval store, LLM client, etc.)

**Initial state:**
```rust
let mut already_approved = false;
let mut sandbox_selected = None;
```

### Phase 2: Initial Approval Check

```
┌─────────────────────────────────────────┐
│  Check if approval needed               │
└───────────────┬─────────────────────────┘
                │
                ▼
   tool.wants_initial_approval(
       &request,
       approval_policy,
       sandbox_policy
   )
                │
                ├─ Returns false → Skip to Phase 3
                └─ Returns true → Continue
                │
                ▼
┌─────────────────────────────────────────┐
│  Check approval cache                   │
│  (with_cached_approval)                 │
└───────────────┬─────────────────────────┘
                │
                ├─ Found ApprovedForSession → already_approved = true
                └─ Not found → Continue
                │
                ▼
┌─────────────────────────────────────────┐
│  Should bypass approval?                │
│  tool.should_bypass_approval(           │
│      approval_policy,                   │
│      already_approved                   │
│  )                                      │
└───────────────┬─────────────────────────┘
                │
                ├─ Returns true → Skip to Phase 3
                └─ Returns false → Continue
                │
                ▼
┌─────────────────────────────────────────┐
│  Build approval context                 │
│  - Turn context (cwd, env)              │
│  - Risk assessment (if enabled)         │
│  - Session services                     │
└───────────────┬─────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────┐
│  tool.start_approval_async()            │
│  - Request user decision                │
│  - Show approval UI                     │
│  - Wait for response                    │
└───────────────┬─────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────┐
│  Handle ReviewDecision                  │
│  ├─ Approved → Continue                 │
│  ├─ ApprovedForSession → Cache + Continue│
│  ├─ Denied → Return error to model      │
│  └─ Abort → Cancel entire turn          │
└─────────────────────────────────────────┘
```

**Code:**
```rust
if tool.wants_initial_approval(&request, approval_policy, sandbox_policy) {
    let decision = with_cached_approval(
        services,
        tool.approval_key(&request),
        |already_approved| async move {
            if tool.should_bypass_approval(approval_policy, already_approved) {
                return ReviewDecision::Approved;
            }

            let ctx = ApprovalCtx {
                turn_context: &services.turn_context,
                services,
                reason: None,
                risk: None,
            };

            tool.start_approval_async(&request, ctx).await
        },
    )
    .await;

    match decision {
        ReviewDecision::Approved | ReviewDecision::ApprovedForSession => {
            already_approved = true;
        }
        ReviewDecision::Denied => {
            return Err(CodexErr::ApprovalDenied);
        }
        ReviewDecision::Abort => {
            return Err(CodexErr::UserAborted);
        }
    }
}
```

### Phase 3: Sandbox Selection

```
┌─────────────────────────────────────────┐
│  Get tool sandbox preference            │
│  tool.sandbox_preference()              │
│  → Auto / Require / Forbid              │
└───────────────┬─────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────┐
│  Check if escalation requested          │
│  tool.wants_escalated_first_attempt()   │
└───────────────┬─────────────────────────┘
                │
                ├─ Returns true → sandbox = SandboxType::None
                └─ Returns false → Continue
                │
                ▼
┌─────────────────────────────────────────┐
│  SandboxManager.select_initial()        │
│  (policy, preference) → SandboxType     │
└───────────────┬─────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────────┐
│  Platform capability check                  │
│  ├─ macOS → MacosSeatbelt (if available)    │
│  ├─ Linux → LinuxSeccomp (if supported)     │
│  ├─ Windows → WindowsRestrictedToken (exp)  │
│  └─ Fallback → SandboxType::None            │
└───────────────┬─────────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────┐
│  Preference = Require?                  │
│  └─ If sandbox is None → ERROR          │
│     "Sandbox required but unavailable"  │
└─────────────────────────────────────────┘
```

**Code:**
```rust
let sandbox_preference = tool.sandbox_preference();

let sandbox_type = if tool.wants_escalated_first_attempt(&request) {
    SandboxType::None
} else {
    services
        .sandbox_manager
        .select_initial(sandbox_policy, sandbox_preference)
};

if sandbox_preference == SandboxablePreference::Require
    && sandbox_type == SandboxType::None
{
    return Err(CodexErr::SandboxRequired);
}

sandbox_selected = Some(sandbox_type);
```

### Phase 4: Command Transformation

```
┌─────────────────────────────────────────┐
│  Build CommandSpec                      │
│  ├─ Program path                        │
│  ├─ Arguments                           │
│  ├─ Working directory                   │
│  ├─ Base environment variables          │
│  └─ Timeout                             │
└───────────────┬─────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────┐
│  SandboxManager.transform()             │
│  (spec, policy, sandbox_type, ctx)      │
└───────────────┬─────────────────────────┘
                │
                ├─ sandbox_type = None → Return spec as-is
                └─ Otherwise → Platform transformation
                │
                ▼
┌─────────────────────────────────────────────────┐
│  Platform-Specific Transformation               │
│  ┌──────────────────────────────────────────┐   │
│  │ MacosSeatbelt:                           │   │
│  │ - Canonicalize all paths                 │   │
│  │ - Generate Scheme policy profile         │   │
│  │ - Build writable roots list              │   │
│  │ - Exclude .git/ subdirectories           │   │
│  │ - Wrap: sandbox-exec -p <profile> <cmd>  │   │
│  └──────────────────────────────────────────┘   │
│  ┌──────────────────────────────────────────┐   │
│  │ LinuxSeccomp:                            │   │
│  │ - Serialize policy to JSON               │   │
│  │ - Wrap: codex-linux-sandbox <json> -- <cmd>│ │
│  └──────────────────────────────────────────┘   │
│  ┌──────────────────────────────────────────┐   │
│  │ WindowsRestrictedToken:                  │   │
│  │ - Create AppContainer profile            │   │
│  │ - Attach capability SIDs                 │   │
│  │ - Override proxy environment vars        │   │
│  └──────────────────────────────────────────┘   │
└───────────────┬─────────────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────┐
│  Add Codex environment variables        │
│  ├─ CODEX_SANDBOX_NETWORK_DISABLED=1    │
│  │  (if !policy.has_full_network_access)│
│  └─ CODEX_SANDBOX=seatbelt              │
│     (on macOS with Seatbelt)            │
└───────────────┬─────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────┐
│  Return ExecEnv                         │
│  - Wrapped program/args                 │
│  - Enhanced environment                 │
│  - Ready for execution                  │
└─────────────────────────────────────────┘
```

**Code:**
```rust
let exec_env = services
    .sandbox_manager
    .transform(&spec, sandbox_policy, sandbox_type, &services.turn_context)?;
```

### Phase 5: First Execution Attempt

```
┌─────────────────────────────────────────┐
│  tool.run(exec_env)                     │
│  - Tool-specific execution logic        │
│  - spawn_child_async() for commands     │
│  - Direct file I/O for patches          │
└───────────────┬─────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────┐
│  Execution completes                    │
│  → ToolOutput { stdout, stderr, exit }  │
└───────────────┬─────────────────────────┘
                │
                ▼
         Success or Failure?
                │
                ├─ Success (exit code 0)
                │  └─ Return output → Phase 7
                │
                └─ Failure (exit code ≠ 0)
                   └─ Continue to Phase 6
```

### Phase 6: Sandbox Denial Detection & Retry

```
┌─────────────────────────────────────────┐
│  Was sandbox used?                      │
│  sandbox_selected.is_some()             │
└───────────────┬─────────────────────────┘
                │
                ├─ No sandbox → Return failure to model
                └─ Sandbox was used → Continue
                │
                ▼
┌─────────────────────────────────────────┐
│  SandboxManager.denied()                │
│  Check if failure was sandbox denial    │
│  - Platform-specific error patterns     │
│  - Exit codes                           │
│  - stderr content                       │
└───────────────┬─────────────────────────┘
                │
                ├─ Not a denial → Return failure to model
                └─ Sandbox denied → Continue
                │
                ▼
┌─────────────────────────────────────────┐
│  Can retry without sandbox?             │
│  tool.wants_no_sandbox_approval(policy) │
└───────────────┬─────────────────────────┘
                │
                ├─ policy = Never → No retry
                └─ Otherwise → Continue
                │
                ▼
┌─────────────────────────────────────────┐
│  Run Risk Assessment (if enabled)       │
│  - Feature: experimental_sandbox_       │
│    command_assessment                   │
│  - LLM analyzes blocked command         │
│  - Returns risk level + description     │
│  - 5 second timeout                     │
└───────────────┬─────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────┐
│  Request Retry Approval                 │
│  - Reason: "Blocked by sandbox"         │
│  - Include risk assessment              │
│  - Show failed command stderr           │
│  - User decides: Approve/Deny/Abort     │
└───────────────┬─────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────┐
│  Handle Retry Decision                  │
│  ├─ Denied → Return failure to model    │
│  ├─ Abort → Cancel turn                 │
│  └─ Approved → Continue                 │
└───────────────┬─────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────┐
│  Transform without sandbox              │
│  SandboxManager.transform(              │
│      spec,                              │
│      policy,                            │
│      SandboxType::None,  ← Force        │
│      ctx                                │
│  )                                      │
└───────────────┬─────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────┐
│  tool.run(no_sandbox_env)               │
│  - Second attempt without sandbox       │
│  - No further retries                   │
└───────────────┬─────────────────────────┘
                │
                ▼
         Return output
```

**Code:**
```rust
let mut output = tool.run(exec_env).await?;

if let Some(sandbox) = sandbox_selected {
    if services.sandbox_manager.denied(sandbox, &output) {
        if tool.wants_no_sandbox_approval(approval_policy) {
            // Run risk assessment
            let risk = if services.features.experimental_sandbox_command_assessment {
                assess_command_risk(&spec, sandbox_policy, &output.stderr).await
            } else {
                None
            };

            // Request retry approval
            let ctx = ApprovalCtx {
                turn_context: &services.turn_context,
                services,
                reason: Some("Command blocked by sandbox. Retry without sandbox?"),
                risk,
            };

            let decision = tool.start_approval_async(&request, ctx).await;

            match decision {
                ReviewDecision::Approved | ReviewDecision::ApprovedForSession => {
                    // Retry without sandbox
                    let no_sandbox_env = services.sandbox_manager.transform(
                        &spec,
                        sandbox_policy,
                        SandboxType::None,
                        &services.turn_context,
                    )?;

                    output = tool.run(no_sandbox_env).await?;
                }
                ReviewDecision::Denied => {
                    return Ok(output); // Return original failure
                }
                ReviewDecision::Abort => {
                    return Err(CodexErr::UserAborted);
                }
            }
        }
    }
}
```

### Phase 7: Return to Model

```
┌─────────────────────────────────────────┐
│  Format ToolOutput                      │
│  - stdout                               │
│  - stderr                               │
│  - exit_code                            │
│  - duration                             │
└───────────────┬─────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────┐
│  Send to LLM as function result         │
│  - call_id matches original request     │
│  - Content formatted per tool           │
└───────────────┬─────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────┐
│  LLM processes result                   │
│  - Success → Continue task              │
│  - Failure → Retry or adjust approach   │
│  - May trigger more tool calls          │
└─────────────────────────────────────────┘
```

---

## Approval Decision Trees

### wants_initial_approval() Logic

**For Exec Tool** (`codex-rs/tools/exec/src/lib.rs`):

```
wants_initial_approval(request, approval_policy, sandbox_policy)
│
├─ approval_policy = Never?
│  └─ return false
│
├─ sandbox_policy = DangerFullAccess?
│  └─ return false (no sandbox to escape from)
│
├─ approval_policy = UnlessTrusted?
│  ├─ Is command safe? (is_safe_command check)
│  │  ├─ YES → return false (auto-approve)
│  │  └─ NO → return true (needs approval)
│  └─ return true
│
├─ approval_policy = OnFailure?
│  └─ return false (approve first attempt, ask on denial)
│
└─ approval_policy = OnRequest?
   └─ return true (model decides, usually skips if sandboxed)
```

**Safe command detection** (`codex-rs/core/src/command_safety/is_safe_command.rs`):

Known safe commands:
- Read-only: `ls`, `cat`, `head`, `tail`, `grep`, `find` (without `-exec`), `git log`, `git status`, `git diff`
- Safe flags only: `ls` without `-exec`, `find` without `-delete`
- Dangerous patterns blocked: Redirects (`>`), pipes to write commands, sudo, rm, etc.

### wants_no_sandbox_approval() Logic

**For Exec Tool:**

```
wants_no_sandbox_approval(approval_policy)
│
├─ approval_policy = Never?
│  └─ return false (no retry prompts)
│
├─ approval_policy = OnFailure?
│  └─ return true (auto-approve retry)
│
└─ approval_policy = OnRequest | UnlessTrusted?
   └─ return true (ask user for retry)
```

### should_bypass_approval() Logic

**For all tools:**

```
should_bypass_approval(approval_policy, already_approved)
│
├─ already_approved = true?
│  └─ return true (found ApprovedForSession in cache)
│
├─ approval_policy = Never?
│  └─ return true (skip all approvals)
│
└─ Otherwise
   └─ return false (need to ask)
```

---

## Parallel Tool Execution

### Parallel vs Serial Execution

**Location:** `codex-rs/tools/parallel.rs`

```
Multiple tool calls in LLM response
│
├─ Analyze all calls
│  └─ Check each tool's supports_parallel_tool_calls flag
│
├─ All support parallel?
│  ├─ YES → Concurrent execution
│  │  └─ Use read lock on registry
│  │  └─ tokio::spawn for each call
│  │  └─ join_all to collect results
│  │
│  └─ NO → Serial execution
│     └─ Use write lock on registry
│     └─ Execute calls sequentially
│     └─ Preserve order
```

**Parallelization rules:**

1. **Parallel-safe tools** (`supports_parallel_tool_calls = true`):
   - Read-only operations (search, read file)
   - Stateless tools
   - Tools with internal synchronization

2. **Serial-only tools** (`supports_parallel_tool_calls = false`):
   - Exec tool (shell state dependencies)
   - Patch tool (file modification conflicts)
   - REPL tool (terminal state)

3. **Mixed calls**:
   - Always fall back to serial execution
   - Ensures safety at cost of performance

**Code:**
```rust
// codex-rs/tools/parallel.rs:42-87
pub async fn execute_tool_calls(
    calls: Vec<ToolCall>,
    registry: &ToolRegistry,
) -> Vec<ToolOutput> {
    let all_parallel = calls
        .iter()
        .all(|call| {
            registry
                .get(&call.name)
                .map(|tool| tool.supports_parallel_tool_calls())
                .unwrap_or(false)
        });

    if all_parallel {
        // Concurrent execution with read lock
        let futures = calls.into_iter().map(|call| {
            let registry = registry.clone();
            tokio::spawn(async move {
                let tool = registry.get(&call.name).unwrap();
                tool.run(call.invocation).await
            })
        });

        futures::future::join_all(futures).await
    } else {
        // Serial execution with write lock
        let mut outputs = Vec::new();
        for call in calls {
            let tool = registry.get(&call.name).unwrap();
            let output = tool.run(call.invocation).await;
            outputs.push(output);
        }
        outputs
    }
}
```

---

## Subagent Approval Routing

### Subagent Lifecycle

**Location:** `codex-rs/core/src/codex_delegate.rs`

```
Parent Session
│
├─ User requests review/compact/custom subagent
│  └─ call run_codex_conversation_interactive()
│
├─ Create isolated CodexConversation
│  ├─ New conversation ID
│  ├─ Inherit policies from parent
│  └─ Separate message history
│
├─ Spawn event forwarder task
│  └─ forward_events() intercepts approval requests
│
├─ Start subagent processing
│  └─ Model generates response
│     └─ Function calls trigger tool execution
│        └─ ToolOrchestrator requests approval
│           └─ ExecApprovalRequestEvent emitted
│
├─ Event forwarder intercepts approval
│  └─ Route to parent_session.request_command_approval()
│
├─ Parent shows approval UI to user
│  └─ User makes decision
│
├─ Decision sent back to subagent
│  └─ codex.respond_to_approval(call_id, decision)
│
└─ Subagent continues execution
   └─ Eventually completes and returns to parent
```

### Event Routing Code

```rust
// codex-rs/core/src/codex_delegate.rs:152-204
async fn forward_events(
    codex: &CodexConversation,
    id: &ConversationId,
    parent_session: &SessionHandle,
    parent_ctx: &Context,
    tx_sub: mpsc::Sender<Event>,
    cancel_token: CancellationToken,
) -> Result<(), CodexErr> {
    while let Ok(event) = codex.next_event().await {
        match event {
            // Intercept exec approval requests
            Event {
                msg: EventMsg::ExecApprovalRequest(event),
                ..
            } => {
                let decision = parent_session
                    .request_command_approval(
                        id,
                        parent_ctx,
                        event.command.clone(),
                        event.cwd.clone(),
                        event.reason.clone(),
                        event.risk.clone(),
                    )
                    .await;

                if cancel_token.is_cancelled() {
                    let _ = codex
                        .respond_to_approval(&event.call_id, ReviewDecision::Abort)
                        .await;
                    return Ok(());
                }

                let _ = codex
                    .respond_to_approval(&event.call_id, decision)
                    .await;
            }

            // Intercept patch approval requests
            Event {
                msg: EventMsg::ApplyPatchApprovalRequest(event),
                ..
            } => {
                let decision = parent_session
                    .request_patch_approval(
                        id,
                        parent_ctx,
                        event.changes.clone(),
                        event.reason.clone(),
                        event.grant_root.clone(),
                    )
                    .await;

                if cancel_token.is_cancelled() {
                    let _ = codex
                        .respond_to_approval(&event.call_id, ReviewDecision::Abort)
                        .await;
                    return Ok(());
                }

                let _ = codex
                    .respond_to_approval(&event.call_id, decision)
                    .await;
            }

            // Forward all other events to subagent stream
            other => {
                let _ = tx_sub.send(other).await;
            }
        }
    }
    Ok(())
}
```

### Cancellation Propagation

```
User aborts parent session
│
├─ cancel_token.cancel()
│  └─ CancellationToken shared with all subagents
│
├─ forward_events() detects cancellation
│  └─ Sends ReviewDecision::Abort to subagent
│
├─ Subagent tool execution aborted
│  └─ Returns CodexErr::UserAborted
│
└─ Entire task tree cancelled
```

---

## Risk Assessment Flow

**Status:** Experimental feature

**Feature Flag:** `experimental_sandbox_command_assessment = true`

### Assessment Trigger Points

```
Sandbox denies command
│
└─ Feature enabled?
   ├─ NO → Skip assessment, go directly to approval
   └─ YES → Continue
      │
      ▼
   Build assessment prompt
      │
      ├─ Blocked command
      ├─ Sandbox policy details
      ├─ stderr from failed attempt
      └─ Context about what was being attempted
      │
      ▼
   Call LLM API
      │
      ├─ Model: claude-haiku (fast + cheap)
      ├─ Temperature: 0 (deterministic)
      ├─ Max tokens: 200
      └─ Timeout: 5 seconds
      │
      ▼
   Parse response
      │
      ├─ Extract risk level (Low/Medium/High)
      ├─ Extract description
      └─ Handle parsing errors → None
      │
      ▼
   Include in approval request
      │
      └─ Show to user in UI
```

### Assessment Prompt Template

**Location:** `codex-rs/core/templates/sandboxing/assessment_prompt.md`

```markdown
You are analyzing a command that was blocked by a sandbox.

Command: {command}
Working directory: {cwd}
Sandbox policy: {policy}

Failure output:
{stderr}

Assess the risk of running this command without sandbox protection.

Respond in JSON format:
{
  "risk_level": "Low" | "Medium" | "High",
  "description": "Brief explanation of what the command does and why it was blocked"
}

Risk levels:
- Low: Read-only operations, safe system commands
- Medium: File modifications, network access within workspace
- High: System modifications, credential access, network to untrusted hosts
```

### Implementation

```rust
// codex-rs/core/src/sandboxing/assessment.rs:25-89
pub async fn assess_command_risk(
    spec: &CommandSpec,
    policy: &SandboxPolicy,
    stderr: &str,
) -> Option<SandboxCommandAssessment> {
    let prompt = format_assessment_prompt(spec, policy, stderr);

    let timeout = Duration::from_secs(5);

    let response = tokio::time::timeout(timeout, async {
        services
            .llm_client
            .generate(GenerateRequest {
                model: "claude-haiku-3-5",
                prompt,
                temperature: 0.0,
                max_tokens: 200,
            })
            .await
    })
    .await
    .ok()??;

    parse_assessment_response(&response.text)
}

fn parse_assessment_response(text: &str) -> Option<SandboxCommandAssessment> {
    let json: serde_json::Value = serde_json::from_str(text).ok()?;

    let risk_level = match json["risk_level"].as_str()? {
        "Low" => SandboxRiskLevel::Low,
        "Medium" => SandboxRiskLevel::Medium,
        "High" => SandboxRiskLevel::High,
        _ => return None,
    };

    let description = json["description"].as_str()?.to_string();

    Some(SandboxCommandAssessment {
        risk_level,
        description,
    })
}
```

### UI Integration

**TUI:** `codex-rs/tui/src/bottom_pane/approval_overlay.rs`

```
Approval prompt shows:
┌─────────────────────────────────────────┐
│ Command blocked by sandbox              │
│                                         │
│ $ git push origin main                  │
│                                         │
│ Risk Assessment: HIGH                   │
│ This command attempts to push code to   │
│ a remote repository, which requires     │
│ network access and may expose sensitive │
│ code or credentials.                    │
│                                         │
│ Approve retry without sandbox?          │
│ [y] Yes  [n] No  [a] Abort             │
└─────────────────────────────────────────┘
```

---

## Error Handling

### Error Types

**Location:** `codex-rs/core/src/error.rs`

```rust
pub enum CodexErr {
    // Approval-related
    ApprovalDenied,        // User denied approval
    UserAborted,           // User aborted entire turn
    ApprovalTimeout,       // Approval request timed out

    // Sandbox-related
    SandboxRequired,       // Tool requires sandbox but unavailable
    SandboxUnavailable,    // Platform doesn't support sandboxing
    SandboxTransformError(SandboxTransformError),

    // Tool execution
    ToolNotFound(String),
    ToolExecutionFailed(String),

    // General
    Fatal(String),
}
```

### Error Propagation

```
ToolOrchestrator::run()
│
├─ Approval denied
│  └─ return Err(CodexErr::ApprovalDenied)
│     └─ Formatted as tool output to model
│        └─ Model sees: "User denied approval for this command"
│
├─ User aborted
│  └─ return Err(CodexErr::UserAborted)
│     └─ Entire turn cancelled
│        └─ Model stream stopped
│
├─ Sandbox transform failed
│  └─ return Err(CodexErr::SandboxTransformError)
│     └─ Logged to console
│        └─ Fallback to no sandbox (if allowed)
│
└─ Tool execution failed
   └─ return Ok(ToolOutput { exit_code: 1, stderr, ... })
      └─ Returned to model as failure
         └─ Model can retry or adjust
```

### Recovery Strategies

**Approval denied:**
- Return failure to model
- Model can rephrase request or explain why needed
- User can change approval policy and retry

**Sandbox unavailable:**
- Warn user in console
- Fallback to no sandbox (if policy allows)
- Tool with `Require` preference will error

**Sandbox denial:**
- Detect via platform-specific patterns
- Trigger retry flow with approval
- Include risk assessment if enabled

---

## Performance Considerations

### Caching Impact

**Approval cache:**
- In-memory HashMap lookup: ~O(1)
- Avoids UI rendering and user wait time
- Significant UX improvement for repetitive operations

**Risk assessment overhead:**
- 5 second timeout per blocked command
- Only when feature enabled
- Only on sandbox denials
- Async, doesn't block other operations

### Parallel Execution Impact

**Best case** (all tools parallel-safe):
- 3 read-only tools: ~1x time (concurrent)
- vs. 3x time (serial)

**Worst case** (mixed parallel/serial):
- Falls back to serial
- Write lock prevents concurrency
- Safe but slower

**Optimization:**
- Mark read-only tools as parallel-safe
- Implement internal synchronization where possible
- Consider tool-level caching

---

## Sequence Diagrams

### Complete Flow (ASCII)

```
User      TUI       Conversation   ToolRouter   Orchestrator   SandboxMgr   Tool      LLM
 │         │             │              │              │            │         │         │
 │ Submit  │             │              │              │            │         │         │
 ├────────>│             │              │              │            │         │         │
 │         │ Process     │              │              │            │         │         │
 │         ├────────────>│              │              │            │         │         │
 │         │             │ Send to LLM  │              │            │         │         │
 │         │             ├──────────────────────────────────────────────────>│         │
 │         │             │              │              │            │         │         │
 │         │             │<─────────────────────────────────────────────────┤         │
 │         │             │ Function call│              │            │         │         │
 │         │             ├─────────────>│              │            │         │         │
 │         │             │              │ Run tool     │            │         │         │
 │         │             │              ├─────────────>│            │         │         │
 │         │             │              │              │ Check approval      │         │
 │         │<─────────────────────────────────────────┤            │         │         │
 │ Approve?│             │              │              │            │         │         │
 ├────────>│             │              │              │            │         │         │
 │         ├──────────────────────────────────────────>│            │         │         │
 │         │             │              │              │ Select sandbox      │         │
 │         │             │              │              ├───────────>│         │         │
 │         │             │              │              │<──────────┤         │         │
 │         │             │              │              │ Transform  │         │         │
 │         │             │              │              ├───────────>│         │         │
 │         │             │              │              │<──────────┤         │         │
 │         │             │              │              │ Execute    │         │         │
 │         │             │              │              ├────────────────────>│         │
 │         │             │              │              │            │<───────┤         │
 │         │             │              │              │ Denied?    │         │         │
 │         │             │              │              ├───────────>│         │         │
 │         │             │              │              │<──────────┤         │         │
 │         │             │              │              │ Retry approval      │         │
 │         │<─────────────────────────────────────────┤            │         │         │
 │ Approve │             │              │              │            │         │         │
 ├────────>│             │              │              │            │         │         │
 │         ├──────────────────────────────────────────>│            │         │         │
 │         │             │              │              │ Retry (no sandbox)  │         │
 │         │             │              │              ├────────────────────>│         │
 │         │             │              │              │<───────────────────┤         │
 │         │             │              │<────────────┤            │         │         │
 │         │             │<────────────┤              │            │         │         │
 │         │             │ Return to LLM│             │            │         │         │
 │         │             ├──────────────────────────────────────────────────>│         │
 │         │             │              │              │            │         │         │
```

---

## Related Documentation

- **Architecture overview:** [`sandbox-approval-integration.md`](./sandbox-approval-integration.md)
- **User guide:** [`../sandbox.md`](../sandbox.md)
- **Best practices:** [`../guides/sandbox-approval-best-practices.md`](../guides/sandbox-approval-best-practices.md)
- **Config reference:** [`../config.md`](../config.md)

---

## Changelog

| Date | Author | Changes |
|------|--------|---------|
| 2025-11-10 | Initial | Created detailed orchestration flow documentation |
