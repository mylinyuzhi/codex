//! Bundled skill definitions shipped with the binary.
//!
//! TS source: `skills/bundledSkills.ts` (220 LOC) + `skills/bundled/*.ts`
//! (registry in `skills/bundled/index.ts:24-78`).
//!
//! ## TS-mirroring rules
//!
//! 1. **Inventory**: matches `skills/bundled/index.ts` exactly. Skills that TS
//!    ships only as gated registrations (`feature(...)` calls) are gated here
//!    via `gated_by: Some(Feature::*)`. Skills that TS ships as ant-only
//!    (`if (process.env.USER_TYPE !== 'ant') return`) are gated via a
//!    user-type check at registration time.
//! 2. **No Rust-only extras**: TS does NOT ship `commit`, `review-pr`, or
//!    `pdf` as bundled skills (`/commit` is a top-level `commands/commit.ts`,
//!    `review-pr` is covered by `commands/review.ts`, and PDF reading is
//!    handled by the Read tool). They were removed in Round 11.
//! 3. **Feature flags**: each gated skill maps to a `coco_types::Feature`
//!    variant — see `features.rs` and `parity-skills-commands-plugins.md §1.3`.
//! 4. **`disable_model_invocation` / `user_invocable`**: matched per-skill to
//!    TS frontmatter.

use coco_types::Feature;
use coco_types::ToolName;
use coco_types::UserType;
use std::collections::HashMap;

use crate::SkillContext;
use crate::SkillDefinition;
use crate::SkillSource;

/// Create a bundled skill with common defaults.
fn bundled(
    name: &str,
    description: &str,
    prompt: &str,
    allowed_tools: Vec<&str>,
) -> SkillDefinition {
    SkillDefinition {
        name: name.to_string(),
        display_name: None,
        description: description.to_string(),
        prompt: prompt.to_string(),
        source: SkillSource::Bundled,
        aliases: vec![],
        allowed_tools: Some(allowed_tools.into_iter().map(String::from).collect()),
        model: None,
        model_role: None,
        when_to_use: None,
        argument_names: vec![],
        paths: vec![],
        effort: None,
        context: SkillContext::Inline,
        agent: None,
        version: None,
        disabled: false,
        hooks: None,
        argument_hint: None,
        user_invocable: true,
        disable_model_invocation: false,
        shell: None,
        content_length: prompt.len() as i64,
        has_user_specified_description: true,
        progress_message: Some("running".to_string()),
        is_hidden: false,
        gated_by: None,
        files: HashMap::new(),
        skill_root: None,
    }
}

/// Get all bundled skill definitions.
///
/// **Selection logic mirrors TS `skills/bundled/index.ts:initBundledSkills`**:
/// - 5 unconditional skills are always present (update-config, keybindings,
///   loremIpsum, batch — note: TS gates loremIpsum on USER_TYPE='ant', so we
///   gate via [`UserType`] argument).
/// - 5 ant-only skills (verify, debug, skillify, remember, simplify, stuck)
///   present only when `user_type == Ant`.
/// - 7 feature-gated skills (loop, schedule, claude-api, dream, hunter,
///   claude-in-chrome, run-skill-generator) carry `gated_by: Some(Feature::*)`
///   and are filtered by `SkillManager::visible(features)`.
pub fn get_bundled_skills(user_type: UserType) -> Vec<SkillDefinition> {
    let mut skills: Vec<SkillDefinition> = Vec::new();

    // ───────────────── unconditional ─────────────────

    // /update-config — TS: skills/bundled/updateConfig.ts (unconditional)
    {
        let mut s = bundled(
            "update-config",
            "Configure settings via settings.json",
            include_str!("bundled_prompts/update_config.txt"),
            vec![
                ToolName::Read.as_str(),
                ToolName::Edit.as_str(),
                ToolName::Write.as_str(),
                ToolName::Glob.as_str(),
            ],
        );
        s.when_to_use = Some("For hooks, permissions, env vars, or settings changes".to_string());
        skills.push(s);
    }

    // /keybindings-help — TS: skills/bundled/keybindings.ts (unconditional)
    // TS uses `userInvocable: false` so only the model invokes.
    {
        let mut s = bundled(
            "keybindings-help",
            "Customize keyboard shortcuts and keybindings",
            include_str!("bundled_prompts/keybindings_help.txt"),
            vec![
                ToolName::Read.as_str(),
                ToolName::Edit.as_str(),
                ToolName::Write.as_str(),
            ],
        );
        s.when_to_use = Some(
            "When the user wants to customize keyboard shortcuts, rebind keys, add chord bindings, or modify keybindings.json".to_string(),
        );
        s.user_invocable = false;
        s.is_hidden = true; // matches TS isHidden = !userInvocable
        skills.push(s);
    }

    // /batch — TS: skills/bundled/batch.ts (unconditional, disable_model_invocation)
    {
        let mut s = bundled(
            "batch",
            "Run a prompt or command on multiple files",
            include_str!("bundled_prompts/batch.txt"),
            vec![
                ToolName::Bash.as_str(),
                ToolName::Read.as_str(),
                ToolName::Glob.as_str(),
                ToolName::Grep.as_str(),
                ToolName::Edit.as_str(),
                ToolName::Write.as_str(),
            ],
        );
        s.when_to_use =
            Some("When user wants sweeping mechanical changes across many files".to_string());
        s.disable_model_invocation = true;
        skills.push(s);
    }

    // ───────────────── ant-only ─────────────────

    if user_type.is_ant() {
        // /verify — TS: skills/bundled/verify.ts (ant-only)
        skills.push(bundled(
            "verify",
            "Verify a code change does what it should by running the app",
            include_str!("bundled_prompts/verify.txt"),
            vec![
                ToolName::Bash.as_str(),
                ToolName::Read.as_str(),
                ToolName::Glob.as_str(),
                ToolName::Grep.as_str(),
            ],
        ));

        // /debug — TS: skills/bundled/debug.ts (ant-only, disable_model_invocation)
        let mut debug_skill = bundled(
            "debug",
            "Debug your current Claude Code session by reading the session debug log. Includes all event logging",
            include_str!("bundled_prompts/debug.txt"),
            vec![
                ToolName::Bash.as_str(),
                ToolName::Read.as_str(),
                ToolName::Glob.as_str(),
                ToolName::Grep.as_str(),
            ],
        );
        debug_skill.disable_model_invocation = true;
        skills.push(debug_skill);

        // /skillify — TS: skills/bundled/skillify.ts (ant-only)
        let mut sk = bundled(
            "skillify",
            "Convert a workflow into a reusable skill file",
            include_str!("bundled_prompts/skillify.txt"),
            vec![
                ToolName::Read.as_str(),
                ToolName::Write.as_str(),
                ToolName::Glob.as_str(),
                ToolName::Grep.as_str(),
            ],
        );
        sk.when_to_use = Some("When user wants to automate a repeatable workflow".to_string());
        skills.push(sk);

        // /remember — TS: skills/bundled/remember.ts (ant-only)
        let mut rem = bundled(
            "remember",
            "Save information to memory for future conversations",
            include_str!("bundled_prompts/remember.txt"),
            vec![
                ToolName::Read.as_str(),
                ToolName::Write.as_str(),
                ToolName::Glob.as_str(),
            ],
        );
        rem.when_to_use = Some("Use when reviewing or organizing memory entries".to_string());
        skills.push(rem);

        // /simplify — TS: skills/bundled/simplify.ts (ant-only)
        skills.push(bundled(
            "simplify",
            "Review changed code for reuse, quality, and efficiency, then fix any issues found",
            include_str!("bundled_prompts/simplify.txt"),
            vec![
                ToolName::Bash.as_str(),
                ToolName::Read.as_str(),
                ToolName::Glob.as_str(),
                ToolName::Grep.as_str(),
                ToolName::Edit.as_str(),
                ToolName::Write.as_str(),
            ],
        ));

        // /stuck — TS: skills/bundled/stuck.ts (ant-only)
        skills.push(bundled(
            "stuck",
            "Help when stuck in loops or debugging dead ends",
            include_str!("bundled_prompts/stuck.txt"),
            vec![
                ToolName::Read.as_str(),
                ToolName::Glob.as_str(),
                ToolName::Grep.as_str(),
                ToolName::Bash.as_str(),
            ],
        ));

        // /lorem-ipsum — TS: skills/bundled/loremIpsum.ts (ant-only)
        let mut li = bundled(
            "lorem-ipsum",
            "Generate filler text for long context testing. Specify token count as argument (e.g., /lorem-ipsum 50000). Outputs approximately the requested number of tokens. Ant-only.",
            include_str!("bundled_prompts/lorem_ipsum.txt"),
            vec![],
        );
        li.argument_hint = Some("[token_count]".to_string());
        skills.push(li);
    }

    // ───────────────── feature-gated ─────────────────

    // /loop — TS: skills/bundled/loop.ts (gated AGENT_TRIGGERS)
    {
        let mut s = bundled(
            "loop",
            "Run a prompt or slash command on a recurring interval",
            include_str!("bundled_prompts/loop.txt"),
            vec![
                ToolName::Bash.as_str(),
                ToolName::Read.as_str(),
                ToolName::Glob.as_str(),
                ToolName::Grep.as_str(),
            ],
        );
        s.gated_by = Some(Feature::AgentTriggers);
        skills.push(s);
    }

    // /schedule — TS: skills/bundled/scheduleRemoteAgents.ts (gated AGENT_TRIGGERS_REMOTE)
    {
        let mut s = bundled(
            "schedule",
            "Create, update, list, or run scheduled remote agents (triggers) that execute on a cron schedule",
            include_str!("bundled_prompts/schedule.txt"),
            vec![
                ToolName::RemoteTrigger.as_str(),
                ToolName::AskUserQuestion.as_str(),
            ],
        );
        s.when_to_use = Some(
            "When user wants to schedule recurring remote agents or manage scheduled triggers"
                .to_string(),
        );
        s.gated_by = Some(Feature::AgentTriggersRemote);
        skills.push(s);
    }

    // /claude-api — TS: skills/bundled/claudeApi.ts (gated BUILDING_CLAUDE_APPS)
    {
        let mut s = bundled(
            "claude-api",
            "Build apps with the Claude API or Anthropic SDK",
            include_str!("bundled_prompts/claude_api.txt"),
            vec![
                ToolName::Read.as_str(),
                ToolName::Grep.as_str(),
                ToolName::Glob.as_str(),
                ToolName::WebFetch.as_str(),
            ],
        );
        s.when_to_use = Some(
            "When code imports anthropic SDK or user asks about Claude API, Anthropic SDKs, or Agent SDK".to_string(),
        );
        s.gated_by = Some(Feature::BuildingClaudeApps);
        skills.push(s);
    }

    // /dream — TS: skills/bundled/dream.ts (gated KAIROS|KAIROS_DREAM)
    {
        let mut s = bundled(
            "dream",
            "Run KAIROS auto-dream memory consolidation: review and consolidate session memory",
            include_str!("bundled_prompts/dream.txt"),
            vec![
                ToolName::Read.as_str(),
                ToolName::Edit.as_str(),
                ToolName::Write.as_str(),
                ToolName::Grep.as_str(),
                ToolName::Glob.as_str(),
            ],
        );
        s.gated_by = Some(Feature::KairosDream);
        skills.push(s);
    }

    // /hunter — TS: skills/bundled/hunter.ts (gated REVIEW_ARTIFACT)
    {
        let mut s = bundled(
            "hunter",
            "Deep bug-finding review: scour code for bugs, security issues, and edge cases",
            include_str!("bundled_prompts/hunter.txt"),
            vec![
                ToolName::Read.as_str(),
                ToolName::Bash.as_str(),
                ToolName::Grep.as_str(),
                ToolName::Glob.as_str(),
            ],
        );
        s.gated_by = Some(Feature::ReviewArtifact);
        skills.push(s);
    }

    // /claude-in-chrome — TS: skills/bundled/claudeInChrome.ts
    // (gated by `shouldAutoEnableClaudeInChrome()` runtime check)
    {
        let mut s = bundled(
            "claude-in-chrome",
            "Automates your Chrome browser to interact with web pages — clicking elements, filling forms, capturing screenshots, reading console logs, and navigating sites. Opens pages in new tabs within your existing Chrome session. Requires site-level permissions before executing (configured in the extension).",
            include_str!("bundled_prompts/claude_in_chrome.txt"),
            // Browser MCP tools are dynamic; allowed_tools entries must be `mcp__claude-in-chrome__*`
            // populated at startup by the Chrome MCP integration. Empty here is a placeholder.
            vec![],
        );
        s.when_to_use = Some(
            "When the user wants to interact with web pages, automate browser tasks, capture screenshots, read console logs, or perform any browser-based actions. Always invoke BEFORE attempting to use any mcp__claude-in-chrome__* tools.".to_string(),
        );
        s.gated_by = Some(Feature::ClaudeInChrome);
        skills.push(s);
    }

    // /run-skill-generator — TS: skills/bundled/runSkillGenerator.ts (gated RUN_SKILL_GENERATOR)
    {
        let mut s = bundled(
            "run-skill-generator",
            "Create or refine a SKILL.md file for a custom workflow",
            include_str!("bundled_prompts/run_skill_generator.txt"),
            vec![
                ToolName::Read.as_str(),
                ToolName::Write.as_str(),
                ToolName::Edit.as_str(),
                ToolName::Glob.as_str(),
                ToolName::Grep.as_str(),
                ToolName::AskUserQuestion.as_str(),
            ],
        );
        s.gated_by = Some(Feature::RunSkillGenerator);
        skills.push(s);
    }

    skills
}

/// Register all bundled skills into a SkillManager (filtered by user type +
/// feature gates at the visibility layer).
pub fn register_bundled(manager: &crate::SkillManager, user_type: UserType) {
    for skill in get_bundled_skills(user_type) {
        manager.register(skill);
    }
}

/// Convenience for tests that want default (Human user, Stable features).
pub fn register_bundled_default(manager: &crate::SkillManager) {
    register_bundled(manager, UserType::Human);
}

#[cfg(test)]
#[path = "bundled.test.rs"]
mod tests;
