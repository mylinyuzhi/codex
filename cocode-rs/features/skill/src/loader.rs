//! Skill loading from directories.
//!
//! Reads `SKILL.md` files, parses YAML frontmatter and markdown body
//! (which serves as the prompt), validates the result, and produces
//! [`SkillLoadOutcome`] values.

use crate::command::CommandType;
use crate::command::SkillContext;
use crate::command::SkillPromptCommand;
use crate::frontmatter;
use crate::interface::SkillInterface;
use crate::outcome::SkillLoadOutcome;
use crate::scanner::SkillScanner;
use crate::source::LoadedFrom;
use crate::source::SkillSource;
use crate::validator;

use std::fs;
use std::path::Path;
use std::path::PathBuf;

/// The expected skill file name.
const SKILL_MD: &str = "SKILL.md";

/// Loads all skills from a single directory.
///
/// The directory itself is expected to be a skills root (e.g.,
/// `.cocode/skills/`). Each immediate subdirectory (or nested directory
/// found by the scanner) that contains `SKILL.md` is treated as a
/// skill directory.
///
/// Returns one [`SkillLoadOutcome`] per discovered skill directory.
/// Failed skills produce [`SkillLoadOutcome::Failed`] but do not
/// prevent other skills from loading.
pub fn load_skills_from_dir(dir: &Path) -> Vec<SkillLoadOutcome> {
    let scanner = SkillScanner::new();
    let skill_dirs = scanner.scan(dir);

    skill_dirs
        .into_iter()
        .map(|skill_dir| load_single_skill(&skill_dir, dir))
        .collect()
}

/// Loads skills from multiple root directories.
///
/// Scans each root for skill directories and loads them. All results
/// are concatenated.
pub fn load_all_skills(roots: &[PathBuf]) -> Vec<SkillLoadOutcome> {
    let mut outcomes = Vec::new();
    for root in roots {
        if root.is_dir() {
            let loaded = load_skills_from_dir(root);
            tracing::debug!(
                root = %root.display(),
                loaded = loaded.len(),
                success = loaded.iter().filter(|o| o.is_success()).count(),
                "loaded skills from root"
            );
            outcomes.extend(loaded);
        } else {
            tracing::debug!(
                root = %root.display(),
                "skill root does not exist or is not a directory"
            );
        }
    }
    outcomes
}

/// Loads a single skill from its directory.
fn load_single_skill(skill_dir: &Path, root: &Path) -> SkillLoadOutcome {
    let md_path = skill_dir.join(SKILL_MD);

    // Read SKILL.md
    let content = match fs::read_to_string(&md_path) {
        Ok(content) => content,
        Err(err) => {
            return SkillLoadOutcome::Failed {
                path: skill_dir.to_path_buf(),
                error: format!("failed to read {SKILL_MD}: {err}"),
            };
        }
    };

    // Parse frontmatter
    let (yaml_str, body) = match frontmatter::parse_frontmatter(&content) {
        Ok(result) => result,
        Err(err) => {
            return SkillLoadOutcome::Failed {
                path: skill_dir.to_path_buf(),
                error: format!("failed to parse {SKILL_MD} frontmatter: {err}"),
            };
        }
    };

    // Parse YAML into SkillInterface
    let interface: SkillInterface = match serde_yml::from_str(yaml_str) {
        Ok(iface) => iface,
        Err(err) => {
            return SkillLoadOutcome::Failed {
                path: skill_dir.to_path_buf(),
                error: format!("failed to parse {SKILL_MD} YAML: {err}"),
            };
        }
    };

    // Prompt comes from the markdown body
    let prompt = body.trim().to_string();

    // Validate
    if let Err(errors) = validator::validate_skill(&interface, &prompt) {
        return SkillLoadOutcome::Failed {
            path: skill_dir.to_path_buf(),
            error: format!("validation failed: {}", errors.join("; ")),
        };
    }

    // Determine source based on relationship to root
    let source = determine_source(skill_dir, root);
    let loaded_from = LoadedFrom::from(&source);

    // Map new interface fields
    let user_invocable = interface.user_invocable.unwrap_or(true);
    let disable_model_invocation = interface.disable_model_invocation.unwrap_or(false);
    let is_hidden = !user_invocable;

    let context = match interface.context.as_deref() {
        Some("fork") => SkillContext::Fork,
        _ => SkillContext::Main,
    };

    // Check if skill has hooks
    let has_hooks = interface.hooks.as_ref().is_some_and(|h| !h.is_empty());

    SkillLoadOutcome::Success {
        skill: SkillPromptCommand {
            name: interface.name.clone(),
            description: interface.description.clone(),
            prompt,
            allowed_tools: interface.allowed_tools.clone(),
            user_invocable,
            disable_model_invocation,
            is_hidden,
            source: source.clone(),
            loaded_from,
            context,
            agent: interface.agent.clone(),
            model: interface.model.clone(),
            base_dir: Some(skill_dir.to_path_buf()),
            when_to_use: interface.when_to_use.clone(),
            argument_hint: interface.argument_hint.clone(),
            aliases: interface.aliases.clone().unwrap_or_default(),
            // Only keep interface if it has hooks (to save memory)
            interface: if has_hooks { Some(interface) } else { None },
            command_type: CommandType::Prompt,
        },
        source,
    }
}

/// Determines the [`SkillSource`] based on the skill directory and its root.
fn determine_source(skill_dir: &Path, root: &Path) -> SkillSource {
    // Use the root path as a heuristic:
    // - If root contains ".cocode/skills" it is project-local
    // - If root contains home dir patterns, it is user-global
    // - Otherwise default to project-local
    let root_str = root.to_string_lossy();
    if root_str.contains(".cocode/skills") || root_str.contains(".cocode\\skills") {
        SkillSource::ProjectSettings {
            path: skill_dir.to_path_buf(),
        }
    } else {
        SkillSource::UserSettings {
            path: skill_dir.to_path_buf(),
        }
    }
}

#[cfg(test)]
#[path = "loader.test.rs"]
mod tests;
