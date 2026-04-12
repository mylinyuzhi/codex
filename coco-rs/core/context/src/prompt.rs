//! System prompt building.
//!
//! TS: systemPromptType.ts — assembles the system prompt from sections.

use serde::Deserialize;
use serde::Serialize;

/// A compiled system prompt with cache breakpoints.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SystemPrompt {
    pub blocks: Vec<SystemPromptBlock>,
}

/// A block within the system prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SystemPromptBlock {
    Text { content: String },
    CacheBreakpoint,
}

impl SystemPrompt {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a text section.
    pub fn add_text(&mut self, text: impl Into<String>) {
        self.blocks.push(SystemPromptBlock::Text {
            content: text.into(),
        });
    }

    /// Add a cache breakpoint.
    pub fn add_cache_breakpoint(&mut self) {
        self.blocks.push(SystemPromptBlock::CacheBreakpoint);
    }

    /// Get the full prompt text (without cache breakpoints).
    pub fn full_text(&self) -> String {
        self.blocks
            .iter()
            .filter_map(|b| match b {
                SystemPromptBlock::Text { content } => Some(content.as_str()),
                SystemPromptBlock::CacheBreakpoint => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Estimated token count (rough: 1 token ≈ 4 chars).
    pub fn estimated_tokens(&self) -> i64 {
        (self.full_text().len() as i64) / 4
    }
}

/// Build a complete system prompt from all context sources.
///
/// TS: buildEffectiveSystemPrompt() — assembles identity, CLAUDE.md,
/// environment info, tool policies, and injections.
pub fn build_system_prompt(
    identity: &str,
    claude_md_files: &[crate::ClaudeMdFile],
    environment: &crate::EnvironmentInfo,
    skill_listing: Option<&str>,
    custom_append: Option<&str>,
) -> SystemPrompt {
    let mut prompt = SystemPrompt::new();

    // Identity block (who the assistant is)
    prompt.add_text(identity);
    prompt.add_cache_breakpoint();

    // CLAUDE.md files (project instructions)
    if !claude_md_files.is_empty() {
        let mut claude_section = String::from("# Project Instructions\n\n");
        for file in claude_md_files {
            claude_section.push_str(&format!("## {}\n{}\n\n", file.path.display(), file.content));
        }
        prompt.add_text(claude_section);
        prompt.add_cache_breakpoint();
    }

    // Environment info
    let mut env_section = String::from("# Environment\n");
    env_section.push_str(&format!("- Platform: {:?}\n", environment.platform));
    env_section.push_str(&format!("- Shell: {:?}\n", environment.shell));
    env_section.push_str(&format!("- Working directory: {}\n", environment.cwd));
    if environment.is_git_repo {
        if let Some(ref git) = environment.git_status {
            env_section.push_str(&format!("- Git branch: {}\n", git.branch));
        }
    }
    prompt.add_text(env_section);

    // Skill listing (available /commands)
    if let Some(skills) = skill_listing {
        if !skills.is_empty() {
            prompt.add_text(format!("\n# Available Skills\n{skills}"));
        }
    }

    // Custom append (user-specified extra instructions)
    if let Some(append) = custom_append {
        if !append.is_empty() {
            prompt.add_text(format!("\n{append}"));
        }
    }

    prompt
}

/// Build a minimal system prompt (for testing or non-interactive mode).
pub fn build_minimal_prompt(cwd: &std::path::Path) -> SystemPrompt {
    let env = crate::get_environment_info(cwd, "");
    let claude_files = crate::discover_claude_md_files(cwd);

    build_system_prompt(
        "You are an AI coding assistant. Be concise and helpful.",
        &claude_files,
        &env,
        None,
        None,
    )
}
