//! Bridge between plugin contributions and the skill system.
//!
//! Converts plugin skill paths into `SkillDefinition` instances with
//! proper namespacing (plugin:skill-name) and source attribution.
//!
//! TS: utils/plugins/loadPluginCommands.ts — loads skills from plugin
//! directories and namespaces them as `plugin:command-name`.

use std::path::Path;

use coco_skills::SkillDefinition;
use coco_skills::SkillSource;
use coco_skills::load_skill_from_file;

use crate::LoadedPlugin;

/// Load all skill definitions contributed by a plugin.
///
/// Skills are loaded from:
/// 1. Paths listed in the plugin manifest's `skills` field
/// 2. `.md` files in the plugin's `skills/` directory
///
/// Each skill is namespaced as `plugin-name:skill-name` (TS convention).
///
/// TS: `loadPluginCommands()` + `getPluginSkills()`.
pub fn load_plugin_skills(plugin: &LoadedPlugin) -> Vec<SkillDefinition> {
    let mut skills = Vec::new();
    let plugin_name = &plugin.name;

    // 1. Skills from manifest paths
    for skill_path_str in &plugin.manifest.skills {
        let skill_path = plugin.path.join(skill_path_str);
        load_skill_at_path(&skill_path, plugin_name, &mut skills);
    }

    // 2. Skills from skills/ directory
    let skills_dir = plugin.path.join("skills");
    if skills_dir.is_dir() {
        load_skills_from_dir(&skills_dir, plugin_name, &mut skills);
    }

    skills
}

/// Load all skills from all enabled plugins in a manager.
///
/// Returns namespaced skills ready to register in a SkillManager.
pub fn load_all_plugin_skills(plugins: &[&LoadedPlugin]) -> Vec<SkillDefinition> {
    plugins.iter().flat_map(|p| load_plugin_skills(p)).collect()
}

/// Load a single skill from a file path, namespacing it to the plugin.
fn load_skill_at_path(path: &Path, plugin_name: &str, skills: &mut Vec<SkillDefinition>) {
    if !path.exists() {
        tracing::debug!(
            plugin = %plugin_name,
            path = %path.display(),
            "skill path does not exist, skipping"
        );
        return;
    }

    // Handle directory format (skill-name/SKILL.md)
    if path.is_dir() {
        let skill_md = path.join("SKILL.md");
        if skill_md.is_file()
            && let Ok(mut skill) = load_skill_from_file(&skill_md)
        {
            let dir_name = path.file_name().unwrap_or_default().to_string_lossy();
            namespace_skill(&mut skill, plugin_name, &dir_name);
            skills.push(skill);
        }
        return;
    }

    // Handle flat .md file
    if path.extension().is_some_and(|ext| ext == "md") && path.is_file() {
        match load_skill_from_file(path) {
            Ok(mut skill) => {
                let original_name = skill.name.clone();
                namespace_skill(&mut skill, plugin_name, &original_name);
                skills.push(skill);
            }
            Err(e) => {
                tracing::warn!(
                    plugin = %plugin_name,
                    path = %path.display(),
                    "failed to load plugin skill: {e}"
                );
            }
        }
    }
}

/// Load skills from a directory, scanning for .md files and SKILL.md subdirs.
fn load_skills_from_dir(dir: &Path, plugin_name: &str, skills: &mut Vec<SkillDefinition>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            // Check for SKILL.md inside subdirectory
            let skill_md = path.join("SKILL.md");
            if skill_md.is_file()
                && let Ok(mut skill) = load_skill_from_file(&skill_md)
            {
                let dir_name = path.file_name().unwrap_or_default().to_string_lossy();
                namespace_skill(&mut skill, plugin_name, &dir_name);
                skills.push(skill);
            }
        } else if path.extension().is_some_and(|ext| ext == "md") && path.is_file() {
            match load_skill_from_file(&path) {
                Ok(mut skill) => {
                    let original_name = skill.name.clone();
                    namespace_skill(&mut skill, plugin_name, &original_name);
                    skills.push(skill);
                }
                Err(e) => {
                    tracing::warn!(
                        plugin = %plugin_name,
                        path = %path.display(),
                        "failed to load plugin skill: {e}"
                    );
                }
            }
        }
    }
}

/// Apply plugin namespacing to a skill definition.
///
/// TS: plugin skills are named `plugin-name:skill-name` or
/// `plugin-name:namespace:skill-name` for nested directories.
fn namespace_skill(skill: &mut SkillDefinition, plugin_name: &str, skill_name: &str) {
    skill.name = format!("{plugin_name}:{skill_name}");
    skill.source = SkillSource::Plugin {
        plugin_name: plugin_name.to_string(),
    };
}

#[cfg(test)]
#[path = "skill_bridge.test.rs"]
mod tests;
