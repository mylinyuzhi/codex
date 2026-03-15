//! Skill manager for loading and executing skills.
//!
//! The [`SkillManager`] provides a convenient interface for:
//! - Loading bundled skills
//! - Loading skills from configured directories
//! - Looking up skills by name or alias
//! - Filtering skills by invocability and visibility
//! - Executing skill commands by injecting prompts

use crate::bundled::bundled_skills;
use crate::command::CommandType;
use crate::command::SkillContext;
use crate::command::SkillPromptCommand;
use crate::command::SlashCommand;
use crate::dedup::dedup_skills;
use crate::interface::SkillInterface;
use crate::loader::load_all_skills;
use crate::local::builtin_local_commands;
use crate::local::find_local_command;
use crate::outcome::SkillLoadOutcome;
use crate::source::LoadedFrom;
use crate::source::SkillSource;

use std::collections::HashMap;
use std::path::PathBuf;
use tracing::debug;
use tracing::info;
use tracing::warn;

/// Result of loading skills from directories.
#[derive(Debug, Default)]
pub struct SkillLoadResult {
    /// Number of skills successfully loaded.
    pub loaded: i32,
    /// Number of skills that failed to load.
    pub failed: i32,
    /// Paths of skills that failed to load (for debugging).
    pub failures: Vec<PathBuf>,
}

impl SkillLoadResult {
    /// Check if all skills loaded successfully.
    pub fn is_complete(&self) -> bool {
        self.failed == 0
    }
}

/// Manages loaded skills and provides lookup/execution functionality.
///
/// The manager loads skills from configured directories and provides
/// efficient lookup by name or alias. Skills are deduplicated by name,
/// with later-loaded skills taking precedence.
#[derive(Default)]
pub struct SkillManager {
    /// Loaded skills indexed by name.
    skills: HashMap<String, SkillPromptCommand>,
}

impl SkillManager {
    /// Create a new empty skill manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new skill manager with bundled skills pre-loaded.
    ///
    /// Bundled skills are compiled into the binary and provide essential
    /// system commands like `/output-style`.
    pub fn with_bundled() -> Self {
        let mut manager = Self::new();
        manager.register_bundled();
        manager
    }

    /// Register all bundled skills.
    ///
    /// Bundled skills have lowest priority and will be overridden by
    /// user-defined skills with the same name.
    pub fn register_bundled(&mut self) {
        for bundled in bundled_skills() {
            debug!(
                name = %bundled.name,
                fingerprint = %bundled.fingerprint,
                "Registering bundled skill"
            );
            // Only register if not already present (user skills take precedence)
            if !self.skills.contains_key(&bundled.name) {
                self.skills.insert(
                    bundled.name.clone(),
                    SkillPromptCommand {
                        name: bundled.name,
                        description: bundled.description,
                        prompt: bundled.prompt,
                        allowed_tools: None,
                        user_invocable: true,
                        disable_model_invocation: false,
                        is_hidden: false,
                        source: SkillSource::Bundled,
                        loaded_from: LoadedFrom::Bundled,
                        context: SkillContext::Main,
                        agent: None,
                        model: None,
                        base_dir: None,
                        when_to_use: None,
                        argument_hint: None,
                        aliases: Vec::new(),
                        interface: None,
                        command_type: bundled.command_type,
                    },
                );
            }
        }
    }

    /// Load skills from the given root directories.
    ///
    /// Skills are deduplicated by name, with later roots taking precedence.
    /// Returns a [`SkillLoadResult`] with counts and any failures.
    pub fn load_from_roots(&mut self, roots: &[PathBuf]) -> SkillLoadResult {
        let outcomes = load_all_skills(roots);
        let total = outcomes.len();

        // Collect failures for reporting
        let mut failures = Vec::new();
        for outcome in &outcomes {
            if let SkillLoadOutcome::Failed { path, error } = outcome {
                warn!(
                    path = %path.display(),
                    error = %error,
                    "Failed to load skill"
                );
                failures.push(path.clone());
            }
        }

        let success_count = outcomes.iter().filter(|o| o.is_success()).count();

        // Deduplicate by name (keeps first occurrence)
        let deduped = dedup_skills(outcomes);

        // Index by name (only successful loads)
        for outcome in deduped {
            if let SkillLoadOutcome::Success { skill, source } = outcome {
                debug!(
                    name = %skill.name,
                    source = ?source,
                    "Loaded skill"
                );
                self.skills.insert(skill.name.clone(), skill);
            }
        }

        info!(
            total = total,
            success = success_count,
            failed = failures.len(),
            deduped = self.skills.len(),
            "Skill loading complete"
        );

        SkillLoadResult {
            loaded: self.skills.len() as i32,
            failed: failures.len() as i32,
            failures,
        }
    }

    /// Register a single skill.
    ///
    /// If a skill with the same name already exists, it is replaced.
    pub fn register(&mut self, skill: SkillPromptCommand) {
        self.skills.insert(skill.name.clone(), skill);
    }

    /// Look up a skill by name.
    pub fn get(&self, name: &str) -> Option<&SkillPromptCommand> {
        self.skills.get(name)
    }

    /// Look up a skill by name or alias.
    ///
    /// First checks for an exact name match, then searches aliases.
    pub fn find_by_name_or_alias(&self, name: &str) -> Option<&SkillPromptCommand> {
        // Direct name lookup first
        if let Some(skill) = self.skills.get(name) {
            return Some(skill);
        }

        // Search aliases
        self.skills
            .values()
            .find(|skill| skill.aliases.iter().any(|alias| alias == name))
    }

    /// Check if a skill exists.
    pub fn has(&self, name: &str) -> bool {
        self.skills.contains_key(name)
    }

    /// Get all skill names.
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<_> = self.skills.keys().map(String::as_str).collect();
        names.sort();
        names
    }

    /// Get all skills.
    pub fn all(&self) -> impl Iterator<Item = &SkillPromptCommand> {
        self.skills.values()
    }

    /// Get skills that can be invoked by the LLM via the Skill tool.
    ///
    /// Two-layer filter aligned with Claude Code's `getLLMInvocableSkills()`:
    /// 1. **Type filter**: only `CommandType::Prompt` skills pass (excludes `LocalJsx`)
    /// 2. **Field filter**: `!disable_model_invocation`, not `Builtin` source,
    ///    and at least one of: bundled, has description, or has `when_to_use`
    pub fn llm_invocable_skills(&self) -> Vec<&SkillPromptCommand> {
        self.skills
            .values()
            .filter(|s| {
                // Layer 1: type filter (aligned with `c.type !== 'prompt'`)
                s.command_type == CommandType::Prompt
                // Layer 2: field filter (aligned with getLLMInvocableSkills)
                    && !s.disable_model_invocation
                    && s.source != SkillSource::Builtin
                    && (s.loaded_from == LoadedFrom::Bundled
                        || !s.description.is_empty()
                        || s.when_to_use.is_some())
            })
            .collect()
    }

    /// Get skills that should be visible to users in help and command lists.
    ///
    /// Filters out hidden skills and builtin skills.
    pub fn user_visible_skills(&self) -> Vec<&SkillPromptCommand> {
        self.skills
            .values()
            .filter(|s| !s.is_hidden && s.source != SkillSource::Builtin)
            .collect()
    }

    /// Get the number of loaded skills.
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    /// Check if the manager has no skills.
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// Clear all loaded skills.
    pub fn clear(&mut self) {
        self.skills.clear();
    }

    /// Get all commands (both prompt skills and local commands) as a unified list.
    ///
    /// Returns local commands first, then user-visible prompt skills.
    pub fn all_commands(&self) -> Vec<SlashCommand> {
        let mut commands: Vec<SlashCommand> = builtin_local_commands()
            .iter()
            .map(super::local::LocalCommandDef::to_slash_command)
            .collect();

        for skill in self.user_visible_skills() {
            commands.push(SlashCommand {
                name: skill.name.clone(),
                description: skill.description.clone(),
                command_type: skill.command_type,
            });
        }

        commands
    }

    /// Find a command by name across all types (local + prompt skills).
    ///
    /// Checks local commands first (by name and alias), then prompt skills.
    pub fn find_command(&self, name: &str) -> Option<SlashCommand> {
        // Check local commands first
        if let Some(cmd) = find_local_command(name) {
            return Some(cmd.to_slash_command());
        }

        // Then check prompt skills
        if let Some(skill) = self.find_by_name_or_alias(name)
            && skill.is_user_invocable()
        {
            return Some(SlashCommand {
                name: skill.name.clone(),
                description: skill.description.clone(),
                command_type: skill.command_type,
            });
        }

        None
    }

    /// Check if a command exists (local or prompt skill).
    pub fn has_command(&self, name: &str) -> bool {
        find_local_command(name).is_some() || self.has(name)
    }

    /// Check if a command name refers to a local (built-in) command.
    pub fn is_local_command(&self, name: &str) -> bool {
        find_local_command(name).is_some()
    }
}

/// Result of executing a skill command.
#[derive(Debug, Clone)]
pub struct SkillExecutionResult {
    /// The skill that was executed.
    pub skill_name: String,

    /// The prompt text to inject.
    pub prompt: String,

    /// Optional tools the skill is allowed to use.
    pub allowed_tools: Option<Vec<String>>,

    /// Arguments passed to the skill (from the command line).
    pub args: String,

    /// Model override for this skill.
    pub model: Option<String>,

    /// Execution context.
    pub context: SkillContext,

    /// Agent type for fork context.
    pub agent: Option<String>,

    /// Base directory of the skill.
    pub base_dir: Option<PathBuf>,

    /// The skill interface (SKILL.md frontmatter) for hook registration.
    ///
    /// Callers can use this to register skill-scoped hooks via
    /// [`register_skill_hooks`](crate::register_skill_hooks).
    pub interface: Option<SkillInterface>,
}

/// Parse a skill command from user input.
///
/// Returns the skill name and any arguments.
///
/// # Examples
///
/// ```
/// use cocode_skill::manager::parse_skill_command;
///
/// let (name, args) = parse_skill_command("/commit").unwrap();
/// assert_eq!(name, "commit");
/// assert_eq!(args, "");
///
/// let (name, args) = parse_skill_command("/review src/main.rs").unwrap();
/// assert_eq!(name, "review");
/// assert_eq!(args, "src/main.rs");
/// ```
pub fn parse_skill_command(input: &str) -> Option<(&str, &str)> {
    let input = input.trim();
    if !input.starts_with('/') {
        return None;
    }

    let without_slash = &input[1..];
    let mut parts = without_slash.splitn(2, char::is_whitespace);

    let name = parts.next()?;
    if !is_valid_command_name(name) {
        return None;
    }
    let args = parts.next().unwrap_or("").trim();

    Some((name, args))
}

/// Check if a command name contains only safe characters.
///
/// Valid names consist of ASCII alphanumeric characters, hyphens, underscores, and colons.
fn is_valid_command_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':')
}

/// Execute a skill command.
///
/// Parses the input, looks up the skill (by name or alias), and returns
/// the execution result containing the prompt to inject.
///
/// Returns `None` if the skill is not found or if the skill is not
/// user-invocable.
///
/// # Arguments
///
/// * `manager` - The skill manager to look up skills from
/// * `input` - The user input (e.g., "/commit" or "/review file.rs")
///
/// # Returns
///
/// Returns `Some(SkillExecutionResult)` if the skill was found and is
/// user-invocable, `None` otherwise.
pub fn execute_skill(manager: &SkillManager, input: &str) -> Option<SkillExecutionResult> {
    let (name, args) = parse_skill_command(input)?;

    let skill = manager.find_by_name_or_alias(name)?;

    // Check user_invocable flag
    if !skill.is_user_invocable() {
        return None;
    }

    // Build the prompt, potentially incorporating arguments
    // If prompt contains $ARGUMENTS placeholder, replace it; otherwise append args
    let mut prompt = if skill.prompt.contains("$ARGUMENTS") {
        skill.prompt.replace("$ARGUMENTS", args)
    } else if args.is_empty() {
        skill.prompt.clone()
    } else {
        // Append arguments to the prompt (fallback)
        format!("{}\n\nArguments: {}", skill.prompt, args)
    };

    // Inject base directory prefix if available
    if let Some(ref base_dir) = skill.base_dir {
        prompt = format!(
            "Base directory for this skill: {}\n\n{prompt}",
            base_dir.display()
        );
    }

    Some(SkillExecutionResult {
        skill_name: skill.name.clone(),
        prompt,
        allowed_tools: skill.allowed_tools.clone(),
        args: args.to_string(),
        model: skill.model.clone(),
        context: skill.context,
        agent: skill.agent.clone(),
        base_dir: skill.base_dir.clone(),
        interface: skill.interface.clone(),
    })
}

#[cfg(test)]
#[path = "manager.test.rs"]
mod tests;
