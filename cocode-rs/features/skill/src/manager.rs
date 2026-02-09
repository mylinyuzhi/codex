//! Skill manager for loading and executing skills.
//!
//! The [`SkillManager`] provides a convenient interface for:
//! - Loading bundled skills
//! - Loading skills from configured directories
//! - Looking up skills by name or alias
//! - Filtering skills by invocability and visibility
//! - Executing skill commands by injecting prompts

use crate::bundled::bundled_skills;
use crate::command::SkillContext;
use crate::command::SkillPromptCommand;
use crate::dedup::dedup_skills;
use crate::loader::load_all_skills;
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
    /// Filters out skills that have `disable_model_invocation` set, builtin
    /// skills, and skills without a description or `when_to_use` hint.
    pub fn llm_invocable_skills(&self) -> Vec<&SkillPromptCommand> {
        self.skills
            .values()
            .filter(|s| {
                !s.disable_model_invocation
                    && s.source != SkillSource::Builtin
                    && (!s.description.is_empty() || s.when_to_use.is_some())
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
    let args = parts.next().unwrap_or("").trim();

    Some((name, args))
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_skill(name: &str, prompt: &str) -> SkillPromptCommand {
        SkillPromptCommand {
            name: name.to_string(),
            description: format!("{name} description"),
            prompt: prompt.to_string(),
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
        }
    }

    #[test]
    fn test_parse_skill_command() {
        assert_eq!(parse_skill_command("/commit"), Some(("commit", "")));
        assert_eq!(
            parse_skill_command("/review file.rs"),
            Some(("review", "file.rs"))
        );
        assert_eq!(
            parse_skill_command("/test arg1 arg2"),
            Some(("test", "arg1 arg2"))
        );
        assert_eq!(parse_skill_command("not a command"), None);
        assert_eq!(parse_skill_command(""), None);
    }

    #[test]
    fn test_manager_register_and_get() {
        let mut manager = SkillManager::new();
        manager.register(make_skill("commit", "Generate commit message"));

        assert!(manager.has("commit"));
        assert!(!manager.has("review"));

        let skill = manager.get("commit").unwrap();
        assert_eq!(skill.name, "commit");
    }

    #[test]
    fn test_manager_names() {
        let mut manager = SkillManager::new();
        manager.register(make_skill("beta", "Beta"));
        manager.register(make_skill("alpha", "Alpha"));

        let names = manager.names();
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    fn test_execute_skill() {
        let mut manager = SkillManager::new();
        manager.register(make_skill("commit", "Generate a commit message"));

        let result = execute_skill(&manager, "/commit").unwrap();
        assert_eq!(result.skill_name, "commit");
        assert_eq!(result.prompt, "Generate a commit message");
        assert_eq!(result.args, "");

        // With arguments
        let result = execute_skill(&manager, "/commit --amend").unwrap();
        assert!(result.prompt.contains("--amend"));
        assert_eq!(result.args, "--amend");
    }

    #[test]
    fn test_execute_skill_not_found() {
        let manager = SkillManager::new();
        assert!(execute_skill(&manager, "/nonexistent").is_none());
    }

    #[test]
    fn test_execute_skill_with_arguments_placeholder() {
        let mut manager = SkillManager::new();
        let mut skill = make_skill("review", "Review PR #$ARGUMENTS");
        skill.prompt = "Review PR #$ARGUMENTS".to_string();
        manager.register(skill);

        // With placeholder and args
        let result = execute_skill(&manager, "/review 123").unwrap();
        assert_eq!(result.prompt, "Review PR #123");

        // With placeholder but no args (placeholder becomes empty)
        let result = execute_skill(&manager, "/review").unwrap();
        assert_eq!(result.prompt, "Review PR #");
    }

    #[test]
    fn test_with_bundled() {
        let manager = SkillManager::with_bundled();

        // Should have output-style skill
        assert!(manager.has("output-style"));
        let skill = manager.get("output-style").unwrap();
        assert!(skill.prompt.contains("/output-style"));
    }

    #[test]
    fn test_register_bundled_does_not_override_user_skills() {
        let mut manager = SkillManager::new();

        // Register a user skill with the same name as a bundled skill
        manager.register(make_skill("output-style", "User's custom output-style"));

        // Now register bundled skills
        manager.register_bundled();

        // User skill should still be there, not overridden
        let skill = manager.get("output-style").unwrap();
        assert_eq!(skill.prompt, "User's custom output-style");
    }

    #[test]
    fn test_find_by_name_or_alias() {
        let mut manager = SkillManager::new();
        let mut skill = make_skill("commit", "Generate commit message");
        skill.aliases = vec!["ci".to_string(), "cm".to_string()];
        manager.register(skill);

        // By name
        assert!(manager.find_by_name_or_alias("commit").is_some());
        // By alias
        assert!(manager.find_by_name_or_alias("ci").is_some());
        assert!(manager.find_by_name_or_alias("cm").is_some());
        // Not found
        assert!(manager.find_by_name_or_alias("nonexistent").is_none());
    }

    #[test]
    fn test_execute_skill_by_alias() {
        let mut manager = SkillManager::new();
        let mut skill = make_skill("commit", "Generate commit message");
        skill.aliases = vec!["ci".to_string()];
        manager.register(skill);

        let result = execute_skill(&manager, "/ci").unwrap();
        assert_eq!(result.skill_name, "commit");
    }

    #[test]
    fn test_execute_skill_not_user_invocable() {
        let mut manager = SkillManager::new();
        let mut skill = make_skill("internal", "Internal skill");
        skill.user_invocable = false;
        manager.register(skill);

        // Should return None for non-user-invocable skills
        assert!(execute_skill(&manager, "/internal").is_none());
    }

    #[test]
    fn test_llm_invocable_skills() {
        let mut manager = SkillManager::new();

        // Normal skill - should be included
        manager.register(make_skill("commit", "Generate commit"));

        // Disabled model invocation - should be excluded
        let mut disabled = make_skill("internal", "Internal");
        disabled.disable_model_invocation = true;
        manager.register(disabled);

        // Builtin skill - should be excluded
        let mut builtin = make_skill("builtin", "Builtin");
        builtin.source = SkillSource::Builtin;
        manager.register(builtin);

        let invocable = manager.llm_invocable_skills();
        assert_eq!(invocable.len(), 1);
        assert_eq!(invocable[0].name, "commit");
    }

    #[test]
    fn test_user_visible_skills() {
        let mut manager = SkillManager::new();

        // Normal skill - should be visible
        manager.register(make_skill("commit", "Generate commit"));

        // Hidden skill - should not be visible
        let mut hidden = make_skill("hidden", "Hidden");
        hidden.is_hidden = true;
        manager.register(hidden);

        // Builtin skill - should not be visible
        let mut builtin = make_skill("builtin", "Builtin");
        builtin.source = SkillSource::Builtin;
        manager.register(builtin);

        let visible = manager.user_visible_skills();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].name, "commit");
    }

    #[test]
    fn test_execute_skill_with_base_dir() {
        let mut manager = SkillManager::new();
        let mut skill = make_skill("deploy", "Deploy the app");
        skill.base_dir = Some(PathBuf::from("/project/skills/deploy"));
        manager.register(skill);

        let result = execute_skill(&manager, "/deploy").unwrap();
        assert!(
            result
                .prompt
                .contains("Base directory for this skill: /project/skills/deploy")
        );
        assert!(result.prompt.contains("Deploy the app"));
    }

    #[test]
    fn test_execution_result_fields() {
        let mut manager = SkillManager::new();
        let mut skill = make_skill("deploy", "Deploy");
        skill.model = Some("sonnet".to_string());
        skill.context = SkillContext::Fork;
        skill.agent = Some("deploy-agent".to_string());
        skill.base_dir = Some(PathBuf::from("/skills/deploy"));
        manager.register(skill);

        let result = execute_skill(&manager, "/deploy").unwrap();
        assert_eq!(result.model, Some("sonnet".to_string()));
        assert_eq!(result.context, SkillContext::Fork);
        assert_eq!(result.agent, Some("deploy-agent".to_string()));
        assert_eq!(result.base_dir, Some(PathBuf::from("/skills/deploy")));
    }
}
