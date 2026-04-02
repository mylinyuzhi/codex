# coco-permissions — Crate Plan

TS source: `src/utils/permissions/` (26 files), `src/types/permissions.ts`

## Dependencies

```
coco-permissions depends on:
  - coco-types    (PermissionMode, PermissionBehavior, PermissionRule, PermissionRuleSource,
                   PermissionDecision, ToolPermissionContext)
  - coco-config   (Settings — for reading permission rules from settings layers)
  - coco-error
  - regex         (dangerous pattern matching)

coco-permissions does NOT depend on:
  - coco-tool     (no Tool trait — receives tool name + input as parameters)
  - coco-shell    (no shell awareness — bash command classification is internal)
  - coco-inference (no LLM calls — yolo classifier deferred to P1)
  - any app/ crate
```

## Core Logic

### Permission Evaluation Pipeline

```rust
/// Evaluate permission for a tool call. Order:
/// 1. Check deny rules (deny wins immediately)
/// 2. Check mode bypass (bypassPermissions -> allow all)
/// 3. Check allow rules (tool-level, then content-level like "Bash(prefix:git *)")
/// 4. Check ask rules
/// 5. Default: ask user
pub fn evaluate_permission(
    tool: &dyn Tool,
    input: &Value,
    context: &ToolPermissionContext,
) -> PermissionDecision;

/// Rule matching for Bash tool:
/// "Bash(prefix:git *)" matches commands starting with "git "
/// "Bash(exact:ls -la)" matches exact command
pub fn matches_bash_rule(rule_content: &str, command: &str) -> bool;

/// Wildcard pattern matching for shell rules
pub fn shell_rule_matches(pattern: &str, command: &str) -> bool;
```

### Permission Rule Sources (priority high to low)

```rust
/// session > command > cliArg > flagSettings > localSettings > projectSettings > userSettings > policySettings
pub fn get_effective_rules(context: &ToolPermissionContext) -> Vec<PermissionRule>;
```

### Specialized Modules

```rust
// Bash classifier — ML-based command safety classification
pub async fn classify_bash_command(command: &str) -> ClassifierResult;

// Filesystem permissions — path-based rules
pub fn check_path_permission(path: &Path, context: &ToolPermissionContext) -> PermissionDecision;

// Denial tracking — escalate after repeated denials
pub struct DenialTracker { consecutive_denials: i32, threshold: i32 }
pub fn should_fallback_to_prompt(tracker: &DenialTracker) -> bool;

// Shadowed rule detection
pub fn detect_shadowed_rules(allow: &[PermissionRule], deny: &[PermissionRule]) -> Vec<ShadowedRule>;

// Dangerous patterns (regex-based)
pub fn check_dangerous_patterns(command: &str) -> Vec<DangerousPattern>;
```
