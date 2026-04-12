//! Bundled skill definitions shipped with the binary.
//!
//! TS: skills/bundledSkills.ts (220 LOC) + skills/bundled/

use coco_types::ToolName;

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
        description: description.to_string(),
        prompt: prompt.to_string(),
        source: SkillSource::Bundled,
        aliases: vec![],
        allowed_tools: Some(allowed_tools.into_iter().map(String::from).collect()),
        model: None,
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
        is_hidden: false,
    }
}

/// Get all bundled skill definitions.
///
/// TS: `registerBundledSkill()` calls in `skills/bundled/index.ts`.
pub fn get_bundled_skills() -> Vec<SkillDefinition> {
    vec![
        bundled(
            "commit",
            "Create a git commit with a well-crafted message",
            include_str!("bundled_prompts/commit.txt"),
            vec![
                ToolName::Bash.as_str(),
                ToolName::Read.as_str(),
                ToolName::Glob.as_str(),
                ToolName::Grep.as_str(),
            ],
        ),
        bundled(
            "review-pr",
            "Review a pull request for code quality and correctness",
            include_str!("bundled_prompts/review_pr.txt"),
            vec![
                ToolName::Bash.as_str(),
                ToolName::Read.as_str(),
                ToolName::Glob.as_str(),
                ToolName::Grep.as_str(),
            ],
        ),
        bundled(
            "pdf",
            "Read and analyze a PDF file",
            "Read the specified PDF file and provide a summary of its contents.",
            vec![ToolName::Read.as_str(), ToolName::Bash.as_str()],
        ),
        bundled(
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
        ),
        bundled(
            "verify",
            "Run verification checks on recent changes",
            include_str!("bundled_prompts/verify.txt"),
            vec![
                ToolName::Bash.as_str(),
                ToolName::Read.as_str(),
                ToolName::Glob.as_str(),
                ToolName::Grep.as_str(),
            ],
        ),
        {
            let mut skill = bundled(
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
            skill.when_to_use =
                Some("For hooks, permissions, env vars, or settings changes".to_string());
            skill
        },
        {
            let mut skill = bundled(
                "keybindings-help",
                "Customize keyboard shortcuts and keybindings",
                include_str!("bundled_prompts/keybindings_help.txt"),
                vec![
                    ToolName::Read.as_str(),
                    ToolName::Edit.as_str(),
                    ToolName::Write.as_str(),
                ],
            );
            skill.when_to_use = Some(
                "When the user wants to customize keyboard shortcuts, rebind keys, add chord bindings, or modify keybindings.json".to_string(),
            );
            // TS: userInvocable: false — only model invokes this
            skill.user_invocable = false;
            skill
        },
        {
            let mut skill = bundled(
                "remember",
                "Save information to memory for future conversations",
                include_str!("bundled_prompts/remember.txt"),
                vec![
                    ToolName::Read.as_str(),
                    ToolName::Write.as_str(),
                    ToolName::Glob.as_str(),
                ],
            );
            skill.when_to_use = Some("Use when reviewing or organizing memory entries".to_string());
            skill
        },
        bundled(
            "stuck",
            "Help when stuck in loops or debugging dead ends",
            include_str!("bundled_prompts/stuck.txt"),
            vec![
                ToolName::Read.as_str(),
                ToolName::Glob.as_str(),
                ToolName::Grep.as_str(),
                ToolName::Bash.as_str(),
            ],
        ),
        {
            let mut skill = bundled(
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
            skill.when_to_use =
                Some("When user wants sweeping mechanical changes across many files".to_string());
            // TS: disableModelInvocation: true — requires explicit user invocation
            skill.disable_model_invocation = true;
            skill
        },
        bundled(
            "loop",
            "Run a prompt or slash command on a recurring interval",
            include_str!("bundled_prompts/loop.txt"),
            vec![
                ToolName::Bash.as_str(),
                ToolName::Read.as_str(),
                ToolName::Glob.as_str(),
                ToolName::Grep.as_str(),
            ],
        ),
        {
            let mut skill = bundled(
                "debug",
                "Debug tools and inspect internal state",
                include_str!("bundled_prompts/debug.txt"),
                vec![
                    ToolName::Bash.as_str(),
                    ToolName::Read.as_str(),
                    ToolName::Glob.as_str(),
                    ToolName::Grep.as_str(),
                ],
            );
            // TS: disableModelInvocation: true — requires explicit user invocation
            skill.disable_model_invocation = true;
            skill
        },
        {
            let mut skill = bundled(
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
            skill.when_to_use =
                Some("When user wants to automate a repeatable workflow".to_string());
            skill
        },
        bundled(
            "lorem-ipsum",
            "Generate placeholder text for testing",
            "Generate lorem ipsum placeholder text. If the user specifies a length or format, follow their instructions. Otherwise generate a few paragraphs of standard lorem ipsum text.",
            vec![],
        ),
        // TS: claudeApi.ts — Claude API/Anthropic SDK guide
        {
            let mut skill = bundled(
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
            skill.when_to_use = Some(
                "When code imports anthropic SDK or user asks about Claude API, Anthropic SDKs, or Agent SDK".to_string(),
            );
            skill
        },
        // TS: scheduleRemoteAgents.ts — remote agent scheduling
        {
            let mut skill = bundled(
                "schedule",
                "Create, update, list, or run scheduled remote agents (triggers) that execute on a cron schedule",
                include_str!("bundled_prompts/schedule.txt"),
                vec![
                    ToolName::RemoteTrigger.as_str(),
                    ToolName::AskUserQuestion.as_str(),
                ],
            );
            skill.when_to_use = Some(
                "When user wants to schedule recurring remote agents or manage scheduled triggers"
                    .to_string(),
            );
            skill
        },
    ]
}

/// Register all bundled skills into a SkillManager.
pub fn register_bundled(manager: &mut crate::SkillManager) {
    for skill in get_bundled_skills() {
        manager.register(skill);
    }
}

#[cfg(test)]
#[path = "bundled.test.rs"]
mod tests;
