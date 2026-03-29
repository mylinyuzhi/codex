//! Plan mode generators.
//!
//! These generators handle all plan mode workflow reminders:
//! - `PlanModeEnterGenerator`: Instructions when entering plan mode
//! - `PlanToolReminderGenerator`: Periodic reminder to use Write/Edit tools for the plan
//! - `PlanFileReferenceGenerator`: Plan file reference after context compaction
//! - `SubagentPlanReminderGenerator`: Simplified instructions for subagents in plan mode
//! - `PlanModeExitGenerator`: One-time instructions when exiting plan mode after approval
//! - `PlanVerificationGenerator`: Reminder to verify changes after all todos are completed

use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::generator::TodoStatus;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

// ---------------------------------------------------------------------------
// Phase 4 adaptive variants (Gap 5)
// ---------------------------------------------------------------------------

/// Phase 4 instruction variant, controlled by configuration.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase4Variant {
    /// Full instructions with Context section and verification.
    #[default]
    Full,
    /// Trimmed: one-line Context, simplified verification.
    Trimmed,
    /// Compact: no Context section, one line per file, under 40 lines.
    Compact,
    /// Minimal: hard 40-line limit, no prose.
    Minimal,
}

/// Full Phase 4 text (the existing default).
const PHASE4_FULL: &str = "\
### Phase 4: Write the Final Plan
- Write your plan to the plan file using the Write tool
- Begin with a **Context** section: explain why this change is being made
- Include only your recommended approach, not all alternatives
- Reference existing functions and utilities you found that should be reused, with their file paths
- Include specific file paths and changes for each step
- Include a verification section describing how to test the changes end-to-end";

/// Trimmed Phase 4: one-line Context, simplified verification.
const PHASE4_TRIMMED: &str = "\
### Phase 4: Write the Final Plan
Goal: Write your final plan to the plan file.
- One-line **Context**: what is being changed and why
- Include only your recommended approach
- List paths of files to modify
- Reference existing functions to reuse, with file paths
- End with **Verification**: the single command to confirm the change works";

/// Compact Phase 4: no Context section, one line per file, under 40 lines.
const PHASE4_COMPACT: &str = "\
### Phase 4: Write the Final Plan
Goal: Write your final plan to the plan file.
- Do NOT write a Context or Background section
- List paths of files to modify and changes in each (one line per file)
- Reference existing functions to reuse, with file paths
- End with **Verification**: the single command that confirms the change works
- Most good plans are under 40 lines. Prose is a sign you are padding.";

/// Minimal Phase 4: hard 40-line limit, no prose.
const PHASE4_MINIMAL: &str = "\
### Phase 4: Write the Final Plan
Goal: Write your final plan to the plan file.
- Do NOT write a Context, Background, or Overview section
- Do NOT restate the user's request. Do NOT write prose paragraphs.
- List paths of files to modify and changes in each (one bullet per file)
- Reference existing functions to reuse, with file:line
- End with the single verification command
- **Hard limit: 40 lines.** If the plan is longer, delete prose \u{2014} not file paths.";

/// Select Phase 4 instructions by variant.
pub fn phase4_instructions(variant: Phase4Variant) -> &'static str {
    match variant {
        Phase4Variant::Full => PHASE4_FULL,
        Phase4Variant::Trimmed => PHASE4_TRIMMED,
        Phase4Variant::Compact => PHASE4_COMPACT,
        Phase4Variant::Minimal => PHASE4_MINIMAL,
    }
}

/// Generator for plan mode entry instructions.
///
/// Provides the 5-phase workflow instructions when the agent
/// enters plan mode.
#[derive(Debug)]
pub struct PlanModeEnterGenerator;

#[async_trait]
impl AttachmentGenerator for PlanModeEnterGenerator {
    fn name(&self) -> &str {
        "PlanModeEnterGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::PlanModeEnter
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.plan_mode_enter
    }

    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig::plan_mode()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if !ctx.is_plan_mode {
            return Ok(None);
        }

        // Build instructions with plan file path if available
        let plan_path_info = ctx
            .plan_file_path
            .as_ref()
            .map(|p| {
                let path_display = p.display();
                let exists = p.exists();
                if exists {
                    format!(
                        "\n\n## Plan File\n\n\
                         A plan file already exists at `{path_display}`. You can read it and make incremental edits using the Edit tool.\n\n\
                         You should build your plan incrementally by writing to or editing this file. \
                         NOTE that this is the only file you are allowed to edit - other than this you are only allowed to take READ-ONLY actions."
                    )
                } else {
                    format!(
                        "\n\n## Plan File\n\n\
                         No plan file exists yet. You should create your plan at `{path_display}` using the Write tool.\n\n\
                         You should build your plan incrementally by writing to or editing this file. \
                         NOTE that this is the only file you are allowed to edit - other than this you are only allowed to take READ-ONLY actions."
                    )
                }
            })
            .unwrap_or_default();

        // Ultraplan: plan was pre-written by a remote session; instruct the
        // agent to call ExitPlanMode immediately without exploring.
        if ctx.is_ultraplan {
            let plan_path = ctx
                .plan_file_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            let content = format!(
                "Ultraplan complete. The plan has been pre-written to the plan file ({plan_path}) \
                 by the remote planning session. Do NOT read files, explore the codebase, or modify \
                 anything. Your ONLY permitted action is to call ExitPlanMode immediately to present \
                 the plan to the user for approval."
            );
            return Ok(Some(SystemReminder::new(
                AttachmentType::PlanModeEnter,
                content,
            )));
        }

        // Use per-generator full-content flag (pre-computed by orchestrator).
        // Priority: reentry > interview > full > sparse.
        let content = if ctx.is_plan_reentry {
            format!("{PLAN_MODE_REENTRY_INSTRUCTIONS}{plan_path_info}")
        } else if ctx.is_plan_interview_phase {
            if ctx.should_use_full_content(self.attachment_type()) {
                format!("{PLAN_MODE_INTERVIEW_INSTRUCTIONS}{plan_path_info}")
            } else {
                format!("{PLAN_MODE_INTERVIEW_SPARSE_INSTRUCTIONS}{plan_path_info}")
            }
        } else if ctx.should_use_full_content(self.attachment_type()) {
            let full = build_full_instructions(
                ctx.phase4_variant,
                ctx.explore_agent_count,
                ctx.plan_agent_count,
            );
            format!("{full}{plan_path_info}")
        } else {
            format!("{PLAN_MODE_SPARSE_INSTRUCTIONS}{plan_path_info}")
        };

        Ok(Some(SystemReminder::new(
            AttachmentType::PlanModeEnter,
            content,
        )))
    }
}

/// Generator for plan tool reminders.
///
/// Periodically reminds the agent to use Write/Edit tools
/// when in plan mode.
#[derive(Debug)]
pub struct PlanToolReminderGenerator;

#[async_trait]
impl AttachmentGenerator for PlanToolReminderGenerator {
    fn name(&self) -> &str {
        "PlanToolReminderGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::PlanToolReminder
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.plan_tool_reminder
    }

    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig::plan_tool_reminder()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if !ctx.is_plan_mode {
            return Ok(None);
        }

        // Only remind if there's a plan file path
        let Some(plan_path) = &ctx.plan_file_path else {
            return Ok(None);
        };

        let content = format!(
            "Reminder: You are in plan mode. Use the Write tool to create or replace your plan, \
             or the Edit tool to modify it at:\n\
             `{}`\n\n\
             When your plan is ready for approval, use ExitPlanMode to submit it for review.",
            plan_path.display()
        );

        Ok(Some(SystemReminder::new(
            AttachmentType::PlanToolReminder,
            content,
        )))
    }
}

/// Generator for plan file reference after compaction.
///
/// After context compaction, the conversation history is truncated and the plan
/// context may be lost. This generator injects a reference to the plan file
/// so the model knows the plan still exists and can read it.
///
/// This is the cocode-rs equivalent of Claude Code's `plan_file_reference`
/// attachment that survives compaction.
#[derive(Debug)]
pub struct PlanFileReferenceGenerator;

#[async_trait]
impl AttachmentGenerator for PlanFileReferenceGenerator {
    fn name(&self) -> &str {
        "PlanFileReferenceGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::PlanModeFileReference
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.plan_mode_enter
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // No throttle - inject whenever there's a restored plan
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        // Only fire when there's a restored plan (after compaction)
        let Some(restored) = &ctx.restored_plan else {
            return Ok(None);
        };

        let plan_summary = if restored.content.len() > 500 {
            let end = restored.content.floor_char_boundary(500);
            format!(
                "{}...\n\n(truncated — read full plan from file)",
                &restored.content[..end]
            )
        } else {
            restored.content.clone()
        };

        let content = format!(
            "## Plan File Reference\n\n\
             A plan file exists from earlier in this conversation. After context compaction, \
             the full conversation history has been condensed, but your plan remains at:\n\n\
             `{}`\n\n\
             ### Plan Summary\n\n\
             {}\n\n\
             If you need the full plan, use the Read tool to read the file above.",
            restored.file_path.display(),
            plan_summary,
        );

        Ok(Some(SystemReminder::new(
            AttachmentType::PlanModeFileReference,
            content,
        )))
    }
}

/// Generator for subagent plan mode reminder.
///
/// When the main agent is in plan mode and spawns a subagent (e.g., Explore),
/// this generator provides simplified plan instructions so the subagent knows
/// the context: that it's exploring for a plan, not for implementation.
///
/// This is the cocode-rs equivalent of Claude Code's `q2z` subagent plan reminder.
#[derive(Debug)]
pub struct SubagentPlanReminderGenerator;

#[async_trait]
impl AttachmentGenerator for SubagentPlanReminderGenerator {
    fn name(&self) -> &str {
        "SubagentPlanReminderGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::PlanModeEnter
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.plan_mode_enter
    }

    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig::plan_mode()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        // Only fire for subagents when plan mode is active
        if ctx.is_main_agent || !ctx.is_plan_mode {
            return Ok(None);
        }

        let plan_path_info = ctx
            .plan_file_path
            .as_ref()
            .map(|p| format!("\nPlan file: `{}`", p.display()))
            .unwrap_or_default();

        let content = format!("{SUBAGENT_PLAN_REMINDER}{plan_path_info}");

        Ok(Some(SystemReminder::new(
            AttachmentType::PlanModeEnter,
            content,
        )))
    }
}

/// Subagent plan instructions (aligned with CC v2.1.76's `q2z`).
const SUBAGENT_PLAN_REMINDER: &str = r#"## Context: Plan Mode Active

Plan mode is active. The user indicated that they do not want you to execute yet -- you MUST NOT make any edits, run any non-readonly tools (including changing configs or making commits), or otherwise make any changes to the system. This supercedes any other instructions you have received.

Answer the user's query comprehensively, using the AskUserQuestion tool if you need to ask the user clarifying questions. If you do use AskUserQuestion, make sure to ask all clarifying questions you need to fully understand the user's intent before proceeding."#;

/// Build full plan mode instructions with the given Phase 4 variant and
/// dynamic agent counts (Gap 5 + Gap 6).
fn build_full_instructions(phase4: Phase4Variant, explore_count: i32, plan_count: i32) -> String {
    let phase4_text = phase4_instructions(phase4);
    let plan_agent_line = if plan_count == 1 {
        "- Default: Launch at least 1 Plan agent for most tasks \u{2014} it helps validate your understanding and consider alternatives".to_string()
    } else {
        format!(
            "- Default: Launch at least {plan_count} Plan agents for most tasks \u{2014} they help validate your understanding and consider alternatives"
        )
    };
    format!(
        "\
## Plan Mode Active

Plan mode is active. The user indicated that they do not want you to execute yet -- you MUST NOT make any edits (with the exception of the plan file mentioned below), run any non-readonly tools (including changing configs or making commits), or otherwise make any changes to the system. This supercedes any other instructions.

You should build your plan incrementally by writing to or editing the plan file. NOTE that this is the only file you are allowed to edit.

## Plan Workflow

Follow this 5-phase workflow:

### Phase 1: Initial Understanding and Exploration
- Read and analyze the user's request
- Identify key requirements and constraints
- Actively search for existing functions, utilities, and patterns that can be reused \u{2014} avoid proposing new code when suitable implementations already exist
- Launch Explore agents IN PARALLEL to search the codebase
  - Use 1 agent when the task is isolated to known files or a single area of the codebase
  - Use multiple agents when: the scope is uncertain, multiple areas of the codebase are involved, or you need to cross-reference different subsystems
  - Quality over quantity \u{2014} {explore_count} agents maximum, but you should try to use the minimum number of agents necessary
- Each agent should search for existing patterns, identify files to modify, and note dependencies
- Wait for all exploration results before proceeding

### Phase 2: Design
- Read the critical files identified by agents to deepen your understanding
- Synthesize exploration findings into a step-by-step implementation plan
{plan_agent_line}
- Skip agents: Only for truly trivial tasks (typo fixes, single-line changes, simple renames)
- Consider edge cases and error handling
- Document any assumptions

### Phase 3: Review and Clarify
- Read the critical files identified by agents to deepen your understanding
- Ensure that the plans align with the user's original request
- Use AskUserQuestion to clarify any remaining questions with the user

{phase4_text}

### Phase 5: Call ExitPlanMode
- Use ExitPlanMode when ready for user approval
- This is critical \u{2014} your turn should only end with either using the AskUserQuestion tool OR calling ExitPlanMode. Do not stop unless it's for these 2 reasons

## Available Tools in Plan Mode

Read-only tools you CAN use: Read, Glob, Grep, Bash (read-only commands like ls, git status, git log), WebFetch, WebSearch, LSP, AskUserQuestion, Task (Explore and Plan subagent types only)

Write-only exception: Write and Edit tools ONLY for the plan file above.

Tools you CANNOT use: Bash (write commands), Edit/Write (non-plan files), NotebookEdit, or any tool that modifies the system.

## Important

- End turns with AskUserQuestion (for clarifications) or ExitPlanMode (for plan approval)
- Never ask about plan approval via text or AskUserQuestion \u{2014} use ExitPlanMode instead. Do NOT use AskUserQuestion to ask \"Is this plan okay?\", \"Should I proceed?\", or \"Does this look good?\" \u{2014} that is exactly what ExitPlanMode does
- Do NOT make code changes while in plan mode. Focus only on planning."
    )
}

/// Sparse plan mode instructions shown on subsequent turns.
const PLAN_MODE_SPARSE_INSTRUCTIONS: &str = r#"## Plan Mode Active

Plan mode still active (see full instructions earlier in conversation).

Read-only except plan file. Follow 5-phase workflow.

End turns with AskUserQuestion (for clarifications) or ExitPlanMode (for plan approval).

Never ask about plan approval via text or AskUserQuestion -- use ExitPlanMode instead."#;

// ---------------------------------------------------------------------------
// Interview phase instructions (gated by Feature::PlanModeInterview)
// ---------------------------------------------------------------------------

/// Full interview-style plan mode instructions (aligned with Claude Code's `ezz`).
///
/// Replaces the 5-phase workflow with an iterative pair-planning loop:
/// explore → update plan → ask user → repeat.
const PLAN_MODE_INTERVIEW_INSTRUCTIONS: &str = r#"## Plan Mode Active

Plan mode is active. The user indicated that they do not want you to execute yet -- you MUST NOT make any edits (with the exception of the plan file mentioned below), run any non-readonly tools (including changing configs or making commits), or otherwise make any changes to the system. This supercedes any other instructions you have received.

You should build your plan incrementally by writing to or editing the plan file. NOTE that this is the only file you are allowed to edit - other than this you are only allowed to take READ-ONLY actions.

## Iterative Planning Workflow

You are pair-planning with the user. Explore the code to build context, ask the user questions when you hit decisions you can't make alone, and write your findings into the plan file as you go.

### The Loop

Repeat this cycle until the plan is complete:

1. **Explore** — Use Read, Glob, Grep to read code. Look for existing functions, utilities, and patterns to reuse. Understand the codebase structure deeply.
2. **Update the plan file** — After each discovery, immediately capture what you learned. Don't wait until the end.
3. **Ask the user** — When you hit an ambiguity or decision you can't resolve from code alone, use AskUserQuestion. Then go back to step 1.

### First Turn

Start by quickly scanning a few key files to form an initial understanding of the task scope. Then write a skeleton plan (headers and rough notes) and ask the user your first round of questions. Don't explore exhaustively before engaging the user.

### Asking Good Questions

- Never ask what you could find out by reading the code
- Batch related questions together (use multi-question AskUserQuestion calls)
- Focus on things only the user can answer: requirements, preferences, tradeoffs, edge case priorities
- Scale depth to the task — a vague feature request needs many rounds; a focused bug fix may need one or none

### When to Converge

Your plan is ready when you have:
- Understood the codebase structure and existing patterns
- Clarified all critical requirements with the user
- Identified the concrete implementation approach
Call ExitPlanMode when ready.

### Ending Your Turn

Your turn should only end by either:
- Using AskUserQuestion to gather more information
- Calling ExitPlanMode when the plan is ready for approval

## Available Tools in Plan Mode

Read-only: Read, Glob, Grep, Bash (read-only commands only), WebFetch, WebSearch, LSP, AskUserQuestion, Task (Explore/Plan subagents only). Write/Edit ONLY for the plan file.

**Important:** Use AskUserQuestion ONLY to clarify requirements or choose between approaches. Use ExitPlanMode to request plan approval. Do NOT ask about plan approval in any other way."#;

/// Sparse interview-style instructions for subsequent turns.
const PLAN_MODE_INTERVIEW_SPARSE_INSTRUCTIONS: &str = r#"## Plan Mode Active

Plan mode still active (see full instructions earlier in conversation).

Read-only except plan file. Follow iterative workflow: explore codebase, interview user, write to plan incrementally.

End turns with AskUserQuestion (for clarifications) or ExitPlanMode (for plan approval).

Never ask about plan approval via text or AskUserQuestion -- use ExitPlanMode instead."#;

/// Instructions shown when re-entering plan mode (after a previous exit).
///
/// Guides the LLM to evaluate whether the user's new request is for
/// the same task (modify existing plan) or a different task (overwrite).
const PLAN_MODE_REENTRY_INSTRUCTIONS: &str = r#"## Plan Mode Re-entered

You are re-entering plan mode. There is an existing plan from a previous session.

### Evaluate the Existing Plan

1. **Read the existing plan file** to understand what was previously planned
2. **Compare the user's current request** with the existing plan's scope
3. **Decide your approach**:
   - If the request is for the **same task** or a continuation → **modify** the existing plan (update, extend, or refine it)
   - If the request is for a **different task** entirely → **overwrite** the plan file with a new plan

### Important

- You are in plan mode: read-only except for the plan file
- Follow the 5-phase workflow (Understand → Explore → Design → Document → Review)
- End turns with AskUserQuestion (for clarifications) or ExitPlanMode (for plan approval)
- Never ask about plan approval via text or AskUserQuestion -- use ExitPlanMode instead"#;

// ---------------------------------------------------------------------------
// Plan mode exit generator
// ---------------------------------------------------------------------------

/// Generator for plan mode exit instructions.
///
/// Provides one-time instructions when the plan has been approved
/// and the agent is transitioning out of plan mode to implementation.
#[derive(Debug)]
pub struct PlanModeExitGenerator;

#[async_trait]
impl AttachmentGenerator for PlanModeExitGenerator {
    fn name(&self) -> &str {
        "PlanModeExitGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::PlanModeExit
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.plan_mode_exit
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // No throttle - this is a one-time injection
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        // Only trigger when plan mode exit is pending
        if !ctx.plan_mode_exit_pending {
            return Ok(None);
        }

        // Conditionally append plan file reference (CC does this).
        let plan_ref = if let Some(plan_path) = &ctx.plan_file_path {
            if plan_path.exists() {
                format!(
                    " The plan file is located at `{}` if you need to reference it.",
                    plan_path.display()
                )
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let content = format!("{PLAN_MODE_EXIT_INSTRUCTIONS}{plan_ref}");

        Ok(Some(SystemReminder::new(
            AttachmentType::PlanModeExit,
            content,
        )))
    }
}

/// Instructions for transitioning out of plan mode (aligned with CC v2.1.76).
const PLAN_MODE_EXIT_INSTRUCTIONS: &str = r#"## Exited Plan Mode

You have exited plan mode. You can now make edits, run tools, and take actions."#;

// ---------------------------------------------------------------------------
// Plan verification generator
// ---------------------------------------------------------------------------

/// Subagent tool name (cocode-rs's Task tool).
const SUB_AGENT_TOOL_NAME: &str = cocode_protocol::ToolName::Task.as_str();

/// Generator for plan verification reminders.
///
/// Fires when ALL of these conditions hold:
/// 1. Main agent only (not subagents)
/// 2. Not in plan mode (implementation phase)
/// 3. A plan file exists (plan was created)
/// 4. There are tracked todo items
/// 5. All todos have `Completed` status
#[derive(Debug)]
pub struct PlanVerificationGenerator;

#[async_trait]
impl AttachmentGenerator for PlanVerificationGenerator {
    fn name(&self) -> &str {
        "PlanVerificationGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::PlanVerification
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.plan_verification
    }

    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig {
            min_turns_between: 5,
            ..ThrottleConfig::default()
        }
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        // Main agent only (CC tier: MainAgentOnly).
        if !ctx.is_main_agent {
            return Ok(None);
        }
        // Only during implementation (not in plan mode).
        if ctx.is_plan_mode {
            return Ok(None);
        }
        // Only if a plan was created.
        if ctx.plan_file_path.is_none() {
            return Ok(None);
        }
        // Only if there are todo items and all are completed.
        if ctx.todos.is_empty() {
            return Ok(None);
        }
        if !ctx.todos.iter().all(|t| t.status == TodoStatus::Completed) {
            return Ok(None);
        }

        let content = format!(
            "You have completed implementing the plan. \
             Please verify that all changes are correct by reviewing the modified files \
             and running relevant tests. Do NOT delegate verification to the \
             {SUB_AGENT_TOOL_NAME} tool or an agent.",
        );

        Ok(Some(SystemReminder::new(
            AttachmentType::PlanVerification,
            content,
        )))
    }
}

#[cfg(test)]
#[path = "plan_mode.test.rs"]
mod tests;
