//! EnterPlanMode + ExitPlanMode tools.
//!
//! TS:
//! - `src/tools/EnterPlanModeTool/EnterPlanModeTool.ts`
//! - `src/tools/ExitPlanModeTool/ExitPlanModeV2Tool.ts`
//!
//! Plan mode is a permission mode in which the model researches and
//! designs an implementation approach but MUST NOT modify the system.
//! Entry + exit are mediated by these two tools; the per-turn reminder
//! text and the permission-gate defaults live in `core/context` and
//! `core/permissions` respectively.

use coco_messages::ToolResult;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::PlanApprovalMessage;
use coco_tool_runtime::PlanApprovalRequest;
use coco_tool_runtime::PromptOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_tool_runtime::ValidationResult;
use coco_types::PermissionMode;
use coco_types::ToolId;
use coco_types::ToolName;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

/// Typed result payload for [`ExitPlanModeTool`]. Producers: `execute`
/// (creates it) serializes to `Value` at the Tool-trait boundary.
/// Consumers: `build_instructions` deserializes back here. Per
/// CLAUDE.md "Typed Structs over JSON Values": both sides live in the
/// same crate, so stringly-typed `Value.get("key")` access is a code
/// smell.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct ExitPlanModeOutput {
    /// The plan text — either the CCR-edited version from `input.plan`
    /// or the on-disk plan file contents.
    #[serde(skip_serializing_if = "Option::is_none")]
    plan: Option<String>,
    /// True when called from a subagent context. Shortens the approval
    /// text the model receives.
    is_agent: bool,
    /// Absolute path to the session's plan file.
    #[serde(skip_serializing_if = "Option::is_none")]
    file_path: Option<String>,
    /// Hint that the TeamCreate tool is available — surfaced as a hint
    /// in the approval message to encourage swarm-based implementation.
    #[serde(skip_serializing_if = "Option::is_none")]
    has_task_tool: Option<bool>,
    /// True when the CCR UI edited the plan via `input.plan` before exit.
    #[serde(skip_serializing_if = "Option::is_none")]
    plan_was_edited: Option<bool>,
    /// ExitPlanMode stale-plan advisory outcome (None when disabled).
    #[serde(skip_serializing_if = "Option::is_none")]
    plan_verification: Option<coco_context::PlanVerificationOutcome>,
    /// Teammate-awaiting-approval branch: the teammate's tool call
    /// submitted the plan to the lead and must stay in plan mode until
    /// the response arrives.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    awaiting_leader_approval: bool,
    /// Correlation ID for the pending approval (only set when
    /// `awaiting_leader_approval` is true).
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<String>,
}

/// Build the cross-turn `AppStatePatch` that flips state into plan
/// mode. Captures the current mode (so `ExitPlanMode` knows where to
/// restore) and stamps the entry timestamp so `ExitPlanMode` can
/// optionally compare against the plan-file mtime.
///
/// TS parity: the inline closure inside `EnterPlanModeTool.ts::call`
/// at lines 88-94 — `setAppState(prev => ({ ...prev, prePlanMode:
/// currentMode, planModeEntryMs: Date.now() }))`. The same shape is
/// applied at two entry sites in coco-rs: the tool's own `execute`
/// and the `/plan <description>` slash command, which (per TS
/// `commands/plan/plan.tsx:73-91`) flips state directly without
/// re-prompting the user.
pub fn build_enter_plan_mode_patch(current_mode: PermissionMode) -> coco_types::AppStatePatch {
    Box::new(move |state| {
        coco_permissions::apply_permission_mode_transition_to_app_state(
            state,
            current_mode,
            PermissionMode::Plan,
        );
    })
}

// ── EnterPlanModeTool ──

pub struct EnterPlanModeTool;

/// Full prompt text for the model.
///
/// TS: `tools/EnterPlanModeTool/prompt.ts` — `getEnterPlanModeToolPromptExternal()`.
/// The Ant arm (`getEnterPlanModeToolPromptAnt`) and the
/// `USER_TYPE === 'ant'` dispatcher are intentionally NOT ported
/// (root `CLAUDE.md` "no Ant gates"). Interpolates
/// `ToolName::AskUserQuestion.as_str()` so a future tool rename
/// propagates without a doc edit, mirroring TS's
/// `${ASK_USER_QUESTION_TOOL_NAME}` substitution. Names like
/// `EnterPlanMode` / `ExitPlanMode` stay as literals — TS does the
/// same.
fn enter_plan_mode_prompt(is_plan_interview_phase: bool) -> String {
    let aq = coco_types::ToolName::AskUserQuestion.as_str();
    // Mirrors TS `WHAT_HAPPENS_SECTION` interpolation: empty string
    // when the iterative interview workflow is active (the model
    // gets the detailed loop via the plan-mode attachment instead).
    let what_happens = if is_plan_interview_phase {
        String::new()
    } else {
        format!(
            "## What Happens in Plan Mode

In plan mode, you'll:
1. Thoroughly explore the codebase using Glob, Grep, and Read tools
2. Understand existing patterns and architecture
3. Design an implementation approach
4. Present your plan to the user for approval
5. Use {aq} if you need to clarify approaches
6. Exit plan mode with ExitPlanMode when ready to implement

"
        )
    };
    format!(
        "Use this tool proactively when you're about to start a non-trivial implementation task. Getting user sign-off on your approach before writing code prevents wasted effort and ensures alignment. This tool transitions you into plan mode where you can explore the codebase and design an implementation approach for user approval.

## When to Use This Tool

**Prefer using EnterPlanMode** for implementation tasks unless they're simple. Use it when ANY of these conditions apply:

1. **New Feature Implementation**: Adding meaningful new functionality
   - Example: \"Add a logout button\" - where should it go? What should happen on click?
   - Example: \"Add form validation\" - what rules? What error messages?

2. **Multiple Valid Approaches**: The task can be solved in several different ways
   - Example: \"Add caching to the API\" - could use Redis, in-memory, file-based, etc.
   - Example: \"Improve performance\" - many optimization strategies possible

3. **Code Modifications**: Changes that affect existing behavior or structure
   - Example: \"Update the login flow\" - what exactly should change?
   - Example: \"Refactor this component\" - what's the target architecture?

4. **Architectural Decisions**: The task requires choosing between patterns or technologies
   - Example: \"Add real-time updates\" - WebSockets vs SSE vs polling
   - Example: \"Implement state management\" - Redux vs Context vs custom solution

5. **Multi-File Changes**: The task will likely touch more than 2-3 files
   - Example: \"Refactor the authentication system\"
   - Example: \"Add a new API endpoint with tests\"

6. **Unclear Requirements**: You need to explore before understanding the full scope
   - Example: \"Make the app faster\" - need to profile and identify bottlenecks
   - Example: \"Fix the bug in checkout\" - need to investigate root cause

7. **User Preferences Matter**: The implementation could reasonably go multiple ways
   - If you would use {aq} to clarify the approach, use EnterPlanMode instead
   - Plan mode lets you explore first, then present options with context

## When NOT to Use This Tool

Only skip EnterPlanMode for simple tasks:
- Single-line or few-line fixes (typos, obvious bugs, small tweaks)
- Adding a single function with clear requirements
- Tasks where the user has given very specific, detailed instructions
- Pure research/exploration tasks (use the Agent tool with explore agent instead)

{what_happens}## Examples

### GOOD - Use EnterPlanMode:
User: \"Add user authentication to the app\"
- Requires architectural decisions (session vs JWT, where to store tokens, middleware structure)

User: \"Optimize the database queries\"
- Multiple approaches possible, need to profile first, significant impact

User: \"Implement dark mode\"
- Architectural decision on theme system, affects many components

User: \"Add a delete button to the user profile\"
- Seems simple but involves: where to place it, confirmation dialog, API call, error handling, state updates

User: \"Update the error handling in the API\"
- Affects multiple files, user should approve the approach

### BAD - Don't use EnterPlanMode:
User: \"Fix the typo in the README\"
- Straightforward, no planning needed

User: \"Add a console.log to debug this function\"
- Simple, obvious implementation

User: \"What files handle routing?\"
- Research task, not implementation planning

## Important Notes

- This tool REQUIRES user approval - they must consent to entering plan mode
- If unsure whether to use it, err on the side of planning - it's better to get alignment upfront than to redo work
- Users appreciate being consulted before significant changes are made to their codebase
"
    )
}

/// Typed input for [`EnterPlanModeTool`] — no parameters. TS
/// `EnterPlanModeTool.ts` declares an empty input schema.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct EnterPlanModeInput {}

#[async_trait::async_trait]
impl Tool for EnterPlanModeTool {
    type Input = EnterPlanModeInput;
    coco_tool_runtime::impl_runtime_schema!(EnterPlanModeInput);
    /// Output is `Value` — the renderer reads `message` +
    /// `isInterviewPhase` positionally. Could be a typed
    /// `{message, isInterviewPhase}` struct in a follow-up; current
    /// shape stays compatible with the existing test fixtures.
    type Output = Value;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::EnterPlanMode)
    }
    fn name(&self) -> &str {
        ToolName::EnterPlanMode.as_str()
    }
    fn description(&self, _input: &EnterPlanModeInput, _options: &DescriptionOptions) -> String {
        "Requests permission to enter plan mode for complex tasks requiring \
         exploration and design"
            .into()
    }
    async fn prompt(&self, options: &PromptOptions) -> String {
        enter_plan_mode_prompt(options.is_plan_interview_phase)
    }
    fn user_facing_name(&self) -> &str {
        ""
    }
    fn is_read_only(&self, _input: &EnterPlanModeInput) -> bool {
        true
    }
    fn is_always_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self, _input: &EnterPlanModeInput) -> bool {
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
        _input: EnterPlanModeInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        // TS: agents cannot enter plan mode (EnterPlanModeTool.ts:78)
        if ctx.agent_id.is_some() {
            return Err(ToolError::InvalidInput {
                message: "EnterPlanMode tool cannot be used in agent contexts".into(),
                error_code: None,
            });
        }

        // Read live state (short-lived read lock) to decide what
        // the patch will do. TS parity:
        // `setAppState(prev => ({ ...prev, toolPermissionContext:
        // { mode: 'plan', prePlanMode: currentMode, ... } }))` —
        // `prev` in the TS closure is our `current` snapshot.
        let current_mode = match ctx.app_state.as_ref() {
            Some(h) => h
                .read()
                .await
                .permission_mode
                .unwrap_or(ctx.permission_context.mode),
            None => ctx.permission_context.mode,
        };

        // Queue the mutation. Executor applies post-execute under a
        // write lock. Tools can no longer `.write()` on app_state
        // directly — the type system blocks it (`AppStateReadHandle`
        // has no write surface). TS parity:
        // `orchestration.ts:queuedContextModifiers`. The patch
        // builder is shared with `dispatch_plan` so a typed `/plan
        // <description>` and a tool-driven entry land identical
        // app-state shape.
        let patch = build_enter_plan_mode_patch(current_mode);

        // TS parity: `EnterPlanModeTool.ts::call` returns only the
        // short confirmation in `data.message`. The post-processing
        // splice (6-step list vs interview-phase short form) lives in
        // `mapToolResultToToolResultBlockParam` — i.e.
        // [`Tool::render_for_model`] below. We carry
        // `is_plan_interview_phase` on `data` so the renderer (which
        // doesn't see `ToolUseContext`) can pick the right variant.
        Ok(
            ToolResult::data(serde_json::json!({
                "message": "Entered plan mode. You should now focus on exploring the codebase and designing an implementation approach.",
                "isInterviewPhase": ctx.is_plan_interview_phase,
            }))
            .with_patch(patch),
        )
    }

    fn render_for_model(&self, data: &Value) -> Vec<ToolResultContentPart> {
        // TS parity: `mapToolResultToToolResultBlockParam` in
        // `EnterPlanModeTool.ts:103-118`. Reads `data.message` (short
        // confirmation written by `execute`) and `data.isInterviewPhase`
        // (workflow-mode flag also written by `execute`) and emits the
        // splice. The renderer is a pure projection over `data` —
        // doesn't have access to `ToolUseContext`, which is why
        // `execute` stashes the flag.
        let message = data
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let is_interview = data
            .get("isInterviewPhase")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        vec![ToolResultContentPart::Text {
            text: Self::build_instructions(message, is_interview),
            provider_options: None,
        }]
    }
}

impl EnterPlanModeTool {
    /// Build the instructions text returned to the model as tool_result content.
    ///
    /// TS: `mapToolResultToToolResultBlockParam` in
    /// `tools/EnterPlanModeTool/EnterPlanModeTool.ts:103-118`. Two
    /// byte-precise variants gated on the interview-phase flag — the
    /// terse "DO NOT write" version when the iterative interview
    /// workflow will deliver detailed steps via the plan-mode
    /// attachment, otherwise the 6-step fallback that doubles as the
    /// workflow primer.
    pub fn build_instructions(confirmation: &str, is_plan_interview_phase: bool) -> String {
        if is_plan_interview_phase {
            format!(
                "{confirmation}

DO NOT write or edit any files except the plan file. Detailed workflow instructions will follow."
            )
        } else {
            format!(
                "{confirmation}

In plan mode, you should:
1. Thoroughly explore the codebase to understand existing patterns
2. Identify similar features and architectural approaches
3. Consider multiple approaches and their trade-offs
4. Use AskUserQuestion if you need to clarify the approach
5. Design a concrete implementation strategy
6. When ready, use ExitPlanMode to present your plan for approval

Remember: DO NOT write or edit any files yet. This is a read-only exploration and planning phase."
            )
        }
    }
}

// ── ExitPlanModeTool ──

pub struct ExitPlanModeTool;

/// Full prompt text for the model.
///
/// TS: tools/ExitPlanModeTool/prompt.ts — `EXIT_PLAN_MODE_V2_TOOL_PROMPT`.
/// Interpolates `ToolName::AskUserQuestion.as_str()` so a future rename of
/// the AskUserQuestion tool propagates without a doc edit, mirroring TS's
/// `${ASK_USER_QUESTION_TOOL_NAME}` substitution.
fn exit_plan_mode_prompt() -> String {
    let aq = coco_types::ToolName::AskUserQuestion.as_str();
    format!(
        "Use this tool when you are in plan mode and have finished writing your plan to the plan file and are ready for user approval.\n\
         \n\
         ## How This Tool Works\n\
         - You should have already written your plan to the plan file specified in the plan mode system message\n\
         - This tool does NOT take the plan content as a parameter - it will read the plan from the file you wrote\n\
         - This tool simply signals that you're done planning and ready for the user to review and approve\n\
         - The user will see the contents of your plan file when they review it\n\
         \n\
         ## When to Use This Tool\n\
         IMPORTANT: Only use this tool when the task requires planning the implementation steps of a task that requires writing code. For research tasks where you're gathering information, searching files, reading files or in general trying to understand the codebase - do NOT use this tool.\n\
         \n\
         ## Before Using This Tool\n\
         Ensure your plan is complete and unambiguous:\n\
         - If you have unresolved questions about requirements or approach, use {aq} first (in earlier phases)\n\
         - Once your plan is finalized, use THIS tool to request approval\n\
         \n\
         **Important:** Do NOT use {aq} to ask \"Is this plan okay?\" or \"Should I proceed?\" - that's exactly what THIS tool does. ExitPlanMode inherently requests user approval of your plan.\n\
         "
    )
}

/// The tool an `allowedPrompts` entry pre-approves. TS `z.enum(['Bash'])`.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
pub enum AllowedPromptTool {
    #[default]
    Bash,
}

/// Single entry in the `allowedPrompts` array — pre-approved tool / prompt
/// pair that the model is signaling it intends to use when plan is approved.
/// Both fields are required (TS `z.object({ tool, prompt })`), so the
/// derived schema carries `required: [tool, prompt]` and the `tool` enum —
/// no hand-patching of the model schema needed.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ExitPlanAllowedPrompt {
    /// The tool this prompt applies to (only `Bash` is supported).
    pub tool: AllowedPromptTool,
    /// The exact command/prompt to pre-approve.
    pub prompt: String,
}

/// Typed input for [`ExitPlanModeTool`].
///
/// The schema-visible field is `allowedPrompts` (TS-mirror). Three
/// additional fields ride along internally: `plan` and `planFilePath`
/// are spliced by the query layer (TS `normalizeToolInput` parity —
/// inject the on-disk plan content + its path into the tool's input for
/// hooks/SDK/transcript), and `user_choice` is spliced by the TUI
/// permission-multichoice dialog. All three are declared so the closed
/// runtime schema accepts them on re-validation; the model is taught to
/// emit only `allowedPrompts`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ExitPlanModeInput {
    /// Prompt-based permissions needed to implement the plan.
    #[serde(default, rename = "allowedPrompts")]
    pub allowed_prompts: Option<Vec<ExitPlanAllowedPrompt>>,
    /// (Internal) Plan content spliced by the query layer before
    /// invocation so hooks see the full plan body — TS parity with
    /// `normalizeToolInput`. Model never populates this.
    #[serde(default)]
    pub plan: Option<String>,
    /// (Internal) Absolute path to the on-disk plan file, spliced by the
    /// query layer alongside `plan` (same `normalizeToolInput` parity) so
    /// hooks/SDK observe it. Model never populates this.
    #[serde(default, rename = "planFilePath")]
    pub plan_file_path: Option<String>,
    /// (Internal) User's choice from the multi-option permission
    /// dialog: `yes-keep-context`, `yes-clear-context`, or `no`. The
    /// TUI splices it via `PermissionOutcome::Allow.updated_input`.
    #[serde(default)]
    pub user_choice: Option<String>,
}

#[async_trait::async_trait]
impl Tool for ExitPlanModeTool {
    type Input = ExitPlanModeInput;
    coco_tool_runtime::impl_runtime_schema!(ExitPlanModeInput);
    /// Output is `Value` — `ExitPlanModeOutput` is rich (multiple
    /// flags + nested `PlanVerificationOutcome` from coco-context that
    /// lacks JsonSchema). Renderer continues reading positional fields.
    type Output = Value;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::ExitPlanMode)
    }
    fn name(&self) -> &str {
        ToolName::ExitPlanMode.as_str()
    }

    /// Model-facing spec exposes ONLY `allowedPrompts` (TS
    /// `ExitPlanModeV2Tool` inputSchema). `plan` / `planFilePath` /
    /// `user_choice` stay in the runtime schema (CCR UI / hooks / SDK /
    /// TUI splice them) but are hidden from the model. The `allowedPrompts`
    /// item shape (`{ tool: enum["Bash"], prompt }`, both required) is
    /// derived from [`ExitPlanAllowedPrompt`] — so the model-facing and
    /// runtime schemas agree, and there is nothing to hand-patch here.
    async fn tool_spec(
        &self,
        _ctx: &coco_tool_runtime::SchemaContext,
        prompt_opts: &coco_tool_runtime::PromptOptions,
    ) -> coco_tool_runtime::ToolSpec {
        coco_tool_runtime::ToolSpec::Function(coco_tool_runtime::FunctionToolSpec {
            name: self.name().to_string(),
            description: self.prompt(prompt_opts).await,
            parameters: coco_tool_runtime::schema_omit_properties(
                self.runtime_validation_schema().as_value(),
                &["plan", "planFilePath", "user_choice"],
            ),
            strict: self.strict(),
        })
    }

    fn description(&self, _input: &ExitPlanModeInput, _options: &DescriptionOptions) -> String {
        "Prompts the user to exit plan mode and start coding".into()
    }
    async fn prompt(&self, _options: &PromptOptions) -> String {
        exit_plan_mode_prompt()
    }
    fn user_facing_name(&self) -> &str {
        ""
    }
    fn is_read_only(&self, _input: &ExitPlanModeInput) -> bool {
        false
    }
    fn is_concurrency_safe(&self, _input: &ExitPlanModeInput) -> bool {
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
    fn validate_input(&self, _input: &ExitPlanModeInput, ctx: &ToolUseContext) -> ValidationResult {
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
    async fn check_permissions(
        &self,
        _input: &ExitPlanModeInput,
        ctx: &ToolUseContext,
    ) -> coco_types::ToolCheckResult {
        // Teammates bypass the permission UI (TS: isTeammate() check).
        // Use explicit Allow rather than Passthrough — we want the
        // evaluator to short-circuit on this positive opinion before
        // rule / mode-fallthrough lookups.
        if ctx.is_teammate {
            return coco_types::ToolCheckResult::Allow {
                updated_input: None,
                feedback: None,
            };
        }
        // Non-teammates: require user confirmation.
        //
        // When `plan_mode.show_clear_context_on_exit` is true, surface
        // the multi-choice dialog. TS parity:
        // `ExitPlanModePermissionRequest.tsx:137, 691-704` — gated on
        // `settings.showClearContextOnPlanAccept` (default false).
        //
        // The TUI echoes the picked `value` back via
        // `updated_input.user_choice`, which `execute()` reads to
        // decide whether to flag `pending_clear_message_history` on the
        // app-state patch.
        let choices = if ctx.plan_mode_settings.show_clear_context_on_exit {
            Some(vec![
                coco_types::PermissionAskChoice {
                    value: "yes-keep-context".into(),
                    label: "Yes, keep context".into(),
                    description: Some("Exit plan mode and retain the conversation history.".into()),
                },
                coco_types::PermissionAskChoice {
                    value: "yes-clear-context".into(),
                    label: "Yes, clear context".into(),
                    description: Some("Exit plan mode and start a fresh conversation.".into()),
                },
                coco_types::PermissionAskChoice {
                    value: "no".into(),
                    label: "No, stay in plan mode".into(),
                    description: None,
                },
            ])
        } else {
            None
        };
        coco_types::ToolCheckResult::Ask {
            message: "Exit plan mode?".into(),
            suggestions: vec![],
            choices,
        }
    }

    async fn execute(
        &self,
        input: ExitPlanModeInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let is_agent = ctx.agent_id.is_some();
        let agent_id_str = ctx.agent_id.as_ref().map(|a| a.as_str().to_string());

        // Read the plan. Input (CCR/hook override) wins; otherwise read from
        // the on-disk plan file. TS ExitPlanModeV2Tool.ts:251-253 —
        // `inputPlan ?? getPlan(context.agentId)`.
        //
        // The plans directory is pre-resolved by the engine (respecting the
        // `plansDirectory` setting + project root); fall back to the legacy
        // `config_home`-only resolution for older call sites that haven't
        // migrated to populating `ctx.plans_dir`.
        let input_plan = input.plan.clone();
        let session_id = ctx.session_id_for_history.as_deref();
        let plans_dir = ctx.plans_dir.clone().or_else(|| {
            ctx.config_home
                .as_ref()
                .map(|ch| coco_context::resolve_plans_directory(ch, None, None))
        });

        let file_path: Option<String> = match (session_id, plans_dir.as_ref()) {
            (Some(sid), Some(pd)) => Some(
                coco_context::get_plan_file_path(sid, pd, agent_id_str.as_deref())
                    .to_string_lossy()
                    .into_owned(),
            ),
            _ => None,
        };
        let disk_plan = match (session_id, plans_dir.as_ref()) {
            (Some(sid), Some(pd)) => coco_context::get_plan(sid, pd, agent_id_str.as_deref()),
            _ => None,
        };
        let input_plan_is_edit = match (&input_plan, &disk_plan) {
            (Some(input), Some(disk)) => input != disk,
            (Some(_), None) => true,
            _ => false,
        };
        let plan = input_plan.clone().or(disk_plan);

        // If plan was provided in input (CCR edit), persist to disk so the
        // next reader (VerifyPlanExecution, Read tool) sees the edit.
        //
        // The query layer also injects the current on-disk plan into
        // ExitPlanMode input for hooks/SDK/transcript parity with TS
        // `normalizeToolInput`. Do not treat that byte-identical snapshot
        // as a user edit or rewrite the file unnecessarily.
        if input_plan_is_edit
            && let (Some(plan_content), Some(path)) = (&input_plan, &file_path)
            && let Err(e) = tokio::fs::write(path, plan_content.as_bytes()).await
        {
            tracing::warn!("Failed to persist edited plan to {path}: {e}");
        }

        let has_agent_tool = ctx
            .tools
            .get_by_name(ToolName::Agent.as_str())
            .is_some_and(|t| t.is_enabled(ctx));

        // ── Plan verification (best-effort soft check) ──
        //
        // Off unless the user opted in via
        // `settings.plan_mode.verify_execution` (flag threaded through
        // `ToolUseContext::plan_verify_execution`). When enabled, reads
        // `plan_mode_entry_ms` from app_state (set by EnterPlanMode) and
        // compares against the plan file's mtime. Surfaces the outcome
        // as a `planVerification` field on the result data;
        // `build_instructions` appends an advisory note if the outcome
        // is `NotEdited` or `Missing`.
        let plan_verification = if !ctx.plan_verify_execution {
            None
        } else if let Some(state) = &ctx.app_state {
            let entry_ms = state.read().await.plan_mode_entry_ms.unwrap_or(0);
            file_path.as_deref().and_then(|fp| {
                coco_context::verify_plan_was_edited(std::path::Path::new(fp), entry_ms)
            })
        } else {
            None
        };

        // ── Teammate branch — write plan_approval_request to leader inbox ──
        //
        // TS: ExitPlanModeV2Tool.ts:264-313. If the caller is a teammate
        // whose role requires plan approval, we don't let them exit
        // locally — the plan is serialized and handed off to the team lead
        // via mailbox. The leader sees it, decides, and writes a
        // plan_approval_response back to this teammate's inbox.
        //
        // TS parity: gate on `isTeammate() && isPlanModeRequired()`.
        // Voluntary teammates (is_teammate=true but plan_mode_required=false)
        // fall through to the normal exit path — they restore their mode
        // locally just like a non-swarm session.
        if ctx.is_teammate && ctx.plan_mode_required {
            let Some(plan_text) = plan.as_deref() else {
                return Err(ToolError::InvalidInput {
                    message: format!(
                        "No plan file found at {}. Please write your plan to this file before calling ExitPlanMode.",
                        file_path.as_deref().unwrap_or("(unresolved)"),
                    ),
                    error_code: Some("1".into()),
                });
            };
            if plan_text.trim().is_empty() {
                return Err(ToolError::InvalidInput {
                    message: "Plan file is empty. Write your plan before calling ExitPlanMode."
                        .into(),
                    error_code: Some("1".into()),
                });
            }

            // Swarm identity is pre-resolved by the engine into
            // `ctx.agent_name` + `ctx.team_name` (3-tier fallback done
            // once at ctx build time). Tools read from the typed field,
            // never from process env directly. TS: `getAgentName()` /
            // `getTeamName()`.
            let agent_name = ctx
                .agent_name
                .clone()
                .or_else(|| agent_id_str.clone())
                .unwrap_or_else(|| "unknown".into());
            let team_name = ctx.team_name.clone().unwrap_or_else(|| "default".into());

            let timestamp = chrono::Utc::now().to_rfc3339();
            let short_uuid: String = uuid::Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(8)
                .collect();
            let request_id = format!("plan_approval-{agent_name}-{team_name}-{short_uuid}");

            // Typed protocol message (TS shape preserved via serde
            // renames). Shared schema in `coco_tool_runtime::plan_approval`.
            let approval_msg = PlanApprovalMessage::PlanApprovalRequest(PlanApprovalRequest {
                from: agent_name.clone(),
                timestamp: timestamp.clone(),
                plan_file_path: file_path.clone().unwrap_or_default(),
                plan_content: plan_text.to_string(),
                request_id: request_id.clone(),
            });
            let serialized =
                serde_json::to_string(&approval_msg).map_err(|e| ToolError::ExecutionFailed {
                    message: format!("failed to serialize plan_approval_request: {e}"),
                    display_data: None,
                    source: None,
                })?;
            let envelope = coco_tool_runtime::MailboxEnvelope {
                text: serialized,
                from: agent_name.clone(),
                timestamp: timestamp.clone(),
            };
            // "team-lead" is the canonical inbox name (TS: TEAM_LEAD_NAME).
            ctx.mailbox
                .write_to_mailbox("team-lead", &team_name, envelope)
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    message: format!(
                        "failed to write plan_approval_request to leader mailbox: {e}"
                    ),
                    display_data: None,
                    source: None,
                })?;

            // Queue the "awaiting approval" flags — the teammate
            // stays in plan mode until the leader responds.
            let request_id_for_patch = request_id.clone();
            let awaiting_patch: coco_types::AppStatePatch = Box::new(move |state| {
                state.awaiting_plan_approval = true;
                state.awaiting_plan_approval_request_id = Some(request_id_for_patch);
            });

            let out = ExitPlanModeOutput {
                plan: Some(plan_text.to_string()),
                is_agent: true,
                file_path,
                awaiting_leader_approval: true,
                request_id: Some(request_id),
                ..Default::default()
            };
            return Ok(
                ToolResult::data(serde_json::to_value(&out).unwrap_or_default())
                    .with_patch(awaiting_patch),
            );
        }

        // All mode-related writes happen here. TS parity:
        // `ExitPlanModeV2Tool.ts:357-403` is one big `setAppState` that
        // flips mode → restoreMode, clears prePlanMode, toggles
        // strippedDangerousRules, and sets the exit banner latches.
        //
        // Source of truth: `app_state.pre_plan_mode` (set by
        // EnterPlanMode.execute). Fall back to `ctx.permission_context`
        // for callers without app_state.
        //
        // Auto-mode-exit banner: fires when auto was effectively active
        // during the plan but we're not restoring to Auto. In Rust
        // "auto was active" = `stripped_dangerous_rules.is_some()` OR
        // `pre_plan_mode == Some(Auto)`. TS parity:
        // `autoWasUsedDuringPlan && !finalRestoringAuto`
        // (ExitPlanModeV2Tool.ts:370-378).
        let (pre_plan_from_state, stripped_from_state) = match ctx.app_state.as_ref() {
            Some(state) => {
                let guard = state.read().await;
                (
                    guard.pre_plan_mode,
                    guard.stripped_dangerous_rules.is_some(),
                )
            }
            None => (
                ctx.permission_context.pre_plan_mode,
                ctx.permission_context.stripped_dangerous_rules.is_some(),
            ),
        };
        let restore_mode = pre_plan_from_state.unwrap_or(PermissionMode::Default);
        let auto_was_active_during_plan =
            stripped_from_state || pre_plan_from_state == Some(PermissionMode::Auto);
        let restoring_to_auto = restore_mode == PermissionMode::Auto;
        let needs_auto_exit = auto_was_active_during_plan && !restoring_to_auto;

        // Pre-compute the strip-snapshot (if we'll enter Auto) before
        // building the patch closure. `strip_dangerous_rules`
        // operates on a `ToolPermissionContext` and returns the
        // stashed rules via the `stripped_dangerous_rules` field; we
        // snapshot here so the closure only needs to store the
        // resulting `Option<PermissionRulesBySource>`.
        let snapshotted_stripped_rules = if restoring_to_auto && !stripped_from_state {
            let mut shadow_ctx = ctx.permission_context.clone();
            coco_permissions::dangerous_rules::strip_dangerous_rules(
                &mut shadow_ctx,
                /*is_ant_user*/ false,
            );
            shadow_ctx.stripped_dangerous_rules
        } else {
            None
        };

        // TS parity: `ExitPlanModePermissionRequest.tsx:332-394`. When
        // the multi-choice dialog is enabled, the TUI echoes the picked
        // option as `input.user_choice`. `yes-clear-context` schedules
        // a `MessageHistory::clear()` at the next turn boundary.
        let clear_history_requested = input.user_choice.as_deref() == Some("yes-clear-context");

        // Queue the full ExitPlanMode transition. TS parity:
        // `ExitPlanModeV2Tool.ts:357-403` is one big `setAppState`;
        // our closure is the Rust equivalent of that updater.
        let patch: coco_types::AppStatePatch = Box::new(move |state| {
            state.permission_mode = Some(restore_mode);
            state.pre_plan_mode = None;
            state.has_exited_plan_mode = true;
            state.needs_plan_mode_exit_attachment = true;
            // Mark the plan as awaiting `VerifyPlanExecution`. Cleared
            // when that tool runs or the user resets it.
            // Drives the `verify_plan_reminder` system reminder — so a
            // plan exit always leaves a durable signal behind, not just
            // the one-shot `needs_plan_mode_exit_attachment` that the
            // reminder subsystem consumes on the next turn.
            state.pending_plan_verification = true;
            if needs_auto_exit {
                state.needs_auto_mode_exit_attachment = true;
            }
            // Dangerous-rules stash management on Auto boundary.
            // (The strip happens on Plan→Auto, restore on
            // Auto→non-Auto; see TS ExitPlanModeV2Tool.ts:380-394.)
            if restoring_to_auto && state.stripped_dangerous_rules.is_none() {
                state.stripped_dangerous_rules = snapshotted_stripped_rules;
            } else if !restoring_to_auto && state.stripped_dangerous_rules.is_some() {
                state.stripped_dangerous_rules = None;
            }
            if clear_history_requested {
                state.pending_clear_message_history = true;
            }
        });

        let out = ExitPlanModeOutput {
            plan,
            is_agent,
            file_path,
            has_task_tool: if has_agent_tool { Some(true) } else { None },
            plan_was_edited: if input_plan_is_edit { Some(true) } else { None },
            plan_verification,
            ..Default::default()
        };
        Ok(ToolResult::data(serde_json::to_value(&out).unwrap_or_default()).with_patch(patch))
    }

    fn render_for_model(&self, data: &Value) -> Vec<ToolResultContentPart> {
        // TS parity: `mapToolResultToToolResultBlockParam` in
        // `ExitPlanModeV2Tool.ts:419-492`. The four-variant
        // formatting (teammate-awaiting / sub-agent / empty / approved)
        // already lives in [`ExitPlanModeTool::build_instructions`];
        // this is the trait hook that finally wires it up to the
        // executor's render path. Pre-refactor this method was dead
        // code — `tool_outcome_builder.rs` JSON-stringified `data`.
        vec![ToolResultContentPart::Text {
            text: Self::build_instructions(data),
            provider_options: None,
        }]
    }
}

impl ExitPlanModeTool {
    /// Build the instructions text returned to the model as tool_result content.
    ///
    /// TS: `mapToolResultToToolResultBlockParam` in ExitPlanModeV2Tool.ts
    pub fn build_instructions(result_data: &Value) -> String {
        let out: ExitPlanModeOutput =
            serde_json::from_value(result_data.clone()).unwrap_or_default();

        // Teammate "awaiting approval" branch — TS ExitPlanModeV2Tool.ts:431-450.
        if out.awaiting_leader_approval {
            let request_id = out.request_id.as_deref().unwrap_or("(unknown)");
            let file_path_str = out.file_path.as_deref().unwrap_or("(unknown)");
            return format!(
                "Your plan has been submitted to the team lead for approval.\n\n\
                 Plan file: {file_path_str}\n\n\
                 **What happens next:**\n\
                 1. Wait for the team lead to review your plan\n\
                 2. You will receive a message in your inbox with approval/rejection\n\
                 3. If approved, you can proceed with implementation\n\
                 4. If rejected, refine your plan based on the feedback\n\n\
                 **Important:** Do NOT proceed until you receive approval. Check \
                 your inbox for response.\n\n\
                 Request ID: {request_id}"
            );
        }

        if out.is_agent {
            return "User has approved the plan. There is nothing else needed \
                    from you now. Please respond with \"ok\""
                .to_string();
        }

        let plan_text = out.plan.as_deref().unwrap_or("");
        if plan_text.trim().is_empty() {
            return "User has approved exiting plan mode. You can now proceed.".to_string();
        }

        // Optional stale-plan advisory. Never blocks, just appends a note.
        let verification_note = match out.plan_verification {
            Some(coco_context::PlanVerificationOutcome::NotEdited) => {
                "\n\n**Heads up:** the plan file mtime suggests you \
                didn't edit it during plan mode. Review the plan before proceeding \
                — it may not reflect your intended approach."
            }
            Some(coco_context::PlanVerificationOutcome::Missing) => {
                "\n\n**Heads up:** the plan file is missing. Review \
                your implementation approach before proceeding."
            }
            _ => "",
        };
        let team_hint = if out.has_task_tool.unwrap_or(false) {
            "\n\nIf this plan can be broken down into multiple independent tasks, \
             consider using the TeamCreate tool to create a team and parallelize \
             the work."
        } else {
            ""
        };
        let plan_label = if out.plan_was_edited.unwrap_or(false) {
            "Approved Plan (edited by user)"
        } else {
            "Approved Plan"
        };
        let path_note = out
            .file_path
            .as_deref()
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
             {path_note}{team_hint}{verification_note}\n\n\
             ## {plan_label}:\n{plan_text}"
        )
    }
}

#[cfg(test)]
#[path = "plan_mode.test.rs"]
mod tests;
