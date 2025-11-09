# Sandbox & Approval Integration Architecture

**Status:** Living document
**Last Updated:** 2025-11-10
**Audience:** Codex developers and contributors

## Overview

This document describes the internal architecture of how Codex's **sandbox permission system** and **approval workflow system** integrate to provide secure, user-controlled AI agent execution. For user-facing documentation, see [`docs/sandbox.md`](../sandbox.md).

### Key Design Principles

1. **Separation of Concerns**: Sandbox and approval are independent systems that coordinate
2. **Defense in Depth**: Multiple layers of protection (approval gates + OS-level enforcement)
3. **Progressive Trust**: Start conservative, user explicitly grants more autonomy
4. **Platform Abstraction**: Unified API across macOS, Linux, Windows

---

## System Components

### 1. Approval System

**Purpose:** User permission and trust management

**Core Files:**
- `codex-rs/protocol/src/approvals.rs` - Event definitions
- `codex-rs/core/src/tools/sandboxing.rs` - Approval store and traits
- `codex-rs/core/src/tools/orchestrator.rs` - Approval orchestration
- `codex-rs/tui/src/bottom_pane/approval_overlay.rs` - User interface

**Key Components:**

```
┌─────────────────────────────────────────┐
│       Approval Policy (Config)          │
│  - UnlessTrusted / OnFailure /          │
│    OnRequest / Never                    │
└───────────────┬─────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────┐
│         Approvable Trait                │
│  - wants_initial_approval()             │
│  - wants_no_sandbox_approval()          │
│  - start_approval_async()               │
└───────────────┬─────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────┐
│        Approval Store (Cache)           │
│  HashMap<Key, ReviewDecision>           │
│  - ApprovedForSession cached            │
└───────────────┬─────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────┐
│       Review Decision Events            │
│  - Approved                             │
│  - ApprovedForSession                   │
│  - Denied / Abort                       │
└─────────────────────────────────────────┘
```

### 2. Sandbox System

**Purpose:** OS-level permission enforcement

**Core Files:**
- `codex-rs/core/src/sandboxing/mod.rs` - Sandbox manager
- `codex-rs/core/src/seatbelt.rs` - macOS Seatbelt
- `codex-rs/core/src/landlock.rs` - Linux Landlock
- `codex-rs/protocol/src/protocol.rs` - Policy definitions

**Key Components:**

```
┌─────────────────────────────────────────┐
│      Sandbox Policy (Config)            │
│  - ReadOnly / WorkspaceWrite /          │
│    DangerFullAccess                     │
└───────────────┬─────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────┐
│        SandboxManager                   │
│  - select_initial()                     │
│  - transform()                          │
│  - denied()                             │
└───────────────┬─────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────┐
│         SandboxType (Platform)          │
│  - None / MacosSeatbelt /               │
│    LinuxSeccomp / WindowsRestrictedToken│
└───────────────┬─────────────────────────┘
                │
                ▼
┌─────────────────────────────────────────┐
│        ExecEnv (Transformed)            │
│  - Wrapped command                      │
│  - Environment variables                │
│  - CODEX_SANDBOX_*                      │
└─────────────────────────────────────────┘
```

### 3. Integration Layer

**Purpose:** Coordinates approval and sandbox during tool execution

**Core File:** `codex-rs/core/src/tools/orchestrator.rs`

**ToolOrchestrator** is the central coordination point that:
1. Checks approval policy and requests user permission
2. Selects appropriate sandbox based on policy and platform
3. Transforms commands to run within sandbox
4. Handles sandbox denials and retry logic
5. Routes approval requests for subagents

---

## Integration Architecture

### High-Level Flow

```
┌──────────────┐
│  User Turn   │
│  (Request)   │
└──────┬───────┘
       │
       ▼
┌──────────────────────────────────────┐
│     ToolOrchestrator.run()           │
│  (Integration Coordinator)           │
└──────┬───────────────────────────────┘
       │
       │  ┌────────────────────────────────────┐
       ├─→│ 1. APPROVAL STAGE                  │
       │  │  - Check approval policy           │
       │  │  - Run risk assessment (optional)  │
       │  │  - Request user approval           │
       │  │  - Cache decision                  │
       │  └────────────────────────────────────┘
       │
       │  ┌────────────────────────────────────┐
       ├─→│ 2. SANDBOX SELECTION               │
       │  │  - SandboxManager.select_initial() │
       │  │  - Check tool preference           │
       │  │  - Platform capability detection   │
       │  └────────────────────────────────────┘
       │
       │  ┌────────────────────────────────────┐
       ├─→│ 3. COMMAND TRANSFORMATION          │
       │  │  - Build CommandSpec               │
       │  │  - SandboxManager.transform()      │
       │  │  - Add CODEX_SANDBOX_* env vars    │
       │  │  - Wrap with sandbox launcher      │
       │  └────────────────────────────────────┘
       │
       │  ┌────────────────────────────────────┐
       ├─→│ 4. EXECUTION                       │
       │  │  - spawn_child_async()             │
       │  │  - Set process group               │
       │  │  - Capture output                  │
       │  └────────┬───────────────────────────┘
       │           │
       │           ▼
       │  ┌────────────────────────────────────┐
       └─→│ 5. RETRY ON DENIAL (if needed)     │
          │  - Detect sandbox denial           │
          │  - Check retry policy              │
          │  - Request approval for no-sandbox │
          │  - Re-run without sandbox          │
          └────────────────────────────────────┘
```

### Configuration Matrix

How sandbox modes and approval policies interact:

| Sandbox Mode | Approval Policy | Typical Use Case | Behavior |
|--------------|----------------|------------------|----------|
| `ReadOnly` | `UnlessTrusted` | Untrusted code exploration | Every command requires approval; safe commands auto-approved |
| `ReadOnly` | `OnRequest` | Safe browsing | Read-only; model decides when to ask |
| `ReadOnly` | `Never` | CI/CD read-only analysis | No writes; no prompts; failures returned to model |
| `WorkspaceWrite` | `UnlessTrusted` | Conservative trusted repo | Workspace edits require approval per command |
| `WorkspaceWrite` | `OnRequest` | **Default trusted repo** | Model iterates freely in workspace; asks to leave |
| `WorkspaceWrite` | `OnFailure` | Automated development | Everything auto-approved; escalate on sandbox denial |
| `DangerFullAccess` | `OnRequest` | Model decides approval | No sandbox; model still can request approval |
| `DangerFullAccess` | `Never` | **YOLO mode** | No sandbox; no approvals; maximum autonomy |

**Key Insights:**

1. **Approval policy is checked first**: Even with `DangerFullAccess` sandbox, approval can still gate actions
2. **`OnRequest` with `DangerFullAccess` = silent execution**: Model won't ask because there's no sandbox to escalate from
3. **`UnlessTrusted` only works with command safety analysis**: Uses `is_safe_command()` to auto-approve known-safe operations
4. **`OnFailure` requires sandbox**: If sandbox unavailable, behaves like `OnRequest`

---

## Approval Decision Flow

### Decision Tree

```
Tool Invocation
│
├─ Is approval policy "Never"?
│  ├─ YES → Skip approval, proceed to execution
│  └─ NO → Continue
│
├─ Is sandbox policy "DangerFullAccess"?
│  ├─ YES → Skip approval (no sandbox to escape from)
│  └─ NO → Continue
│
├─ Check approval cache (ApprovalStore)
│  ├─ Found "ApprovedForSession"? → Skip approval
│  └─ Not found → Continue
│
├─ Is tool requesting escalated permissions?
│  │  (wants_escalated_first_attempt = true)
│  ├─ YES → Request approval (reason: "escalated permissions")
│  └─ NO → Continue
│
├─ Is approval policy "UnlessTrusted"?
│  ├─ YES → Is command safe? (is_safe_command check)
│  │  ├─ YES → Auto-approve
│  │  └─ NO → Request approval
│  └─ NO → Continue
│
├─ Is approval policy "OnRequest"?
│  ├─ YES → Model decides (usually skips if sandbox active)
│  └─ NO → Continue
│
└─ Is approval policy "OnFailure"?
   └─ YES → Auto-approve first attempt
       └─ On sandbox denial → Request approval for retry
```

### Approval Caching

**Location:** `codex-rs/core/src/tools/sandboxing.rs:29-78`

```rust
pub struct ApprovalStore {
    cache: Arc<RwLock<HashMap<String, ReviewDecision>>>,
}

// Only ApprovedForSession is cached
if decision == ReviewDecision::ApprovedForSession {
    store.set(key, decision).await;
}
```

**Serialization Key Format:**
- Exec tool: `format!("exec:{}", command_str)`
- Patch tool: `format!("patch:{}", file_path)`

**Cache Lifetime:**
- Session-scoped (in-memory only)
- Cleared on session restart
- Not persisted to disk

**Cache Bypass:**
- `Approved` (one-time) is not cached
- `Denied` is not cached
- Cache check skipped if approval policy is `Never`

---

## Sandbox Selection Logic

### Selection Algorithm

**Location:** `codex-rs/core/src/sandboxing/mod.rs:68-110`

```rust
pub fn select_initial(
    &self,
    policy: &SandboxPolicy,
    pref: SandboxablePreference,
) -> SandboxType
```

**Decision tree:**

```
Policy = DangerFullAccess?
├─ YES → return SandboxType::None
└─ NO → Continue

Tool preference = Forbid?
├─ YES → return SandboxType::None
└─ NO → Continue

Platform = macOS?
├─ YES → Is sandbox-exec available?
│  ├─ YES → return MacosSeatbelt
│  └─ NO → fallback to None
└─ Platform = Linux?
    ├─ YES → Is Landlock supported?
    │  ├─ YES → return LinuxSeccomp
    │  └─ NO → fallback to None
    └─ Platform = Windows?
        ├─ YES → Is experimental feature enabled?
        │  ├─ YES → return WindowsRestrictedToken
        │  └─ NO → return None
        └─ NO → return None

Tool preference = Require?
├─ If SandboxType::None → ERROR (sandbox required but unavailable)
└─ Otherwise → return selected SandboxType
```

### Tool Sandbox Preferences

**Location:** `codex-rs/core/src/tools/sandboxing.rs:139-153`

Tools declare their sandboxing preference:

```rust
pub enum SandboxablePreference {
    Auto,     // Use sandbox if available (default)
    Require,  // Must have sandbox or error
    Forbid,   // Never use sandbox (e.g., repl_tool needs direct access)
}
```

**Examples:**

- **Exec tool**: `Auto` - prefers sandbox but can run without
- **Repl tool**: `Forbid` - needs direct terminal access
- **Future security-critical tools**: `Require` - refuse to run unsandboxed

---

## Command Transformation

### Transformation Process

**Location:** `codex-rs/core/src/sandboxing/mod.rs:112-153`

```rust
pub fn transform(
    &self,
    spec: &CommandSpec,
    policy: &SandboxPolicy,
    sandbox: SandboxType,
    turn_context: &TurnContext,
) -> Result<ExecEnv, SandboxTransformError>
```

**Steps:**

1. **Build base ExecEnv** from CommandSpec
   - Program path
   - Arguments
   - Working directory
   - Base environment variables
   - Timeout

2. **Add sandbox environment variables**
   - `CODEX_SANDBOX_NETWORK_DISABLED=1` if `!policy.has_full_network_access()`
   - `CODEX_SANDBOX=seatbelt` on macOS when using Seatbelt

3. **Platform-specific transformation**

   **macOS (Seatbelt):**
   ```rust
   // codex-rs/core/src/seatbelt.rs:130-250
   create_seatbelt_command_args(policy, spec) -> Vec<String>
   ```
   - Wrap command: `/usr/bin/sandbox-exec -p <profile> <command>`
   - Generate Scheme policy profile
   - Canonicalize paths (handle `/var` vs `/private/var` symlinks)
   - Add writable root parameters
   - Exclude `.git/` subdirectories from write access
   - Apply network policy from `seatbelt_network_policy.sbpl`

   **Linux (Landlock):**
   ```rust
   // codex-rs/core/src/landlock.rs
   // Invokes codex-linux-sandbox binary
   ```
   - Wrap command: `codex-linux-sandbox <policy-json> -- <command>`
   - Serialize policy to JSON
   - Binary uses Landlock + seccomp APIs
   - Graceful fallback if kernel doesn't support

   **Windows (Experimental):**
   ```rust
   // Platform-specific restricted token creation
   ```
   - Derives restricted token from AppContainer
   - Grants filesystem capabilities via SIDs
   - Overrides proxy environment variables
   - Inserts stub executables for network tools

4. **Return transformed ExecEnv**

### Writable Root Processing

**Location:** `codex-rs/protocol/src/protocol.rs:286-310`

```rust
pub struct WritableRoot {
    pub path: PathBuf,
    pub read_only_subpaths: Vec<PathBuf>,
}

impl WritableRoot {
    pub fn is_path_writable(&self, path: &Path) -> bool {
        // Path must be under writable root
        // AND not under any read-only subpath
    }
}
```

**Default writable roots in `WorkspaceWrite` mode:**

```rust
// codex-rs/core/src/sandboxing/mod.rs
let mut writable_roots = vec![
    WritableRoot {
        path: turn_context.cwd.clone(),
        read_only_subpaths: find_git_dirs(&turn_context.cwd),
    },
];

if !policy.exclude_slash_tmp {
    writable_roots.push(WritableRoot {
        path: PathBuf::from("/tmp"),
        read_only_subpaths: vec![],
    });
}

if !policy.exclude_tmpdir_env_var {
    if let Some(tmpdir) = env::var("TMPDIR").ok() {
        writable_roots.push(WritableRoot {
            path: PathBuf::from(tmpdir),
            read_only_subpaths: vec![],
        });
    }
}

writable_roots.extend(policy.writable_roots.clone());
```

**Git directory protection:**

```rust
// .git directories are ALWAYS read-only, even in workspace-write mode
// Prevents accidental git push, git commit --amend, etc.
fn find_git_dirs(root: &Path) -> Vec<PathBuf> {
    // Returns all .git/ directories under root
}
```

---

## Sandbox Denial Detection & Retry

### Denial Detection

**Location:** `codex-rs/core/src/sandboxing/mod.rs:155-170`

```rust
pub fn denied(
    &self,
    sandbox: SandboxType,
    out: &ExecToolCallOutput,
) -> bool
```

**Platform-specific denial patterns:**

**macOS Seatbelt:**
- Exit code: Non-zero (typically 1)
- stderr contains: `"sandbox-exec: denied"` or `"Operation not permitted"`

**Linux Landlock:**
- Exit code: 1
- stderr contains: `"Permission denied"` or `"landlock"`

**Windows:**
- Exit code: 5 (ERROR_ACCESS_DENIED)
- stderr contains: `"Access is denied"`

### Retry Logic

**Location:** `codex-rs/core/src/tools/orchestrator.rs:150-187`

```rust
// First attempt failed due to sandbox
if sandbox_manager.denied(sandbox_type, &output) {
    // Check if retry allowed
    if tool.wants_no_sandbox_approval(approval_policy) {
        // Run risk assessment (if enabled)
        let risk = assess_sandbox_command_risk(...).await;

        // Request user approval with context
        let decision = request_approval(
            reason: "Command blocked by sandbox. Retry without sandbox?",
            risk,
            failed_output: output.stderr,
        ).await;

        if decision.is_approved() {
            // Retry without sandbox
            let no_sandbox_env = sandbox_manager.transform(
                spec,
                policy,
                SandboxType::None,  // Force no sandbox
                turn_context,
            )?;

            return tool.run(no_sandbox_env).await;
        }
    }
}
```

**Retry decision matrix:**

| Approval Policy | Sandbox Denied | Retry Behavior |
|----------------|----------------|----------------|
| `Never` | Yes | Return failure to model (no retry) |
| `OnFailure` | Yes | Auto-approve retry without sandbox |
| `OnRequest` | Yes | Request approval for retry |
| `UnlessTrusted` | Yes | Request approval for retry |

---

## Subagent Approval Delegation

### Architecture

Subagents (Review, Compact, custom) are nested Codex conversations that delegate approval decisions to the parent session. This enables hierarchical workflows while maintaining a single approval UI.

**Location:** `codex-rs/core/src/codex_delegate.rs`

### Flow Diagram

```
┌─────────────────────────────┐
│   Parent Session (User)     │
│   - approval_policy         │
│   - sandbox_policy          │
└──────────┬──────────────────┘
           │
           │ spawns
           ▼
┌─────────────────────────────┐
│  SubAgent (Review/Compact)  │
│  - Isolated conversation    │
│  - Inherits policies        │
└──────────┬──────────────────┘
           │
           │ encounters approval request
           ▼
┌─────────────────────────────┐
│  forward_events() Router    │
│  - Intercepts approval      │
│  - Routes to parent         │
└──────────┬──────────────────┘
           │
           ▼
┌─────────────────────────────┐
│  Parent Session Approver    │
│  - parent_session           │
│    .request_command_approval│
│  - Shows UI to user         │
└──────────┬──────────────────┘
           │
           │ decision
           ▼
┌─────────────────────────────┐
│  codex.respond_to_approval  │
│  - Sends decision to sub    │
└──────────┬──────────────────┘
           │
           ▼
┌─────────────────────────────┐
│  SubAgent Continues         │
│  - Approved or Denied       │
└─────────────────────────────┘
```

### Implementation

**Approval event interception:**

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
            Event {
                msg: EventMsg::ExecApprovalRequest(event),
                ..
            } => {
                handle_exec_approval(
                    &codex,
                    id,
                    &parent_session,
                    &parent_ctx,
                    event,
                    &cancel_token,
                )
                .await;
            }
            Event {
                msg: EventMsg::ApplyPatchApprovalRequest(event),
                ..
            } => {
                handle_patch_approval(
                    &codex,
                    id,
                    &parent_session,
                    &parent_ctx,
                    event,
                    &cancel_token,
                )
                .await;
            }
            other => {
                // Forward other events to subagent event stream
                let _ = tx_sub.send(other).await;
            }
        }
    }
    Ok(())
}
```

**Exec approval delegation:**

```rust
// codex-rs/core/src/codex_delegate.rs:206-245
async fn handle_exec_approval(
    codex: &CodexConversation,
    id: &ConversationId,
    parent_session: &SessionHandle,
    parent_ctx: &Context,
    event: ExecApprovalRequestEvent,
    cancel_token: &CancellationToken,
) {
    // Request approval from parent session's UI
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

    // Handle cancellation
    if cancel_token.is_cancelled() {
        let _ = codex
            .respond_to_approval(&event.call_id, ReviewDecision::Abort)
            .await;
        return;
    }

    // Send decision back to subagent
    let _ = codex
        .respond_to_approval(&event.call_id, decision)
        .await;
}
```

**Key characteristics:**

1. **Isolated context**: Subagent has its own conversation history and state
2. **Shared policies**: Inherits sandbox and approval policies from parent
3. **Transparent to model**: Model doesn't know it's in a subagent
4. **Single approval UI**: User only interacts with parent session
5. **Cancellation propagation**: Aborting parent cancels all subagents

### Subagent Types

**Location:** `codex-rs/protocol/src/protocol.rs:151-159`

```rust
pub enum SessionSource {
    Interactive,
    Exec,
    SubAgent(SubAgentSource),
}

pub enum SubAgentSource {
    Review,
    Compact,
    Other(String),
}
```

**HTTP header for model provider:**

```rust
// Automatically added to LLM API requests
headers.insert("x-openai-subagent", subagent_type);
// e.g., "x-openai-subagent: review"
```

---

## Environment Variables

### Read-Only Environment Variables

**NEVER modify these** - they are set by Codex and used for detection:

| Variable | Value | Location Set | Purpose |
|----------|-------|--------------|---------|
| `CODEX_SANDBOX_NETWORK_DISABLED` | `"1"` | `core/src/spawn.rs:18` | Signal to child processes that network is blocked |
| `CODEX_SANDBOX` | `"seatbelt"` | `core/src/spawn.rs:23` | Indicates which sandbox mechanism is active |

**Usage in child processes:**

```rust
// core/src/safety.rs:12-25
pub fn is_network_disabled() -> bool {
    env::var("CODEX_SANDBOX_NETWORK_DISABLED")
        .map(|v| v == "1")
        .unwrap_or(false)
}

pub fn is_sandboxed() -> bool {
    env::var("CODEX_SANDBOX").is_ok()
}
```

**Test early-exit pattern:**

```rust
#[test]
fn test_network_operation() {
    if is_network_disabled() {
        // Skip test when sandboxed
        return;
    }
    // ... test code
}
```

---

## Risk Assessment System

**Status:** Experimental (feature flag: `experimental_sandbox_command_assessment`)

**Purpose:** Provide AI-powered risk analysis of blocked commands to help users make informed approval decisions.

### Architecture

**Location:** `codex-rs/core/src/sandboxing/assessment.rs`

```rust
pub struct SandboxCommandAssessment {
    pub description: String,
    pub risk_level: SandboxRiskLevel,
}

pub enum SandboxRiskLevel {
    Low,
    Medium,
    High,
}
```

### Assessment Flow

```
Sandbox blocks command
│
├─ Feature enabled?
│  └─ NO → Skip assessment
│
├─ Build assessment prompt
│  ├─ Command that was blocked
│  ├─ Sandbox policy
│  ├─ stderr from failed attempt
│  └─ Template: templates/sandboxing/assessment_prompt.md
│
├─ Call LLM API with 5s timeout
│  ├─ Model: haiku (fast + cheap)
│  ├─ Response format: JSON
│  └─ Parse risk level + description
│
├─ Include in approval request
│  └─ Show to user in approval UI
│
└─ User makes decision with context
```

**Prompt template location:** `codex-rs/core/templates/sandboxing/assessment_prompt.md`

**Timeout handling:**
```rust
// 5 second timeout for risk assessment
tokio::time::timeout(
    Duration::from_secs(5),
    assess_risk(command, policy, failure_output),
)
.await
.unwrap_or(None)  // Fall back to no assessment if timeout
```

---

## Configuration Integration

### Config File Structure

**Location:** User's `~/.codex/config.toml` or project `.codex/config.toml`

```toml
# Top-level approval and sandbox settings
approval_policy = "on-request"  # untrusted | on-failure | on-request | never
sandbox_mode = "workspace-write"  # read-only | workspace-write | danger-full-access

# Workspace-write specific configuration
[sandbox_workspace_write]
writable_roots = [
    "/home/user/.cache",
    "/home/user/.local/share"
]
network_access = true
exclude_tmpdir_env_var = false
exclude_slash_tmp = false

# Project-specific trust
[projects."/home/user/myrepo"]
trust_level = "trusted"  # Changes default preset from read-only to auto

# Named profiles
[profiles.paranoid]
approval_policy = "untrusted"
sandbox_mode = "read-only"

[profiles.paranoid.shell_environment_policy]
inherit = "core"
exclude = ["*TOKEN*", "*KEY*", "*SECRET*"]

[profiles.ci]
approval_policy = "never"
sandbox_mode = "read-only"

# Feature flags
[features]
experimental_sandbox_command_assessment = true
enable_experimental_windows_sandbox = false
```

### Loading Priority

**Location:** `codex-rs/core/src/config/mod.rs`

Configuration is resolved in this order (later overrides earlier):

1. **Built-in defaults** (`approval_presets.rs`)
   - Untrusted directory → "read-only" preset
   - Trusted directory → "auto" preset

2. **Global config** (`~/.codex/config.toml`)
   - User's global preferences

3. **Project config** (`.codex/config.toml` in repo root)
   - Project-specific overrides

4. **Command-line flags**
   - `--sandbox`, `--ask-for-approval`, `--profile`
   - Highest priority

5. **Onboarding prompts** (TUI only)
   - Trust directory prompt
   - Writes to project config

### Trust Resolution

**Location:** `codex-rs/core/src/config/mod.rs:385-461`

```rust
pub fn is_trusted_directory(&self, path: &Path) -> bool {
    // 1. Resolve git repository root if applicable
    let trust_path = if is_git_repo(path) {
        find_git_root(path)
    } else {
        path
    };

    // 2. Check projects config
    self.projects
        .get(trust_path)
        .map(|p| p.trust_level == TrustLevel::Trusted)
        .unwrap_or(false)
}
```

**Git repository trust behavior:**
- Trusting any path within a git repo → Trusts entire repo
- Trust is stored for the repo root (not the subdirectory)
- Enables consistent behavior regardless of `cd` location

---

## Tool Handler Integration

### Tool Handler Lifecycle

All tool handlers implement this interface and go through the orchestrator:

```rust
// codex-rs/tools/src/lib.rs
#[async_trait]
pub trait ToolHandler: Send + Sync {
    async fn run(&self, invocation: ToolInvocation) -> Result<ToolOutput>;
}
```

### Exec Tool Example

The exec tool is the primary user of both approval and sandbox systems:

**Location:** `codex-rs/tools/exec/src/lib.rs`

**Integration points:**

1. **Implements Approvable trait** (`core/src/tools/sandboxing.rs`)
   ```rust
   impl Approvable<ExecRequest> for ExecTool {
       type ApprovalKey = String;

       fn approval_key(&self, req: &ExecRequest) -> String {
           format!("exec:{}", req.command_str())
       }

       fn wants_initial_approval(...) -> bool {
           // Check if command needs approval before first attempt
       }

       fn start_approval_async(...) -> ReviewDecision {
           // Request approval from user
       }
   }
   ```

2. **Declares sandbox preference**
   ```rust
   fn sandbox_preference(&self) -> SandboxablePreference {
       SandboxablePreference::Auto
   }
   ```

3. **Invoked through orchestrator**
   ```rust
   // codex-rs/core/src/tools/orchestrator.rs
   let output = ToolOrchestrator::run(
       tool,
       request,
       approval_policy,
       sandbox_policy,
       services,
   ).await?;
   ```

4. **Receives transformed ExecEnv**
   ```rust
   async fn run(&self, invocation: ToolInvocation) -> Result<ToolOutput> {
       let env: ExecEnv = invocation.exec_env;
       // env.program may be wrapped with sandbox-exec
       // env.environment contains CODEX_SANDBOX_* vars
       spawn_and_execute(env).await
   }
   ```

### Patch Tool Example

The patch tool (apply file changes) also uses approval system:

**Location:** `codex-rs/tools/patch/src/lib.rs`

**Integration points:**

1. **Implements Approvable trait** for `ApplyPatchRequest`
   ```rust
   fn approval_key(&self, req: &ApplyPatchRequest) -> String {
       // Serialize changed file paths
       format!("patch:{}", req.files.join(","))
   }

   fn wants_initial_approval(...) -> bool {
       // Always check for patch approval unless policy is Never
       approval_policy != AskForApproval::Never
   }
   ```

2. **No sandbox interaction** (file writes handled by Codex directly, not shell)

3. **Approval includes grant_root option**
   ```rust
   // User can approve entire directory for session
   if decision.grant_root.is_some() {
       services.approval_store.set_root_grant(grant_root).await;
   }
   ```

---

## Best Practices for Developers

### Adding a New Tool

When implementing a new tool handler:

1. **Determine if tool needs approval**
   - Reading data: Usually no
   - Writing data or executing commands: Usually yes

2. **Declare sandbox preference**
   ```rust
   fn sandbox_preference(&self) -> SandboxablePreference {
       Auto    // Most tools
       Require // Security-critical tools
       Forbid  // Tools needing direct access (REPL, terminal)
   }
   ```

3. **Implement Approvable trait if needed**
   ```rust
   impl Approvable<MyRequest> for MyTool {
       type ApprovalKey = String;

       fn approval_key(&self, req: &MyRequest) -> String {
           // Unique key for caching
       }

       fn wants_initial_approval(...) -> bool {
           // When to ask before execution
       }

       fn start_approval_async(...) -> ReviewDecision {
           // Trigger approval flow
       }
   }
   ```

4. **Use ToolOrchestrator**
   ```rust
   let output = ToolOrchestrator::run(
       tool,
       request,
       ctx.approval_policy,
       ctx.sandbox_policy,
       &services,
   ).await?;
   ```

### Testing Sandbox Integration

**Unit tests:**
```rust
#[test]
fn test_sandbox_selection() {
    let manager = SandboxManager::new();
    let policy = SandboxPolicy::WorkspaceWrite { /* ... */ };
    let pref = SandboxablePreference::Auto;

    let sandbox = manager.select_initial(&policy, pref);

    #[cfg(target_os = "macos")]
    assert_eq!(sandbox, SandboxType::MacosSeatbelt);
}
```

**Integration tests:**
```rust
// Skip when sandboxed to avoid nested sandbox issues
#[test]
fn test_network_access() {
    if is_sandboxed() || is_network_disabled() {
        return;
    }

    // Test code
}
```

**Manual testing:**
```bash
# Test specific sandbox mode
codex sandbox macos --full-auto ls -la

# Test with approval policy
codex --sandbox workspace-write --ask-for-approval untrusted
```

### Common Pitfalls

1. **Forgetting Send + Sync bounds** on tool handlers
   - Error: Handlers stored in `Arc<dyn ToolHandler>` require `Send + Sync`
   - Solution: Add bounds to trait and all implementations

2. **Path canonicalization on macOS**
   - Error: `/var` vs `/private/var` symlink confusion
   - Solution: Use `canonicalize()` before passing to Seatbelt

3. **Not checking sandbox_denied() after execution**
   - Error: Returning generic error instead of triggering retry
   - Solution: Always call `sandbox_manager.denied(sandbox, output)`

4. **Caching Approved instead of ApprovedForSession**
   - Error: Re-prompting every time
   - Solution: Only cache `ReviewDecision::ApprovedForSession`

5. **Bypassing orchestrator**
   - Error: Running tools directly without approval/sandbox
   - Solution: Always use `ToolOrchestrator::run()`

---

## Related Documentation

- **User guide:** [`docs/sandbox.md`](../sandbox.md)
- **Orchestration flow:** [`docs/architecture/tool-execution-orchestration.md`](./tool-execution-orchestration.md)
- **Best practices:** [`docs/guides/sandbox-approval-best-practices.md`](../guides/sandbox-approval-best-practices.md)
- **Platform details:** [`docs/platform-sandboxing.md`](../platform-sandboxing.md)
- **Config reference:** [`docs/config.md`](../config.md)

---

## Changelog

| Date | Author | Changes |
|------|--------|---------|
| 2025-11-10 | Initial | Created comprehensive architecture documentation |
