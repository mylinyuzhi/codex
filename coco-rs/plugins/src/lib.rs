//! Plugin system via PLUGIN.toml manifests.
//!
//! TS: plugins/ + services/plugins/ (PluginManifest, PluginManager, contributions)

pub mod command_bridge;
pub mod hook_bridge;
pub mod hot_reload;
pub mod loader;
pub mod marketplace;
pub mod schemas;
pub mod skill_bridge;

use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

/// Plugin manifest — loaded from PLUGIN.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    pub description: String,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub hooks: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub mcp_servers: HashMap<String, serde_json::Value>,
}

/// A loaded plugin with its source and state.
#[derive(Debug, Clone)]
pub struct LoadedPlugin {
    pub name: String,
    pub manifest: PluginManifest,
    pub path: PathBuf,
    pub source: PluginSource,
    pub enabled: bool,
}

/// Where a plugin was loaded from.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginSource {
    Builtin,
    User,
    Project,
    Repository { url: String },
}

/// Load a plugin manifest from a PLUGIN.toml file.
pub fn load_plugin_manifest(path: &Path) -> anyhow::Result<PluginManifest> {
    let content = std::fs::read_to_string(path)?;
    let manifest: PluginManifest = toml::from_str(&content)?;
    Ok(manifest)
}

/// Discover plugins by scanning directories for PLUGIN.toml files.
/// Each directory in `dirs` is checked for a PLUGIN.toml at its root.
pub fn discover_plugins(dirs: &[PathBuf]) -> Vec<LoadedPlugin> {
    dirs.iter()
        .filter_map(|dir| {
            let manifest_path = dir.join("PLUGIN.toml");
            let manifest = load_plugin_manifest(&manifest_path).ok()?;
            Some(LoadedPlugin {
                name: manifest.name.clone(),
                manifest,
                path: dir.clone(),
                source: PluginSource::Project,
                enabled: true,
            })
        })
        .collect()
}

/// Plugin manager — loading, lifecycle, contributions.
#[derive(Default)]
pub struct PluginManager {
    plugins: HashMap<String, LoadedPlugin>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, plugin: LoadedPlugin) {
        self.plugins.insert(plugin.name.clone(), plugin);
    }

    pub fn get(&self, name: &str) -> Option<&LoadedPlugin> {
        self.plugins.get(name)
    }

    pub fn enabled(&self) -> Vec<&LoadedPlugin> {
        self.plugins.values().filter(|p| p.enabled).collect()
    }

    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    /// Enable a plugin by name. Returns false if the plugin was not found.
    pub fn enable(&mut self, name: &str) -> bool {
        if let Some(plugin) = self.plugins.get_mut(name) {
            plugin.enabled = true;
            true
        } else {
            false
        }
    }

    /// Disable a plugin by name. Returns false if the plugin was not found.
    pub fn disable(&mut self, name: &str) -> bool {
        if let Some(plugin) = self.plugins.get_mut(name) {
            plugin.enabled = false;
            true
        } else {
            false
        }
    }

    /// Discover and register plugins from the given directories.
    /// Each directory is scanned for a PLUGIN.toml at its root.
    pub fn load_from_dirs(&mut self, dirs: &[PathBuf]) {
        for plugin in discover_plugins(dirs) {
            self.register(plugin);
        }
    }
}

/// Plugin contributions — what a plugin provides to the system.
///
/// TS: PluginContributions in utils/plugins/
#[derive(Debug, Clone, Default)]
pub struct PluginContributions {
    /// Skill definitions contributed by this plugin.
    pub skills: Vec<String>,
    /// Hook definitions contributed.
    pub hooks: Vec<serde_json::Value>,
    /// MCP server configs contributed.
    pub mcp_servers: HashMap<String, serde_json::Value>,
    /// Agent definitions contributed.
    pub agents: Vec<String>,
    /// Slash commands contributed.
    pub commands: Vec<String>,
}

impl LoadedPlugin {
    /// Collect all contributions from this plugin.
    pub fn contributions(&self) -> PluginContributions {
        let mut contributions = PluginContributions::default();

        // Skills from manifest
        for skill in &self.manifest.skills {
            contributions.skills.push(skill.clone());
        }

        // Hooks from manifest
        for hook_value in self.manifest.hooks.values() {
            contributions.hooks.push(hook_value.clone());
        }

        // MCP servers from manifest
        contributions.mcp_servers = self.manifest.mcp_servers.clone();

        // Discover additional contributions from directory structure
        self.discover_dir_contributions(&mut contributions);

        contributions
    }

    /// Discover contributions from the plugin's directory structure.
    fn discover_dir_contributions(&self, contributions: &mut PluginContributions) {
        // Check for skills/ directory
        let skills_dir = self.path.join("skills");
        if skills_dir.is_dir()
            && let Ok(entries) = std::fs::read_dir(&skills_dir)
        {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "md")
                    && let Some(name) = path.file_stem().and_then(|s| s.to_str())
                {
                    contributions.skills.push(name.to_string());
                }
            }
        }

        // Check for agents/ directory
        let agents_dir = self.path.join("agents");
        if agents_dir.is_dir()
            && let Ok(entries) = std::fs::read_dir(&agents_dir)
        {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "md")
                    && let Some(name) = path.file_stem().and_then(|s| s.to_str())
                {
                    contributions.agents.push(name.to_string());
                }
            }
        }

        // Check for commands/ directory
        let commands_dir = self.path.join("commands");
        if commands_dir.is_dir()
            && let Ok(entries) = std::fs::read_dir(&commands_dir)
        {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "md")
                    && let Some(name) = path.file_stem().and_then(|s| s.to_str())
                {
                    contributions.commands.push(name.to_string());
                }
            }
        }
    }
}

/// Standard plugin directories.
pub fn get_plugin_dirs(config_dir: &Path, project_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // User-level plugins: ~/.coco/plugins/
    let user_plugins = config_dir.join("plugins");
    if user_plugins.is_dir()
        && let Ok(entries) = std::fs::read_dir(&user_plugins)
    {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                dirs.push(entry.path());
            }
        }
    }

    // Project-level plugins: .claude/plugins/
    let project_plugins = project_dir.join(".claude").join("plugins");
    if project_plugins.is_dir()
        && let Ok(entries) = std::fs::read_dir(&project_plugins)
    {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                dirs.push(entry.path());
            }
        }
    }

    dirs
}

/// Collect all contributions from all enabled plugins.
pub fn collect_all_contributions(manager: &PluginManager) -> PluginContributions {
    let mut total = PluginContributions::default();

    for plugin in manager.enabled() {
        let c = plugin.contributions();
        total.skills.extend(c.skills);
        total.hooks.extend(c.hooks);
        total.mcp_servers.extend(c.mcp_servers);
        total.agents.extend(c.agents);
        total.commands.extend(c.commands);
    }

    total
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
