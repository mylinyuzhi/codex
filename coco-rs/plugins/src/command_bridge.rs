//! Bridge between plugin contributions and the command system.
//!
//! Loads plugin command definitions from:
//! 1. The `commands/` directory (`.md` files and `SKILL.md` subdirs)
//! 2. The V2 manifest `commands` field (path string, array, or object mapping)
//!
//! Produces `PluginCommand` structs containing all data needed to register
//! commands into a `CommandRegistry`. The actual `RegisteredCommand` construction
//! happens in the consuming layer (`coco-commands` or app) to avoid a circular
//! dependency (`coco-commands` already depends on `coco-plugins`).
//!
//! TS: utils/plugins/loadPluginCommands.ts

use std::path::Path;

use coco_skills::load_skill_from_file;
use coco_types::CommandBase;
use coco_types::CommandSafety;
use coco_types::CommandSource;
use coco_types::CommandType;
use coco_types::PromptCommandData;

use crate::LoadedPlugin;
use crate::loader::LoadedPluginV2;
use crate::schemas::CommandMetadata;
use crate::schemas::ManifestCommands;

/// A command loaded from a plugin, ready to be registered.
///
/// Contains all data needed to construct a `RegisteredCommand` in the
/// consuming layer without depending on `coco-commands`.
#[derive(Debug, Clone)]
pub struct PluginCommand {
    pub base: CommandBase,
    pub command_type: CommandType,
    /// The skill prompt text that the command handler should return.
    pub prompt: String,
}

// ---------------------------------------------------------------------------
// V1 loading
// ---------------------------------------------------------------------------

/// Load commands contributed by a V1 plugin.
///
/// Scans the `commands/` directory for `.md` files and `SKILL.md` subdirs.
pub fn load_plugin_commands(plugin: &LoadedPlugin) -> Vec<PluginCommand> {
    let mut commands = Vec::new();
    let commands_dir = plugin.path.join("commands");
    if commands_dir.is_dir() {
        load_commands_from_dir(&commands_dir, &plugin.name, &mut commands);
    }
    commands
}

/// Load commands from all enabled V1 plugins.
pub fn load_all_plugin_commands(plugins: &[&LoadedPlugin]) -> Vec<PluginCommand> {
    plugins
        .iter()
        .flat_map(|p| load_plugin_commands(p))
        .collect()
}

// ---------------------------------------------------------------------------
// V2 loading
// ---------------------------------------------------------------------------

/// Load commands contributed by a V2 plugin.
///
/// Sources:
/// 1. `commands/` directory (same as V1)
/// 2. Manifest `commands` field: string path, array of paths, or object mapping
pub fn load_plugin_commands_v2(plugin: &LoadedPluginV2) -> Vec<PluginCommand> {
    let plugin_name = &plugin.id.name;
    let mut commands = Vec::new();

    // 1. Scan commands/ directory
    let commands_dir = plugin.path.join("commands");
    if commands_dir.is_dir() {
        load_commands_from_dir(&commands_dir, plugin_name, &mut commands);
    }

    // 2. Load from manifest commands field
    if let Some(ref manifest_cmds) = plugin.manifest.commands {
        load_from_manifest_commands(manifest_cmds, &plugin.path, plugin_name, &mut commands);
    }

    commands
}

/// Load commands from all enabled V2 plugins.
pub fn load_all_plugin_commands_v2(plugins: &[&LoadedPluginV2]) -> Vec<PluginCommand> {
    plugins
        .iter()
        .flat_map(|p| load_plugin_commands_v2(p))
        .collect()
}

// ---------------------------------------------------------------------------
// Internal: directory scanning
// ---------------------------------------------------------------------------

/// Scan a directory for `.md` files and `SKILL.md` subdirs, producing commands.
fn load_commands_from_dir(dir: &Path, plugin_name: &str, out: &mut Vec<PluginCommand>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            // Check for SKILL.md inside subdirectory
            let skill_md = path.join("SKILL.md");
            if skill_md.is_file() {
                let dir_name = path.file_name().unwrap_or_default().to_string_lossy();
                load_command_from_file(&skill_md, plugin_name, &dir_name, out);
            }
        } else if path.extension().is_some_and(|ext| ext == "md") && path.is_file() {
            let name = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            load_command_from_file(&path, plugin_name, &name, out);
        }
    }
}

/// Load a single command from a markdown file using the skill parser.
fn load_command_from_file(
    path: &Path,
    plugin_name: &str,
    command_name: &str,
    out: &mut Vec<PluginCommand>,
) {
    match load_skill_from_file(path) {
        Ok(skill) => {
            let namespaced = format!("{plugin_name}:{command_name}");
            out.push(build_plugin_command(
                &namespaced,
                &skill.description,
                &skill.prompt,
                skill.argument_hint.as_deref(),
                skill.model.as_deref(),
                skill.allowed_tools.as_deref(),
            ));
        }
        Err(e) => {
            tracing::warn!(
                plugin = %plugin_name,
                path = %path.display(),
                "failed to load plugin command: {e}",
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Internal: manifest parsing
// ---------------------------------------------------------------------------

/// Dispatch on the `ManifestCommands` enum variants.
fn load_from_manifest_commands(
    manifest_cmds: &ManifestCommands,
    plugin_path: &Path,
    plugin_name: &str,
    out: &mut Vec<PluginCommand>,
) {
    match manifest_cmds {
        ManifestCommands::SinglePath(path_str) => {
            load_manifest_path(path_str, plugin_path, plugin_name, out);
        }
        ManifestCommands::MultiplePaths(paths) => {
            for path_str in paths {
                load_manifest_path(path_str, plugin_path, plugin_name, out);
            }
        }
        ManifestCommands::ObjectMapping(map) => {
            for (cmd_name, meta) in map {
                load_manifest_object_entry(cmd_name, meta, plugin_path, plugin_name, out);
            }
        }
    }
}

/// Load commands from a single manifest path (file or directory).
fn load_manifest_path(
    path_str: &str,
    plugin_path: &Path,
    plugin_name: &str,
    out: &mut Vec<PluginCommand>,
) {
    let resolved = plugin_path.join(path_str);

    if resolved.is_dir() {
        load_commands_from_dir(&resolved, plugin_name, out);
    } else if resolved.is_file() {
        let name = resolved
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        load_command_from_file(&resolved, plugin_name, &name, out);
    } else {
        tracing::debug!(
            plugin = %plugin_name,
            path = %resolved.display(),
            "manifest command path does not exist, skipping",
        );
    }
}

/// Load a command from an object-mapping entry in the manifest.
///
/// Handles `CommandMetadata` with either a `source` file path or inline `content`.
fn load_manifest_object_entry(
    cmd_name: &str,
    meta: &CommandMetadata,
    plugin_path: &Path,
    plugin_name: &str,
    out: &mut Vec<PluginCommand>,
) {
    let namespaced = format!("{plugin_name}:{cmd_name}");

    // Try source file first, then inline content
    if let Some(ref source_path) = meta.source {
        let resolved = plugin_path.join(source_path);
        match load_skill_from_file(&resolved) {
            Ok(skill) => {
                out.push(build_plugin_command(
                    &namespaced,
                    meta.description.as_deref().unwrap_or(&skill.description),
                    &skill.prompt,
                    meta.argument_hint
                        .as_deref()
                        .or(skill.argument_hint.as_deref()),
                    meta.model.as_deref().or(skill.model.as_deref()),
                    meta.allowed_tools
                        .as_deref()
                        .or(skill.allowed_tools.as_deref()),
                ));
            }
            Err(e) => {
                tracing::warn!(
                    plugin = %plugin_name,
                    command = %cmd_name,
                    path = %resolved.display(),
                    "failed to load command source file: {e}",
                );
            }
        }
    } else if let Some(ref content) = meta.content {
        out.push(build_plugin_command(
            &namespaced,
            meta.description.as_deref().unwrap_or("Plugin command"),
            content,
            meta.argument_hint.as_deref(),
            meta.model.as_deref(),
            meta.allowed_tools.as_deref(),
        ));
    } else {
        tracing::warn!(
            plugin = %plugin_name,
            command = %cmd_name,
            "command metadata has neither source nor content, skipping",
        );
    }
}

// ---------------------------------------------------------------------------
// Internal: builder
// ---------------------------------------------------------------------------

/// Build a `PluginCommand` from parsed data.
fn build_plugin_command(
    namespaced_name: &str,
    description: &str,
    prompt: &str,
    argument_hint: Option<&str>,
    model: Option<&str>,
    allowed_tools: Option<&[String]>,
) -> PluginCommand {
    PluginCommand {
        base: CommandBase {
            name: namespaced_name.to_string(),
            description: description.to_string(),
            aliases: vec![],
            availability: vec![],
            is_hidden: false,
            argument_hint: argument_hint.map(ToString::to_string),
            when_to_use: None,
            user_invocable: true,
            is_sensitive: false,
            loaded_from: Some(CommandSource::Plugin),
            safety: CommandSafety::default(),
            supports_non_interactive: false,
        },
        command_type: CommandType::Prompt(PromptCommandData {
            progress_message: format!("Running {namespaced_name}..."),
            content_length: prompt.len() as i64,
            allowed_tools: allowed_tools.map(<[String]>::to_vec),
            model: model.map(ToString::to_string),
            context: Default::default(),
            agent: None,
            thinking_level: None,
            hooks: None,
        }),
        prompt: prompt.to_string(),
    }
}

#[cfg(test)]
#[path = "command_bridge.test.rs"]
mod tests;
