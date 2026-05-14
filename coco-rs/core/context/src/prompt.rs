//! System prompt building.
//!
//! TS: `constants/prompts.ts::getSystemPrompt`. coco-rs assembles the
//! prompt as an ordered list of `SystemPromptBlock`s with explicit
//! cache breakpoints; the final cache-prefix mirrors TS's
//! `SYSTEM_PROMPT_DYNAMIC_BOUNDARY` placement (static identity +
//! style + project instructions cached together; environment +
//! memory + custom-append placed after).

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

/// Borrowed view of an active output style for the prompt builder.
///
/// Defined locally so `coco-context` does not depend on
/// `coco-output-styles` (which would break the
/// `core/` → `root/` layering rule). The CLI converts an
/// `OutputStyleConfig` to this view at the boundary.
///
/// TS source: `constants/prompts.ts::getOutputStyleSection` +
/// `getSimpleIntroSection` (the `outputStyleConfig !== null` branch
/// alters intro framing) + the `keepCodingInstructions` gate that
/// suppresses the standard "Doing tasks" block when `false`.
#[derive(Debug, Clone, Copy)]
pub struct OutputStyleSection<'a> {
    /// Display name (e.g., `Explanatory`, `alpha:concise`).
    pub name: &'a str,
    /// Full prompt body.
    pub prompt: &'a str,
    /// When `true`, the standard coding instructions stay on top of
    /// the style. When `false`, the style replaces them. TS only keeps
    /// those instructions for default style or explicit
    /// `keepCodingInstructions: true`.
    pub keep_coding_instructions: bool,
}

/// Build a complete system prompt from all context sources.
///
/// TS: `getSystemPrompt()` — assembles identity, output style,
/// project instructions, environment, skills, memory, and custom
/// append. The `output_style` parameter mirrors the
/// `getOutputStyleSection` block; when present it is injected
/// immediately after the identity block (and before the cache
/// breakpoint) so the cached prefix covers identity + style + project
/// instructions, matching TS's static-prefix layout.
///
/// Note: TS additionally toggles the intro phrasing
/// (`with software engineering tasks` vs `according to your "Output
/// Style" below`) and conditionally emits the "Doing tasks" section
/// based on `keepCodingInstructions`. coco-rs uses a static identity
/// string passed by the caller, so the intro toggle isn't applied
/// here — callers that want full TS parity build the identity string
/// with awareness of the output-style presence (e.g., the binary
/// embedded prompt swap). The `keep_coding_instructions` flag is
/// surfaced on `OutputStyleSection` for future use; it does not
/// short-circuit the current static identity.
pub fn build_system_prompt(
    identity: &str,
    claude_md_files: &[crate::MemoryFile],
    environment: &crate::EnvironmentInfo,
    skill_listing: Option<&str>,
    memory_section: Option<&str>,
    custom_append: Option<&str>,
    output_style: Option<OutputStyleSection<'_>>,
) -> SystemPrompt {
    let mut prompt = SystemPrompt::new();

    // Identity block (who the assistant is)
    prompt.add_text(identity);

    // Output style — placed immediately after identity so the cached
    // static prefix covers it. TS:
    // `getOutputStyleSection(outputStyleConfig)` rendered as
    // `# Output Style: <name>\n<prompt>`.
    if let Some(style) = output_style {
        prompt.add_text(format!(
            "\n# Output Style: {}\n{}",
            style.name, style.prompt
        ));
    }

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
    if environment.is_git_repo
        && let Some(ref git) = environment.git_status
    {
        env_section.push_str(&format!("- Git branch: {}\n", git.branch));
    }
    prompt.add_text(env_section);

    // Skill listing (available /commands)
    if let Some(skills) = skill_listing
        && !skills.is_empty()
    {
        prompt.add_text(format!("\n# Available Skills\n{skills}"));
    }

    // Auto-memory block: type taxonomy + how-to-save + MEMORY.md.
    // Cache-broken so MEMORY.md edits don't invalidate the identity
    // / CLAUDE.md prefix above it.
    if let Some(memory) = memory_section
        && !memory.is_empty()
    {
        prompt.add_cache_breakpoint();
        prompt.add_text(memory);
    }

    // Custom append (user-specified extra instructions)
    if let Some(append) = custom_append
        && !append.is_empty()
    {
        prompt.add_text(format!("\n{append}"));
    }

    prompt
}

/// Build a minimal system prompt (for testing or non-interactive mode).
pub fn build_minimal_prompt(cwd: &std::path::Path) -> SystemPrompt {
    let env = crate::get_environment_info(cwd, "");
    let claude_files = crate::discover_memory_files(cwd);

    build_system_prompt(
        "You are an AI coding assistant. Be concise and helpful.",
        &claude_files,
        &env,
        None,
        None,
        None,
        None,
    )
}

#[cfg(test)]
#[path = "prompt.test.rs"]
mod tests;
