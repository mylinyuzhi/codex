# coco-permissions — Crate Plan

TS source: `src/utils/permissions/` (26 files), `src/types/permissions.ts`

## Dependencies

```
coco-permissions depends on:
  - coco-types    (PermissionMode, PermissionBehavior, PermissionRule, PermissionRuleSource,
                   PermissionDecision, ToolPermissionContext)
  - coco-config   (Settings — for reading permission rules from settings layers)
  - coco-inference (ApiClient — for yolo/auto-mode classifier LLM calls)
  - coco-error
  - regex         (dangerous pattern matching)

coco-permissions does NOT depend on:
  - coco-tool     (no Tool trait — receives tool name + input as parameters)
  - coco-shell    (no shell awareness — bash command classification is internal)
  - any app/ crate
```

## Modules

```
coco-permissions/src/
  evaluate.rs            # Core permission evaluation pipeline (from permissions.ts, 1486 LOC)
  shell_rules.rs         # Shell rule parsing and matching (from shellRuleMatching.ts)
  dangerous_patterns.rs  # Code-exec pattern detection (from dangerousPatterns.ts)
  auto_mode.rs           # Auto-mode state machine (from autoModeState.ts, 39 LOC)
  classifier.rs          # Two-stage yolo/auto-mode classifier (from yoloClassifier.ts, 1495 LOC)
  classifier_decision.rs # Classifier → PermissionDecision integration (from classifierDecision.ts, 98 LOC)
  classifier_shared.rs   # Safe-tool allowlist, shared utils (from classifierShared.ts)
  denial_tracking.rs     # Denial state machine, fallback triggers (from denialTracking.ts, 45 LOC)
  loader.rs              # Load/persist permission rules from disk (from permissionsLoader.ts)
  setup.rs               # Permission context initialization (from permissionSetup.ts)
  context.rs             # PermissionContext wrapper (from PermissionContext.ts, 388 LOC)
  bash_classifier.rs     # Semantic bash classifier (from bashClassifier.ts — stub for external builds)
```

## Data Definitions

### Permission Types (from `types/permissions.ts`)

```rust
// PermissionMode, PermissionBehavior, PermissionRule, PermissionRuleSource
// PermissionDecision — all defined in coco-types, not here.

/// Classifier result for bash command analysis
pub struct ClassifierResult {
    pub matches: bool,
    pub matched_description: Option<String>,
    pub confidence: ClassifierConfidence,
    pub reason: String,
}

pub enum ClassifierConfidence { High, Medium, Low }

/// Auto-mode user configuration — type owned by coco-config (part of Settings).
/// See `crate-coco-config.md` § AutoModeConfig. Referenced here, not redefined.

/// Yolo/auto-mode classifier result
pub struct YoloClassifierResult {
    pub thinking: Option<String>,
    pub should_block: bool,
    pub reason: String,
    pub unavailable: bool,
    pub transcript_too_long: bool,
    pub model: String,
    pub usage: Option<ClassifierUsage>,
    pub duration_ms: Option<i64>,
    pub stage: Option<ClassifierStage>,
    pub stage1_usage: Option<ClassifierUsage>,
    pub stage1_duration_ms: Option<i64>,
}

pub enum ClassifierStage { Fast, Thinking }

/// Denial tracking state (session-scoped, not persisted)
pub struct DenialTrackingState {
    pub consecutive_denials: i32,
    pub total_denials: i32,
}

/// Decision reason — see `crate-coco-types.md` § PermissionDecisionReason.
/// Not redefined here per CLAUDE.md single-source rule.
/// Key variants used by this crate: Rule, Mode, Classifier, Hook, SafetyCheck.
```

## Core Logic

### Permission Evaluation Pipeline (from `permissions.ts`, 1486 LOC)

```rust
/// Evaluate permission for a tool call. Order:
/// 1. Tool-level deny rules → Deny immediately (highest priority)
/// 2. Tool-level allow rules → Allow immediately
/// 3. Content-specific rules (for Bash/PowerShell):
///    - Exact: "Bash(exact:ls -la)" → exact command match
///    - Prefix: "Bash(prefix:git *)" → commands starting with "git "
///    - Wildcard: "git *" → regex matching with escape support
///    - Command parsing splits compound commands (&&, ||, |)
/// 4. MCP tool rules: "mcp__server1" matches all tools from server1
/// 5. Mode-based fallthrough:
///    - default → ask user
///    - dontAsk → deny (convert ask to deny)
///    - bypassPermissions → allow all
///    - acceptEdits → fast-path file ops in CWD
///    - auto/plan → run classifier
/// Never fails — always returns a decision (Allow, Ask, or Deny).
/// Invalid inputs (empty tool_name, malformed Value) produce Deny with reason.
pub fn evaluate_permission(
    tool_name: &str,
    input: &Value,
    context: &ToolPermissionContext,
) -> PermissionDecision;

/// Rule matching for Bash/PowerShell tools:
pub fn matches_bash_rule(rule_content: &str, command: &str) -> bool;
```

### Shell Rule Matching (from `shellRuleMatching.ts`)

```rust
pub enum ShellPermissionRule {
    Exact { command: String },
    Prefix { prefix: String },
    Wildcard { pattern: String },
}

pub fn parse_permission_rule(rule: &str) -> ShellPermissionRule;

/// Wildcard escape handling:
/// - \* → literal asterisk (becomes \* in regex)
/// - \\ → literal backslash
/// - "git *" → regex "git( .*)?" (trailing space-args optional)
/// - "." → escaped to "\." (prevent regex dot-all)
pub fn match_wildcard_pattern(pattern: &str, command: &str, case_insensitive: bool) -> bool;
```

### Permission Rule Sources (priority high to low)

```rust
/// session > command > cliArg > flagSettings > localSettings > projectSettings > userSettings > policySettings
pub fn get_effective_rules(context: &ToolPermissionContext) -> Vec<PermissionRule>;

pub fn load_permission_rules_from_disk() -> Vec<PermissionRule>;
pub fn get_rules_for_source(source: SettingSource) -> Vec<PermissionRule>;
pub fn add_permission_rule(source: SettingSource, tool_name: &str, content: Option<&str>, behavior: PermissionBehavior);
pub fn delete_permission_rule(rule: &PermissionRule) -> bool;
```

### MCP Tool Permission Matching

```rust
/// Server-level: rule "mcp__server1" matches "mcp__server1__tool1"
/// Wildcard: rule "mcp__server1__*" matches all tools from server1
pub fn get_tool_name_for_permission_check(tool_name: &str) -> &str;
```

## Auto-Mode / Yolo Classifier (from `yoloClassifier.ts`, 1495 LOC)

### Two-Stage XML Classifier

The auto-mode classifier uses a two-stage LLM call to classify tool calls:

```rust
/// Stage 1: FAST (64 or 256 tokens max)
/// - Nudged with suffix: "Err on the side of blocking. <block> immediately."
/// - Uses stop_sequences: ['</block>'] for quick decision
/// - Returns immediately if result is false (allow)
/// - If result is true or unparseable → escalate to Stage 2
///
/// Stage 2: THINKING (4096+ tokens)
/// - Full chain-of-thought reasoning
/// - Nudged: "Review the classification process...explicit user confirmation required..."
/// - Parses <thinking>, <block>yes/no</block>, <reason> XML tags
/// - Strips thinking content before parsing to prevent tag forgery
///
/// Prompt caching: Both stages share identical system + transcript prefix
/// → Stage 2 guaranteed cache hit on prefix
pub async fn classify_tool_use(
    tool_name: &str,
    input: &Value,
    context: &ToolPermissionContext,
    messages: &[Message],
    api_client: &ApiClient,
    cancel: CancellationToken,
) -> YoloClassifierResult;
```

### Prompt Construction

```rust
/// Assembles classifier system prompt:
/// 1. BASE_PROMPT (system instructions)
/// 2. PERMISSIONS_TEMPLATE (external or anthropic-internal)
/// 3. User allow rules (from settings.autoMode.allow)
/// 4. User deny rules (soft_deny + optional PowerShell deny)
/// 5. Environment context (settings.autoMode.environment)
/// 6. Bash prompt rules (if BASH_CLASSIFIER feature gate enabled)
///
/// Replace tags: <user_allow_rules_to_replace>, <user_deny_rules_to_replace>,
///               <user_environment_to_replace>
/// Sync — no I/O. PERMISSIONS_TEMPLATE is embedded in the binary at build time.
/// GrowthBook bash prompt rules are read from cached config (no network).
pub fn build_yolo_system_prompt(context: &ToolPermissionContext) -> String;
```

### Transcript Building

```rust
/// Build classifier transcript from conversation messages.
/// INCLUDED:  User text (full content, including queued_command attachments)
/// INCLUDED:  Assistant tool_use blocks (redacted via Tool::to_auto_classifier_input)
/// EXCLUDED:  Assistant text blocks (model-authored, not user intent — prevents self-influence)
/// EXCLUDED:  Tool outputs (prevents information leakage)
pub fn build_transcript_for_classifier(messages: &[Message], tools: &ToolRegistry) -> String;

/// CLAUDE.md integration: wrapped in <user_claude_md>...</user_claude_md>
/// Treated as user-provided configuration of intent (not prompt injection)
/// Included in cache prefix with cache_control metadata
pub fn build_claude_md_message(claude_md_content: &str) -> Option<MessageParam>;
```

### Classifier Decision Integration (from `classifierDecision.ts`, 98 LOC)

```rust
/// Entry point for auto-mode permission check.
/// Returns None when auto-mode is inactive (caller falls through to interactive prompt).
/// Returns Some(decision) when auto-mode handled the request.
///
/// 1. If auto-mode is not active → return None (fallthrough)
/// 2. If tool is in safe allowlist → return Some(Allow)
/// 3. Run yoloClassifier:
///    - should_block: false → Some(Allow)
///    - should_block: true  → Some(Deny { message: reason })
///    - unavailable/error   → Some(Deny { message: "classifier unavailable" })
///    - transcript too long  → None (fallthrough to interactive)
pub async fn can_use_tool_in_auto_mode(
    tool_name: &str,
    input: &Value,
    context: &ToolPermissionContext,
    tool_use_id: &str,
    cancel: CancellationToken,
    is_non_interactive: bool,
) -> Option<PermissionDecision>;
```

### Safe-Tool Allowlist (from `classifierShared.ts`)

```rust
/// Read-only and task-management tools skip classifier entirely in auto-mode.
/// List: FileRead, Grep, Glob, LSP, ToolSearch, TodoWrite,
///       TaskCreate/Update/Get/List/Stop/Output,
///       EnterPlanMode/ExitPlanMode,
///       TeamCreate/TeamDelete, SendMessage, Sleep
pub fn is_auto_mode_allowlisted_tool(tool_name: &str) -> bool;
```

## Auto-Mode State Machine (from `autoModeState.ts`, 39 LOC)

```rust
/// Session-scoped auto-mode state (not persisted).
/// THREAD SAFETY: Must be stored in `Arc<RwLock<AutoModeState>>` since it is
/// mutated from permission checks (any tokio task) and read from UI thread.
/// Use AtomicBool for the hot-path `is_active()` check to avoid lock contention.
pub struct AutoModeState {
    active: AtomicBool,     // Hot path — lock-free read
    cli_flag: bool,         // Immutable after startup
    circuit_broken: AtomicBool,  // Set once by GrowthBook gate check
}

impl AutoModeState {
    pub fn is_active(&self) -> bool;         // AtomicBool::load(Ordering::Relaxed)
    pub fn set_active(&self, active: bool);  // AtomicBool::store
    pub fn is_circuit_broken(&self) -> bool;
    pub fn set_circuit_broken(&self, broken: bool);
}

/// Circuit breaker: reads GrowthBook gate (cached in coco-config, no I/O here).
/// If tengu_auto_mode_config.enabled == "disabled" → set circuit_broken = true.
/// Called once at session startup.
pub fn verify_auto_mode_gate_access(state: &AutoModeState);
```

## Denial Tracking (from `denialTracking.ts`, 45 LOC)

```rust
/// Fail-safe: auto-mode **classifier** falls back to prompting after too many classifier denials.
/// This does NOT override rule-based denials (step 1 of evaluate_permission always wins).
/// Denial tracking only applies to auto-mode classifier decisions (step 5 of pipeline).
///
/// THREAD SAFETY: Must be stored in `Arc<RwLock<DenialTrackingState>>`.
/// Write lock required for record_denial()/record_success().
/// Read lock sufficient for should_fallback_to_prompting().
const MAX_CONSECUTIVE_DENIALS: i32 = 3;
const MAX_TOTAL_DENIALS: i32 = 20;

impl DenialTrackingState {
    pub fn record_denial(&mut self);
    pub fn record_success(&mut self);  // Resets consecutive counter
    pub fn should_fallback_to_prompting(&self) -> bool;
    // Triggers when: consecutive >= 3 OR total >= 20
}
```

## Dangerous Pattern Detection (from `dangerousPatterns.ts`)

```rust
/// Patterns stripped from permission context at auto-mode entry
/// to prevent classifier bypass via user-configured rules.
///
/// Bash dangerous patterns:
///   python:*, node:*, eval, exec, ssh, curl, git, sudo
///   Anthropic-internal: fa run, coo, gh api, kubectl, aws, gcloud
///
/// PowerShell dangerous patterns:
///   iex (Invoke-Expression), pwsh, Start-Process, Start-Job
///   All CROSS_PLATFORM_CODE_EXEC
pub fn is_dangerous_bash_permission(tool_name: &str, rule_content: &str) -> bool;
pub fn is_dangerous_powershell_permission(tool_name: &str, rule_content: &str) -> bool;

/// Strip dangerous rules from permission context at auto-mode entry
pub fn strip_dangerous_rules(rules: &mut Vec<PermissionRule>);
```

## Permission Context (from `PermissionContext.ts`, 388 LOC)

```rust
/// Wraps tool + input + decision logic into a frozen context object.
/// Used by the interactive handler to manage the decision lifecycle.
pub struct PermissionContext {
    tool_id: ToolId,
    input: Value,
    context: ToolPermissionContext,
    tool_use_id: String,
}

impl PermissionContext {
    /// Try classifier-based auto-approval
    pub async fn try_classifier(&self) -> Option<PermissionDecision>;

    /// Run pre/post tool use hooks for permission decisions
    pub async fn run_hooks(&self, mode: Option<&str>) -> Option<PermissionDecision>;

    /// Persist "always allow" rules to disk
    pub async fn persist_permissions(&self, updates: &[PermissionUpdate]) -> bool;

    /// Build allow/deny decisions with reason tracking
    pub fn build_allow(&self, updated_input: Option<Value>) -> PermissionDecision;
    pub fn build_deny(&self, message: &str, reason: PermissionDecisionReason) -> PermissionDecision;
}
```

## Security Invariants

1. **Thinking content stripping**: Parse `<thinking>` separately before extracting `<block>`/`<reason>`. Prevents crafted content inside reasoning from forging decisions.
2. **Transcript sanitization**: Exclude assistant text and tool outputs from classifier input. Prevents prompt injection via model output.
3. **Dangerous pattern stripping**: Remove code-exec patterns (python/node/eval) from permission context at auto-mode entry. Prevents classifier bypass via user rules.
4. **Denial tracking fail-safe**: 3 consecutive or 20 total denials → force interactive prompting. Prevents stuck classifier loops.
5. **Circuit breaker**: GrowthBook gate can remotely disable auto-mode system-wide.

## Key Constants

| Constant | Value | Purpose |
|----------|-------|---------|
| Stage 1 max tokens | 64 or 256 | Quick decision budget |
| Stage 2 max tokens | 4096+ | Full reasoning budget |
| Thinking padding | 2048 | Headroom for adaptive thinking |
| Max consecutive denials | 3 | Before fallback to prompting |
| Max total denials | 20 | Per session |
| Grace period | 200ms | Before hiding classifier indicator on user interact |
| Cache control TTL | 5 min | Default per cache_control |
