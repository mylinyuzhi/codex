use coco_tool::DescriptionOptions;
use coco_tool::PromptOptions;
use coco_tool::Tool;
use coco_tool::ToolError;
use coco_tool::ToolUseContext;
use coco_tool::ValidationResult;
use coco_types::PermissionDecision;
use coco_types::PermissionMode;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use coco_types::ToolResult;
use serde_json::Value;
use std::collections::HashMap;

// ── EnterPlanModeTool ──

pub struct EnterPlanModeTool;

/// Full prompt text for the model.
///
/// TS: tools/EnterPlanModeTool/prompt.ts — getEnterPlanModeToolPromptExternal()
const ENTER_PLAN_MODE_PROMPT: &str = "\
Use this tool proactively when you're about to start a non-trivial implementation task. \
Getting user sign-off on your approach before writing code prevents wasted effort and ensures \
alignment. This tool transitions you into plan mode where you can explore the codebase and \
design an implementation approach for user approval.

## When to Use This Tool

**Prefer using EnterPlanMode** for implementation tasks unless they're simple. Use it when \
ANY of these conditions apply:

1. **New Feature Implementation**: Adding meaningful new functionality
2. **Multiple Valid Approaches**: The task can be solved in several different ways
3. **Code Modifications**: Changes that affect existing behavior or structure
4. **Architectural Decisions**: The task requires choosing between patterns or technologies
5. **Multi-File Changes**: The task will likely touch more than 2-3 files
6. **Unclear Requirements**: You need to explore before understanding the full scope
7. **User Preferences Matter**: The implementation could reasonably go multiple ways

## When NOT to Use This Tool

Only skip EnterPlanMode for simple tasks:
- Single-line or few-line fixes (typos, obvious bugs, small tweaks)
- Adding a single function with clear requirements
- Tasks where the user has given very specific, detailed instructions
- Pure research/exploration tasks (use the Agent tool with explore agent instead)

## What Happens in Plan Mode

In plan mode, you'll:
1. Thoroughly explore the codebase using Glob, Grep, and Read tools
2. Understand existing patterns and architecture
3. Design an implementation approach
4. Present your plan to the user for approval
5. Use AskUserQuestion if you need to clarify approaches
6. Exit plan mode with ExitPlanMode when ready to implement

## Important Notes

- This tool REQUIRES user approval - they must consent to entering plan mode
- If unsure whether to use it, err on the side of planning
- Users appreciate being consulted before significant changes are made to their codebase";

#[async_trait::async_trait]
impl Tool for EnterPlanModeTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::EnterPlanMode)
    }
    fn name(&self) -> &str {
        ToolName::EnterPlanMode.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Requests permission to enter plan mode for complex tasks requiring \
         exploration and design"
            .into()
    }
    async fn prompt(&self, _options: &PromptOptions) -> String {
        ENTER_PLAN_MODE_PROMPT.to_string()
    }
    fn user_facing_name(&self) -> &str {
        ""
    }
    fn input_schema(&self) -> ToolInputSchema {
        ToolInputSchema {
            properties: HashMap::new(),
        }
    }
    fn is_read_only(&self, _: &Value) -> bool {
        true
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn search_hint(&self) -> Option<&str> {
        Some("switch to plan mode to design an approach before coding")
    }

    async fn execute(
        &self,
        _input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        // TS: agents cannot enter plan mode (EnterPlanModeTool.ts:78)
        if ctx.agent_id.is_some() {
            return Err(ToolError::InvalidInput {
                message: "EnterPlanMode tool cannot be used in agent contexts".into(),
                error_code: None,
            });
        }

        Ok(ToolResult {
            data: serde_json::json!({
                "message": "Entered plan mode. You should now focus on exploring \
                            the codebase and designing an implementation approach.",
            }),
            new_messages: vec![],
        })
    }

    /// Stash the current permission mode and switch to Plan.
    ///
    /// TS: `prepareContextForPlanMode()` + `applyPermissionUpdate({ mode: 'plan' })`
    ///      + `handlePlanModeTransition()` (clears exit attachment to prevent
    ///      double-attach on quick toggle)
    fn modify_context_after(&self, _result: &ToolResult<Value>, ctx: &mut ToolUseContext) {
        let current_mode = ctx.permission_context.mode;
        if current_mode != PermissionMode::Plan {
            ctx.permission_context.pre_plan_mode = Some(current_mode);
        }
        ctx.permission_context.mode = PermissionMode::Plan;

        // TS: handlePlanModeTransition — clear any pending exit attachment
        // when entering plan mode (prevents sending both plan_mode and
        // plan_mode_exit when user toggles quickly).
        if let Some(state) = &ctx.app_state
            && let Ok(mut guard) = state.try_write()
            && let Some(obj) = guard.as_object_mut()
        {
            obj.insert(
                "needs_plan_mode_exit_attachment".into(),
                serde_json::Value::Bool(false),
            );
        }
    }
}

impl EnterPlanModeTool {
    /// Build the instructions text returned to the model as tool_result content.
    ///
    /// TS: `mapToolResultToToolResultBlockParam` in EnterPlanModeTool.ts
    pub fn build_instructions(confirmation: &str) -> String {
        format!(
            "{confirmation}\n\n\
             In plan mode, you should:\n\
             1. Thoroughly explore the codebase to understand existing patterns\n\
             2. Identify similar features and architectural approaches\n\
             3. Consider multiple approaches and their trade-offs\n\
             4. Use AskUserQuestion if you need to clarify the approach\n\
             5. Design a concrete implementation strategy\n\
             6. When ready, use ExitPlanMode to present your plan for approval\n\n\
             Remember: DO NOT write or edit any files yet. This is a read-only \
             exploration and planning phase."
        )
    }
}

// ── ExitPlanModeTool ──

pub struct ExitPlanModeTool;

/// Full prompt text for the model.
///
/// TS: tools/ExitPlanModeTool/prompt.ts — EXIT_PLAN_MODE_V2_TOOL_PROMPT
const EXIT_PLAN_MODE_PROMPT: &str = "\
Use this tool when you are in plan mode and have finished writing your plan to the plan file \
and are ready for user approval.

## How This Tool Works
- You should have already written your plan to the plan file specified in the plan mode system \
message
- This tool does NOT take the plan content as a parameter - it will read the plan from the file \
you wrote
- This tool simply signals that you're done planning and ready for the user to review and approve
- The user will see the contents of your plan file when they review it

## When to Use This Tool
IMPORTANT: Only use this tool when the task requires planning the implementation steps of a task \
that requires writing code. For research tasks where you're gathering information, searching \
files, reading files or in general trying to understand the codebase - do NOT use this tool.

## Before Using This Tool
Ensure your plan is complete and unambiguous:
- If you have unresolved questions about requirements or approach, use AskUserQuestion first
- Once your plan is finalized, use THIS tool to request approval

**Important:** Do NOT use AskUserQuestion to ask \"Is this plan okay?\" or \"Should I proceed?\" \
- that's exactly what THIS tool does. ExitPlanMode inherently requests user approval of your plan.";

#[async_trait::async_trait]
impl Tool for ExitPlanModeTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::ExitPlanMode)
    }
    fn name(&self) -> &str {
        ToolName::ExitPlanMode.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Prompts the user to exit plan mode and start coding".into()
    }
    async fn prompt(&self, _options: &PromptOptions) -> String {
        EXIT_PLAN_MODE_PROMPT.to_string()
    }
    fn user_facing_name(&self) -> &str {
        ""
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "allowedPrompts".into(),
            serde_json::json!({
                "type": "array",
                "description": "Prompt-based permissions needed to implement the plan.",
                "items": {
                    "type": "object",
                    "properties": {
                        "tool": { "type": "string", "description": "The tool this prompt applies to" },
                        "prompt": { "type": "string", "description": "Semantic description of the action" }
                    }
                }
            }),
        );
        ToolInputSchema { properties: p }
    }
    fn is_read_only(&self, _: &Value) -> bool {
        false
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn search_hint(&self) -> Option<&str> {
        Some("present plan for approval and start coding (plan mode only)")
    }

    /// Reject if not currently in plan mode.
    ///
    /// TS: ExitPlanModeV2Tool.ts:195-219
    /// Teammates bypass validation — their AppState may show the leader's mode,
    /// so `isPlanModeRequired()` is the real source of truth for teammates.
    fn validate_input(&self, _input: &Value, ctx: &ToolUseContext) -> ValidationResult {
        // Teammates always pass validation (TS: isTeammate() check).
        // Note: agent_id.is_some() is NOT the same as isTeammate().
        // Regular subagents have agent_id but are NOT teammates.
        if ctx.is_teammate {
            return ValidationResult::Valid;
        }

        if ctx.permission_context.mode != PermissionMode::Plan {
            return ValidationResult::invalid_with_code(
                "You are not in plan mode. This tool is only for exiting plan mode \
                 after writing a plan. If your plan was already approved, continue \
                 with implementation.",
                "1",
            );
        }
        ValidationResult::Valid
    }

    /// Non-teammate contexts require user confirmation to exit plan mode.
    ///
    /// TS: ExitPlanModeV2Tool.ts:221-239
    /// Teammates bypass the permission UI entirely. The call() method handles
    /// their behavior: isPlanModeRequired() sends plan_approval_request to
    /// leader, otherwise exits locally.
    async fn check_permissions(&self, _input: &Value, ctx: &ToolUseContext) -> PermissionDecision {
        // Teammates bypass the permission UI (TS: isTeammate() check)
        if ctx.is_teammate {
            return PermissionDecision::Allow {
                updated_input: None,
                feedback: None,
            };
        }
        // Non-teammates: require user confirmation
        PermissionDecision::Ask {
            message: "Exit plan mode?".into(),
            suggestions: vec![],
        }
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let is_agent = ctx.agent_id.is_some();

        // Read plan from input (CCR/hook may have injected it) or from app_state
        let input_plan = input.get("plan").and_then(|v| v.as_str()).map(String::from);
        let plan = input_plan.clone().or_else(|| read_plan_from_app_state(ctx));

        // Read plan file path from app_state if available
        let file_path = read_plan_file_path_from_app_state(ctx);

        // If plan was provided in input (CCR edit), persist to disk
        if let (Some(plan_content), Some(path)) = (&input_plan, &file_path)
            && let Err(e) = tokio::fs::write(path, plan_content.as_bytes()).await
        {
            tracing::warn!("Failed to persist edited plan to {path}: {e}");
        }

        let has_agent_tool = ctx
            .tools
            .get_by_name(ToolName::Agent.as_str())
            .is_some_and(|t| t.is_enabled());

        Ok(ToolResult {
            data: serde_json::json!({
                "plan": plan,
                "isAgent": is_agent,
                "filePath": file_path,
                "hasTaskTool": if has_agent_tool { Some(true) } else { None::<bool> },
                "planWasEdited": if input_plan.is_some() { Some(true) } else { None::<bool> },
            }),
            new_messages: vec![],
        })
    }

    /// Restore the permission mode that was active before entering plan mode.
    ///
    /// TS: ExitPlanModeV2Tool.ts:357-403
    fn modify_context_after(&self, _result: &ToolResult<Value>, ctx: &mut ToolUseContext) {
        if ctx.permission_context.mode != PermissionMode::Plan {
            return;
        }

        let restore_mode = ctx
            .permission_context
            .pre_plan_mode
            .unwrap_or(PermissionMode::Default);

        ctx.permission_context.mode = restore_mode;
        ctx.permission_context.pre_plan_mode = None;

        // Set exit flags in app_state for the system reminder orchestrator.
        // TS: setHasExitedPlanMode(true), setNeedsPlanModeExitAttachment(true)
        if let Some(state) = &ctx.app_state
            && let Ok(mut guard) = state.try_write()
            && let Some(obj) = guard.as_object_mut()
        {
            obj.insert("has_exited_plan_mode".into(), serde_json::Value::Bool(true));
            obj.insert(
                "needs_plan_mode_exit_attachment".into(),
                serde_json::Value::Bool(true),
            );
        }
    }
}

impl ExitPlanModeTool {
    /// Build the instructions text returned to the model as tool_result content.
    ///
    /// TS: `mapToolResultToToolResultBlockParam` in ExitPlanModeV2Tool.ts
    pub fn build_instructions(result_data: &Value) -> String {
        let is_agent = result_data
            .get("isAgent")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let plan = result_data.get("plan").and_then(|v| v.as_str());
        let file_path = result_data.get("filePath").and_then(|v| v.as_str());
        let has_task_tool = result_data
            .get("hasTaskTool")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let plan_was_edited = result_data
            .get("planWasEdited")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        if is_agent {
            return "User has approved the plan. There is nothing else needed \
                    from you now. Please respond with \"ok\""
                .to_string();
        }

        if plan.is_none() || plan.is_some_and(|p| p.trim().is_empty()) {
            return "User has approved exiting plan mode. You can now proceed.".to_string();
        }

        let plan_text = plan.unwrap_or("");
        let team_hint = if has_task_tool {
            "\n\nIf this plan can be broken down into multiple independent tasks, \
             consider using the TeamCreate tool to create a team and parallelize \
             the work."
        } else {
            ""
        };
        let plan_label = if plan_was_edited {
            "Approved Plan (edited by user)"
        } else {
            "Approved Plan"
        };
        let path_note = file_path
            .map(|p| {
                format!(
                    "\nYour plan has been saved to: {p}\n\
                     You can refer back to it if needed during implementation."
                )
            })
            .unwrap_or_default();

        format!(
            "User has approved your plan. You can now start coding. \
             Start with updating your todo list if applicable\n\
             {path_note}{team_hint}\n\n\
             ## {plan_label}:\n{plan_text}"
        )
    }
}

/// Read plan content from app_state JSON.
///
/// The app layer (tui/cli) stores plan data at `app_state.plan_content`.
fn read_plan_from_app_state(ctx: &ToolUseContext) -> Option<String> {
    let state = ctx.app_state.as_ref()?;
    let guard = state.try_read().ok()?;
    guard
        .get("plan_content")
        .and_then(serde_json::Value::as_str)
        .map(String::from)
}

/// Read plan file path from app_state JSON.
fn read_plan_file_path_from_app_state(ctx: &ToolUseContext) -> Option<String> {
    let state = ctx.app_state.as_ref()?;
    let guard = state.try_read().ok()?;
    guard
        .get("plan_file_path")
        .and_then(serde_json::Value::as_str)
        .map(String::from)
}

// ── EnterWorktreeTool ──

pub struct EnterWorktreeTool;

#[async_trait::async_trait]
impl Tool for EnterWorktreeTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::EnterWorktree)
    }
    fn name(&self) -> &str {
        ToolName::EnterWorktree.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Create and enter a git worktree for isolated work on a branch.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "branch".into(),
            serde_json::json!({"type": "string", "description": "Branch name for the worktree"}),
        );
        p.insert(
            "path".into(),
            serde_json::json!({"type": "string", "description": "Path for the worktree directory (optional, defaults to ../worktrees/<branch>)"}),
        );
        ToolInputSchema { properties: p }
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let branch = input
            .get("branch")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();

        if branch.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "branch parameter is required".into(),
                error_code: None,
            });
        }

        let worktree_path = input
            .get("path")
            .and_then(|v| v.as_str())
            .map(std::string::ToString::to_string)
            .unwrap_or_else(|| format!("../worktrees/{branch}"));

        let output = tokio::process::Command::new("git")
            .args(["worktree", "add", "-b", branch, &worktree_path])
            .output()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("Failed to run git worktree add: {e}"),
                source: None,
            })?;

        if !output.status.success() {
            // Try without -b (branch may already exist)
            let output2 = tokio::process::Command::new("git")
                .args(["worktree", "add", &worktree_path, branch])
                .output()
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    message: format!("Failed to run git worktree add: {e}"),
                    source: None,
                })?;

            if !output2.status.success() {
                let stderr = String::from_utf8_lossy(&output2.stderr);
                return Err(ToolError::ExecutionFailed {
                    message: format!("git worktree add failed: {stderr}"),
                    source: None,
                });
            }
        }

        Ok(ToolResult {
            data: serde_json::json!({
                "message": format!("Created worktree at '{worktree_path}' on branch '{branch}'"),
                "path": worktree_path,
                "branch": branch,
            }),
            new_messages: vec![],
        })
    }
}

// ── ExitWorktreeTool ──
//
// TS: `tools/ExitWorktreeTool/ExitWorktreeTool.ts:29-145`. The TS tool
// tears down a worktree AND restores the session's prior state in a
// specific order:
//
//   1. `setCwd(originalCwd)` — process-level current directory
//   2. `setOriginalCwd(originalCwd)` — session's recorded origin cwd
//   3. `setProjectRoot(previousProjectRoot)` — conditional
//   4. `restoreHooksSnapshot()` — revert hook overrides made in worktree
//   5. `restoreSystemPromptSections()` — rebuild system prompt
//   6. `clearMemoryCaches()` — drop claude.md / memory caches
//
// Steps 3–6 live at the query-engine/app layer in coco-rs (they require
// cross-crate access to the system prompt builder, hook registry, etc.)
// and are out of scope for this tool alone. Step 1 (`set_current_dir`)
// is the critical one — without it, the process cwd is left dangling
// inside a just-removed directory and the next Bash call fails with
// ENOENT. This implementation handles step 1 inline and emits the
// other restoration targets in the result payload so the query engine
// can apply them in its SessionEnd-like cleanup hook (follow-up).

pub struct ExitWorktreeTool;

#[async_trait::async_trait]
impl Tool for ExitWorktreeTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::ExitWorktree)
    }
    fn name(&self) -> &str {
        ToolName::ExitWorktree.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Remove a git worktree and return to the previous working directory. \
         Restores the process CWD if it was inside the worktree being removed \
         and returns a `restoration` block describing the session state to \
         rebuild (hooks, system prompt, memory caches)."
            .into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "path".into(),
            serde_json::json!({"type": "string", "description": "Path of the worktree to remove"}),
        );
        p.insert(
            "force".into(),
            serde_json::json!({"type": "boolean", "description": "Force removal even with uncommitted changes"}),
        );
        p.insert(
            "previous_cwd".into(),
            serde_json::json!({
                "type": "string",
                "description": "Absolute path to restore as the process cwd after the \
                               worktree is removed. If omitted, defaults to the parent \
                               directory of the worktree."
            }),
        );
        ToolInputSchema { properties: p }
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let path = input
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();

        if path.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "path parameter is required".into(),
                error_code: None,
            });
        }

        let force = input
            .get("force")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        // Resolve the restoration target BEFORE we remove the worktree.
        // Three sources in priority order:
        //   1. Explicit `previous_cwd` parameter from the caller.
        //   2. The parent directory of the worktree path.
        //   3. Current process cwd — last-ditch fallback; if the process
        //      cwd is inside the worktree this will fail step 1 below
        //      and leave the caller in a dangling dir. Better than
        //      panicking, though.
        let explicit_prev = input
            .get("previous_cwd")
            .and_then(|v| v.as_str())
            .map(std::path::PathBuf::from);

        let worktree_path = std::path::PathBuf::from(path);
        let parent_fallback = worktree_path.parent().map(std::path::Path::to_path_buf);
        let restore_target = explicit_prev
            .or(parent_fallback)
            .or_else(|| std::env::current_dir().ok());

        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.push(path);

        let output = tokio::process::Command::new("git")
            .args(&args)
            .output()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("Failed to run git worktree remove: {e}"),
                source: None,
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ToolError::ExecutionFailed {
                message: format!("git worktree remove failed: {stderr}"),
                source: None,
            });
        }

        // Layer 1: restore process CWD. Critical: without this, if the
        // process cwd was inside the just-removed worktree, every
        // subsequent relative path operation fails with ENOENT.
        //
        // TS `ExitWorktreeTool.ts:126` calls `setCwd(originalCwd)`
        // **unconditionally** — it always moves the cwd to the
        // restoration target, whether or not the current cwd was inside
        // the worktree. This is the predictable behavior: callers can
        // rely on "after ExitWorktree, cwd is the restoration target".
        //
        // Previously coco-rs only chdir'd if the cwd was inside the
        // worktree, which was a divergence from TS and risked leaving
        // the process in an unexpected location. R5 aligns with TS by
        // always chdiring to the restoration target.
        let mut cwd_restored = false;
        let mut restore_error: Option<String> = None;
        if let Some(target) = restore_target.as_ref() {
            match std::env::set_current_dir(target) {
                Ok(()) => cwd_restored = true,
                Err(e) => restore_error = Some(e.to_string()),
            }
        }

        // Layers 2-6: report what the query-engine layer still needs to
        // restore. These are keys the caller can use to drive its own
        // cleanup hook — the tool itself can't touch them because they
        // live in a higher-layer state tree that's not accessible via
        // ToolUseContext.
        Ok(ToolResult {
            data: serde_json::json!({
                "message": format!("Removed worktree at '{path}'"),
                "path": path,
                "restoration": {
                    "cwd_target": restore_target.as_ref().and_then(|p| p.to_str()),
                    "cwd_restored": cwd_restored,
                    "cwd_restore_error": restore_error,
                    // Follow-up layers for the query-engine cleanup hook:
                    "pending_layers": [
                        "originalCwd",
                        "projectRoot",
                        "hooksSnapshot",
                        "systemPromptSections",
                        "memoryCaches",
                    ]
                }
            }),
            new_messages: vec![],
        })
    }
}

#[cfg(test)]
#[path = "plan_worktree.test.rs"]
mod tests;
