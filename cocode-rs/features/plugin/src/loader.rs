//! Plugin discovery and loading.
//!
//! Scans directories for plugins (containing plugin.json) and loads their
//! contributions.

use crate::agent_loader::load_agents_from_dir;
use crate::command_loader::load_commands_from_dir;
use crate::contribution::PluginContribution;
use crate::contribution::PluginContributions;
use crate::error::Result;
use crate::error::plugin_error::InvalidManifestSnafu;
use crate::error::plugin_error::IoSnafu;
use crate::error::plugin_error::PathTraversalSnafu;
use crate::lsp_loader::load_lsp_servers_from_dir;
use crate::lsp_loader::load_lsp_servers_from_file;
use crate::manifest::PLUGIN_DIR;
use crate::manifest::PLUGIN_JSON;
use crate::manifest::PluginManifest;
use crate::manifest::PluginRootSettings;
use crate::mcp_loader::load_mcp_servers_from_dir;
use crate::scope::PluginScope;

use crate::plugin_settings::PluginSettings;

use cocode_skill::SkillLoadOutcome;
use cocode_skill::load_skills_from_dir;
use std::path::Path;
use std::path::PathBuf;
use tracing::debug;
use tracing::info;
use tracing::warn;
use walkdir::WalkDir;

/// Maximum depth to scan for plugins.
const MAX_SCAN_DEPTH: i32 = 3;

/// Maximum output style file size (1 MB).
const MAX_OUTPUT_STYLE_SIZE: u64 = 1_048_576;

/// A loaded plugin with its manifest and contributions.
#[derive(Debug, Clone)]
pub struct LoadedPlugin {
    /// Plugin manifest.
    pub manifest: PluginManifest,

    /// Plugin directory.
    pub path: PathBuf,

    /// Scope the plugin was loaded from.
    pub scope: PluginScope,

    /// Loaded contributions.
    pub contributions: Vec<PluginContribution>,

    /// Plugin root settings (from `settings.json`).
    pub settings: PluginRootSettings,
}

impl LoadedPlugin {
    /// Get the plugin name.
    pub fn name(&self) -> &str {
        &self.manifest.plugin.name
    }

    /// Get the plugin version.
    pub fn version(&self) -> &str {
        &self.manifest.plugin.version
    }
}

/// Plugin loader that discovers and loads plugins from directories.
pub struct PluginLoader {
    /// Maximum depth to scan.
    max_depth: i32,
}

impl Default for PluginLoader {
    fn default() -> Self {
        Self {
            max_depth: MAX_SCAN_DEPTH,
        }
    }
}

impl PluginLoader {
    /// Create a new plugin loader.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the maximum scan depth.
    pub fn with_max_depth(mut self, depth: i32) -> Self {
        self.max_depth = depth;
        self
    }

    /// Scan a directory for plugins.
    ///
    /// Returns a list of paths to plugin directories (containing plugin.json).
    pub fn scan(&self, root: &Path) -> Vec<PathBuf> {
        if !root.is_dir() {
            return Vec::new();
        }

        let mut results = Vec::new();
        let depth = self.max_depth.max(0) as usize;

        // Note: Symlinks are not followed to prevent potential security issues
        // with symlink attacks and to ensure plugins stay within their boundaries.
        let walker = WalkDir::new(root)
            .max_depth(depth)
            .follow_links(false)
            .into_iter();

        for entry in walker.filter_map(std::result::Result::ok) {
            if entry.file_type().is_dir() {
                // Skip .cocode-plugin/ directories — they are metadata dirs, not plugin roots
                if entry.file_name().to_str() == Some(PLUGIN_DIR) {
                    continue;
                }
                // Check .cocode-plugin/plugin.json first, then plugin.json at root
                let hidden_path = entry.path().join(PLUGIN_DIR).join(PLUGIN_JSON);
                let root_path = entry.path().join(PLUGIN_JSON);
                if hidden_path.is_file() || root_path.is_file() {
                    results.push(entry.path().to_path_buf());
                }
            }
        }

        results
    }

    /// Load a single plugin from its directory.
    pub fn load(
        &self,
        dir: &Path,
        scope: PluginScope,
        settings: &PluginSettings,
    ) -> Result<LoadedPlugin> {
        debug!(path = %dir.display(), scope = %scope, "Loading plugin");

        // Load manifest
        let manifest = PluginManifest::from_dir(dir)?;

        // Validate manifest
        if let Err(errors) = manifest.validate() {
            return Err(InvalidManifestSnafu {
                path: dir.to_path_buf(),
                message: errors.join("; "),
            }
            .build());
        }

        // Look up per-plugin user config for variable resolution (e.g. MCP servers)
        let user_config = settings.get_plugin_config(&manifest.plugin.name);

        // Load contributions
        let contributions = self.load_contributions(
            dir,
            &manifest.contributions,
            &manifest.plugin.name,
            user_config,
        )?;

        // Load plugin root settings
        let settings = PluginRootSettings::from_dir(dir);

        info!(
            name = %manifest.plugin.name,
            version = %manifest.plugin.version,
            scope = %scope,
            contributions = contributions.len(),
            agent = ?settings.agent,
            "Loaded plugin"
        );

        Ok(LoadedPlugin {
            manifest,
            path: dir.to_path_buf(),
            scope,
            contributions,
            settings,
        })
    }

    /// Validate that a path stays within the plugin directory.
    ///
    /// Returns the canonical path if valid, or an error for path traversal.
    fn validate_path(&self, plugin_dir: &Path, relative_path: &str) -> Result<PathBuf> {
        let full_path = plugin_dir.join(relative_path);

        // Canonicalize both paths to resolve symlinks and ..
        let canonical_plugin = match plugin_dir.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                return Err(IoSnafu {
                    path: plugin_dir.to_path_buf(),
                    message: e.to_string(),
                }
                .build());
            }
        };

        // The target path may not exist yet, so canonicalize the parent
        let canonical_full = if full_path.exists() {
            full_path.canonicalize().map_err(|e| {
                IoSnafu {
                    path: full_path.clone(),
                    message: e.to_string(),
                }
                .build()
            })?
        } else {
            // Normalize the path to resolve .. components for traversal detection,
            // even when the target doesn't exist and can't be canonicalized.
            normalize_path(&full_path)
        };

        // Check that the canonical path is within the plugin directory
        if !canonical_full.starts_with(&canonical_plugin) {
            return Err(PathTraversalSnafu {
                path: PathBuf::from(relative_path),
            }
            .build());
        }

        Ok(canonical_full)
    }

    /// Load contributions from a plugin.
    ///
    /// Loads declared contributions first, then auto-discovers standard
    /// directories if the corresponding contribution list is empty.
    fn load_contributions(
        &self,
        plugin_dir: &Path,
        contributions: &PluginContributions,
        plugin_name: &str,
        user_config: Option<&std::collections::HashMap<String, serde_json::Value>>,
    ) -> Result<Vec<PluginContribution>> {
        let mut result = Vec::new();

        // Load skills (declared or auto-discover skills/)
        let fallback = self.auto_discover_dir(plugin_dir, "skills");
        for full_path in self.resolve_paths(
            plugin_dir,
            &contributions.skills,
            fallback,
            plugin_name,
            "skill",
        ) {
            if full_path.is_dir() {
                for outcome in load_skills_from_dir(&full_path) {
                    match outcome {
                        SkillLoadOutcome::Success { skill, .. } => {
                            result.push(PluginContribution::Skill {
                                skill: *skill,
                                plugin_name: plugin_name.to_string(),
                            });
                        }
                        SkillLoadOutcome::Failed { path, error } => {
                            warn!(
                                plugin = %plugin_name,
                                path = %path.display(),
                                error = %error,
                                "Failed to load skill from plugin"
                            );
                        }
                    }
                }
            }
        }

        // Load hooks (declared or auto-discover hooks/hooks.json)
        let fallback = self.auto_discover_file(plugin_dir, "hooks/hooks.json");
        for full_path in self.resolve_paths(
            plugin_dir,
            &contributions.hooks,
            fallback,
            plugin_name,
            "hook",
        ) {
            if full_path.is_file() {
                match self.load_hooks_from_file(&full_path, plugin_name) {
                    Ok(hooks) => result.extend(hooks),
                    Err(e) => {
                        warn!(
                            plugin = %plugin_name,
                            path = %full_path.display(),
                            error = %e,
                            "Failed to load hooks from plugin"
                        );
                    }
                }
            }
        }

        // Load agents (declared or auto-discover agents/)
        let fallback = self.auto_discover_dir(plugin_dir, "agents");
        for full_path in self.resolve_paths(
            plugin_dir,
            &contributions.agents,
            fallback,
            plugin_name,
            "agent",
        ) {
            if full_path.is_dir() {
                result.extend(load_agents_from_dir(&full_path, plugin_name));
            }
        }

        // Load commands (declared or auto-discover commands/)
        let fallback = self.auto_discover_dir(plugin_dir, "commands");
        for full_path in self.resolve_paths(
            plugin_dir,
            &contributions.commands,
            fallback,
            plugin_name,
            "command",
        ) {
            if full_path.is_dir() {
                result.extend(load_commands_from_dir(&full_path, plugin_name));
            }
        }

        // Load MCP servers (declared or auto-discover mcp/ and .mcp.json)
        let fallback = {
            let mut paths = self.auto_discover_dir(plugin_dir, "mcp");
            let mcp_json = plugin_dir.join(".mcp.json");
            if mcp_json.is_file() {
                paths.push(".mcp.json".to_string());
            }
            paths
        };
        for full_path in self.resolve_paths(
            plugin_dir,
            &contributions.mcp_servers,
            fallback,
            plugin_name,
            "MCP server",
        ) {
            if full_path.is_dir() {
                result.extend(load_mcp_servers_from_dir(
                    &full_path,
                    plugin_name,
                    user_config,
                ));
            }
        }

        // Load LSP servers (declared or auto-discover .lsp.json)
        let fallback = self.auto_discover_file(plugin_dir, ".lsp.json");
        for full_path in self.resolve_paths(
            plugin_dir,
            &contributions.lsp_servers,
            fallback,
            plugin_name,
            "LSP server",
        ) {
            if full_path.is_file() {
                result.extend(load_lsp_servers_from_file(&full_path, plugin_name));
            } else if full_path.is_dir() {
                result.extend(load_lsp_servers_from_dir(&full_path, plugin_name));
            }
        }

        // Load output styles (declared or auto-discover outputStyles/)
        let fallback = self.auto_discover_dir(plugin_dir, "outputStyles");
        for full_path in self.resolve_paths(
            plugin_dir,
            &contributions.output_styles,
            fallback,
            plugin_name,
            "output style",
        ) {
            if full_path.is_dir() {
                result.extend(load_output_styles_from_dir(&full_path, plugin_name));
            }
        }

        Ok(result)
    }

    /// Auto-discover a standard directory if it exists in the plugin dir.
    fn auto_discover_dir(&self, plugin_dir: &Path, dir_name: &str) -> Vec<String> {
        let path = plugin_dir.join(dir_name);
        if path.is_dir() {
            debug!(dir = dir_name, "Auto-discovered standard directory");
            vec![format!("{dir_name}/")]
        } else {
            Vec::new()
        }
    }

    /// Auto-discover a standard file if it exists in the plugin dir.
    fn auto_discover_file(&self, plugin_dir: &Path, file_name: &str) -> Vec<String> {
        let path = plugin_dir.join(file_name);
        if path.is_file() {
            debug!(file = file_name, "Auto-discovered standard file");
            vec![file_name.to_string()]
        } else {
            Vec::new()
        }
    }

    /// Resolve declared or auto-discovered paths, validating each against the
    /// plugin directory boundary.
    fn resolve_paths(
        &self,
        plugin_dir: &Path,
        declared: &crate::contribution::StringOrVec,
        fallback: Vec<String>,
        plugin_name: &str,
        contribution_type: &str,
    ) -> Vec<PathBuf> {
        let relative_paths: Vec<String> = if declared.is_empty() {
            fallback
        } else {
            declared.iter().cloned().collect()
        };

        relative_paths
            .iter()
            .filter_map(|rel| match self.validate_path(plugin_dir, rel) {
                Ok(p) => Some(p),
                Err(e) => {
                    warn!(
                        plugin = %plugin_name,
                        path = %rel,
                        error = %e,
                        "Invalid {contribution_type} path in plugin"
                    );
                    None
                }
            })
            .collect()
    }

    /// Load hooks from a JSON file.
    fn load_hooks_from_file(
        &self,
        path: &Path,
        plugin_name: &str,
    ) -> Result<Vec<PluginContribution>> {
        // load_hooks_from_json takes a path and handles reading internally
        match cocode_hooks::load_hooks_from_json(path) {
            Ok(definitions) => Ok(definitions
                .into_iter()
                .map(|hook| PluginContribution::Hook {
                    hook,
                    plugin_name: plugin_name.to_string(),
                })
                .collect()),
            Err(e) => Err(InvalidManifestSnafu {
                path: path.to_path_buf(),
                message: format!("Failed to parse hooks: {e}"),
            }
            .build()),
        }
    }
}

/// Load plugins from multiple root directories.
///
/// Scans each root for plugins and loads them. Returns all successfully
/// loaded plugins.
pub fn load_plugins_from_roots(
    roots: &[(PathBuf, PluginScope)],
    settings: &PluginSettings,
) -> Vec<LoadedPlugin> {
    let loader = PluginLoader::new();
    let mut plugins = Vec::new();

    for (root, scope) in roots {
        if !root.is_dir() {
            debug!(
                root = %root.display(),
                scope = %scope,
                "Plugin root does not exist or is not a directory"
            );
            continue;
        }

        let plugin_dirs = loader.scan(root);
        debug!(
            root = %root.display(),
            scope = %scope,
            count = plugin_dirs.len(),
            "Scanned for plugins"
        );

        for dir in plugin_dirs {
            match loader.load(&dir, *scope, settings) {
                Ok(plugin) => plugins.push(plugin),
                Err(e) => {
                    warn!(
                        path = %dir.display(),
                        scope = %scope,
                        error = %e,
                        "Failed to load plugin"
                    );
                }
            }
        }
    }

    info!(total = plugins.len(), "Plugin loading complete");

    plugins
}

/// Normalize a path by resolving `.` and `..` components without filesystem access.
fn normalize_path(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            c => result.push(c),
        }
    }
    result
}

/// Load output styles from a directory.
///
/// Each `.md` file in the directory is treated as an output style definition.
/// The filename (without extension) becomes the style name.
fn load_output_styles_from_dir(dir: &Path, plugin_name: &str) -> Vec<PluginContribution> {
    use crate::contribution::OutputStyleDefinition;

    let mut results = Vec::new();

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            warn!(
                plugin = %plugin_name,
                dir = %dir.display(),
                error = %e,
                "Failed to read output styles directory"
            );
            return results;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            // Check file size before reading
            if let Ok(meta) = entry.metadata()
                && meta.len() > MAX_OUTPUT_STYLE_SIZE
            {
                warn!(
                    plugin = %plugin_name,
                    path = %path.display(),
                    size = meta.len(),
                    "Output style file exceeds size limit, skipping"
                );
                continue;
            }
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();
            match std::fs::read_to_string(&path) {
                Ok(prompt) => {
                    debug!(
                        plugin = %plugin_name,
                        style = %name,
                        "Loaded output style"
                    );
                    results.push(PluginContribution::OutputStyle {
                        style: OutputStyleDefinition { name, prompt },
                        plugin_name: plugin_name.to_string(),
                    });
                }
                Err(e) => {
                    warn!(
                        plugin = %plugin_name,
                        path = %path.display(),
                        error = %e,
                        "Failed to read output style file"
                    );
                }
            }
        }
    }

    results
}

#[cfg(test)]
#[path = "loader.test.rs"]
mod tests;
