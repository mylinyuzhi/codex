//! Plugin loading pipeline — discovery, validation, dependency resolution.
//!
//! TS: utils/plugins/pluginLoader.ts + dependencyResolver.ts +
//! installedPluginsManager.ts

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use chrono::Utc;

use crate::schemas::InstalledPluginsFileV2;
use crate::schemas::ManifestValidationError;
use crate::schemas::PluginId;
use crate::schemas::PluginInstallationEntry;
use crate::schemas::PluginInstallationRecord;
use crate::schemas::PluginManifestV2;
use crate::schemas::PluginMarketplace;
use crate::schemas::PluginMarketplaceEntry;
use crate::schemas::PluginScope;
use crate::schemas::validate_manifest;

// ---------------------------------------------------------------------------
// Plugin load result
// ---------------------------------------------------------------------------

/// A fully loaded plugin with metadata and resolved path.
#[derive(Debug, Clone)]
pub struct LoadedPluginV2 {
    /// The plugin ID ("name@marketplace").
    pub id: PluginId,
    /// Parsed manifest.
    pub manifest: PluginManifestV2,
    /// On-disk path to the plugin directory.
    pub path: PathBuf,
    /// Where the plugin was loaded from.
    pub load_source: PluginLoadSource,
    /// Whether the plugin is currently enabled.
    pub enabled: bool,
}

/// Source from which a plugin was loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginLoadSource {
    /// Installed from a marketplace.
    Marketplace { marketplace: String },
    /// Session-only plugin from --plugin-dir flag.
    SessionDir,
    /// Built-in plugin.
    Builtin,
}

/// An error encountered while loading a single plugin.
#[derive(Debug, Clone)]
pub struct PluginLoadError {
    /// Plugin identifier (may be partial if manifest failed to parse).
    pub plugin_id: String,
    pub message: String,
}

impl std::fmt::Display for PluginLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.plugin_id, self.message)
    }
}

/// Result of loading all plugins.
#[derive(Debug, Default)]
pub struct PluginLoadResult {
    pub plugins: Vec<LoadedPluginV2>,
    pub errors: Vec<PluginLoadError>,
}

// ---------------------------------------------------------------------------
// Plugin loader
// ---------------------------------------------------------------------------

/// Loads and validates plugins from directories and marketplace entries.
pub struct PluginLoader {
    /// Base directory for plugin cache (e.g. `~/.cocode/plugins/cache/`).
    plugins_dir: PathBuf,
}

impl PluginLoader {
    pub fn new(plugins_dir: PathBuf) -> Self {
        Self { plugins_dir }
    }

    /// Load a single plugin from a directory containing PLUGIN.toml or plugin.json.
    pub fn load_from_dir(
        &self,
        dir: &Path,
        load_source: PluginLoadSource,
        marketplace_name: Option<&str>,
    ) -> Result<LoadedPluginV2, PluginLoadError> {
        let manifest = self.load_manifest(dir)?;

        let validation_errors = validate_manifest(&manifest);
        if !validation_errors.is_empty() {
            return Err(PluginLoadError {
                plugin_id: manifest.name,
                message: format_validation_errors(&validation_errors),
            });
        }

        let marketplace = marketplace_name.unwrap_or("inline");
        let id = PluginId {
            name: manifest.name.clone(),
            marketplace: marketplace.to_string(),
        };

        Ok(LoadedPluginV2 {
            id,
            manifest,
            path: dir.to_path_buf(),
            load_source,
            enabled: true,
        })
    }

    /// Load a plugin manifest (PLUGIN.toml preferred, falls back to plugin.json).
    fn load_manifest(&self, dir: &Path) -> Result<PluginManifestV2, PluginLoadError> {
        let toml_path = dir.join("PLUGIN.toml");
        let json_path = dir.join("plugin.json");

        if toml_path.exists() {
            let content = std::fs::read_to_string(&toml_path).map_err(|e| PluginLoadError {
                plugin_id: dir.display().to_string(),
                message: format!("failed to read PLUGIN.toml: {e}"),
            })?;
            toml::from_str(&content).map_err(|e| PluginLoadError {
                plugin_id: dir.display().to_string(),
                message: format!("invalid PLUGIN.toml: {e}"),
            })
        } else if json_path.exists() {
            let content = std::fs::read_to_string(&json_path).map_err(|e| PluginLoadError {
                plugin_id: dir.display().to_string(),
                message: format!("failed to read plugin.json: {e}"),
            })?;
            serde_json::from_str(&content).map_err(|e| PluginLoadError {
                plugin_id: dir.display().to_string(),
                message: format!("invalid plugin.json: {e}"),
            })
        } else {
            Err(PluginLoadError {
                plugin_id: dir.display().to_string(),
                message: "no PLUGIN.toml or plugin.json found".to_string(),
            })
        }
    }

    /// Load plugins from a marketplace manifest.
    ///
    /// For each entry, resolves the plugin directory from the cache and loads
    /// its manifest, merging any supplementary fields from the marketplace entry.
    pub fn load_from_marketplace(
        &self,
        marketplace: &PluginMarketplace,
        enabled_plugins: &HashSet<String>,
    ) -> PluginLoadResult {
        let mut result = PluginLoadResult::default();

        for entry in &marketplace.plugins {
            let plugin_id_str = format!("{}@{}", entry.name, marketplace.name);
            let enabled = enabled_plugins.contains(&plugin_id_str);

            let cache_dir = self.resolve_cache_path(&marketplace.name, &entry.name);

            if !cache_dir.exists() {
                if enabled {
                    result.errors.push(PluginLoadError {
                        plugin_id: plugin_id_str,
                        message: format!(
                            "plugin cache directory not found: {}",
                            cache_dir.display()
                        ),
                    });
                }
                continue;
            }

            match self.load_and_merge_entry(&cache_dir, marketplace, entry) {
                Ok(mut plugin) => {
                    plugin.enabled = enabled;
                    result.plugins.push(plugin);
                }
                Err(err) => {
                    result.errors.push(err);
                }
            }
        }

        result
    }

    /// Load a plugin from cache and merge marketplace entry metadata.
    fn load_and_merge_entry(
        &self,
        cache_dir: &Path,
        marketplace: &PluginMarketplace,
        entry: &PluginMarketplaceEntry,
    ) -> Result<LoadedPluginV2, PluginLoadError> {
        let manifest_result = self.load_manifest(cache_dir);

        let manifest = match manifest_result {
            Ok(m) => m,
            Err(_) if !entry.strict => {
                // non-strict: marketplace entry provides the manifest
                build_manifest_from_entry(entry)
            }
            Err(e) => return Err(e),
        };

        let id = PluginId {
            name: manifest.name.clone(),
            marketplace: marketplace.name.clone(),
        };

        Ok(LoadedPluginV2 {
            id,
            manifest,
            path: cache_dir.to_path_buf(),
            load_source: PluginLoadSource::Marketplace {
                marketplace: marketplace.name.clone(),
            },
            enabled: false,
        })
    }

    /// Compute the cache directory path for a marketplace plugin.
    fn resolve_cache_path(&self, marketplace: &str, plugin_name: &str) -> PathBuf {
        let sanitized_mkt = sanitize_for_path(marketplace);
        let sanitized_plugin = sanitize_for_path(plugin_name);
        self.plugins_dir
            .join("cache")
            .join(sanitized_mkt)
            .join(sanitized_plugin)
    }

    /// Discover and load session-only plugins from --plugin-dir paths.
    pub fn load_session_plugins(&self, dirs: &[PathBuf]) -> PluginLoadResult {
        let mut result = PluginLoadResult::default();

        for dir in dirs {
            if !dir.is_dir() {
                result.errors.push(PluginLoadError {
                    plugin_id: dir.display().to_string(),
                    message: "session plugin directory does not exist".to_string(),
                });
                continue;
            }
            match self.load_from_dir(dir, PluginLoadSource::SessionDir, None) {
                Ok(plugin) => result.plugins.push(plugin),
                Err(err) => result.errors.push(err),
            }
        }

        result
    }

    /// Create an `InstallationRecord` for a successfully loaded plugin.
    pub fn record_installation(
        plugin: &LoadedPluginV2,
        source_url: Option<String>,
    ) -> PluginInstallationRecord {
        let scope = match &plugin.load_source {
            PluginLoadSource::Marketplace { .. } => PluginScope::User,
            PluginLoadSource::SessionDir => PluginScope::Local,
            PluginLoadSource::Builtin => PluginScope::Managed,
        };
        PluginInstallationRecord {
            name: plugin.id.name.clone(),
            version: plugin
                .manifest
                .version
                .clone()
                .unwrap_or_else(|| "0.0.0".to_string()),
            installed_at: Utc::now().to_rfc3339(),
            source_url,
            scope,
        }
    }

    /// Discover skills, agents, and commands from the plugin directory structure.
    ///
    /// Scans conventional subdirectories (skills/, agents/, commands/) and returns
    /// discovered file stems alongside any paths declared in the manifest.
    pub fn discover_contributions(plugin: &LoadedPluginV2) -> DiscoveredContributions {
        let mut contributions = DiscoveredContributions::default();

        // From manifest paths
        if let Some(ref paths) = plugin.manifest.skills {
            for p in paths.to_vec() {
                contributions.skills.push(p.to_string());
            }
        }
        if let Some(ref paths) = plugin.manifest.agents {
            for p in paths.to_vec() {
                contributions.agents.push(p.to_string());
            }
        }

        // Scan conventional directories
        scan_md_dir(&plugin.path.join("skills"), &mut contributions.skills);
        scan_md_dir(&plugin.path.join("agents"), &mut contributions.agents);
        scan_md_dir(&plugin.path.join("commands"), &mut contributions.commands);

        contributions
    }

    /// Detect duplicate plugin names across all loaded plugins.
    pub fn detect_duplicates(plugins: &[LoadedPluginV2]) -> Vec<PluginLoadError> {
        let mut seen: HashMap<String, &PluginId> = HashMap::new();
        let mut errors = Vec::new();

        for plugin in plugins {
            let id_str = plugin.id.as_str();
            if let Some(existing) = seen.get(&id_str) {
                errors.push(PluginLoadError {
                    plugin_id: id_str.clone(),
                    message: format!("duplicate plugin ID (already loaded as {existing})"),
                });
            } else {
                seen.insert(id_str, &plugin.id);
            }
        }

        errors
    }
}

// ---------------------------------------------------------------------------
// Dependency resolution (TS: dependencyResolver.ts)
// ---------------------------------------------------------------------------

/// Result of dependency resolution.
#[derive(Debug)]
pub enum DependencyResolution {
    /// Resolved successfully; `closure` includes the root + all transitive deps.
    Ok { closure: Vec<String> },
    /// A cycle was detected.
    Cycle { chain: Vec<String> },
    /// A dependency was not found.
    NotFound {
        missing: String,
        required_by: String,
    },
    /// A cross-marketplace dependency was blocked.
    CrossMarketplace {
        dependency: String,
        required_by: String,
    },
}

/// Normalize a bare dependency name to "name@marketplace" if the declaring
/// plugin has a known marketplace. Inline plugins keep bare dep names.
pub fn qualify_dependency(dep: &str, declaring_plugin_id: &str) -> String {
    if dep.contains('@') {
        return dep.to_string();
    }
    if let Some(id) = PluginId::parse(declaring_plugin_id) {
        if id.marketplace == "inline" {
            return dep.to_string();
        }
        return format!("{dep}@{}", id.marketplace);
    }
    dep.to_string()
}

/// Minimal lookup result for dependency resolution.
pub struct DependencyLookupResult {
    pub dependencies: Vec<String>,
}

/// Walk the transitive dependency closure of `root_id` via DFS.
///
/// Security: cross-marketplace deps are blocked unless explicitly in
/// `allowed_cross_marketplaces`.
pub fn resolve_dependency_closure(
    root_id: &str,
    lookup: &dyn Fn(&str) -> Option<DependencyLookupResult>,
    already_enabled: &HashSet<String>,
    allowed_cross_marketplaces: &HashSet<String>,
) -> DependencyResolution {
    let root_marketplace = PluginId::parse(root_id).map(|id| id.marketplace);

    let mut closure = Vec::new();
    let mut visited = HashSet::new();
    let mut stack = Vec::new();

    #[allow(clippy::too_many_arguments)]
    fn walk(
        id: &str,
        required_by: &str,
        root_id: &str,
        root_marketplace: &Option<String>,
        lookup: &dyn Fn(&str) -> Option<DependencyLookupResult>,
        already_enabled: &HashSet<String>,
        allowed_cross_marketplaces: &HashSet<String>,
        closure: &mut Vec<String>,
        visited: &mut HashSet<String>,
        stack: &mut Vec<String>,
    ) -> Option<DependencyResolution> {
        // Skip already-enabled dependencies (but never skip root).
        if id != root_id && already_enabled.contains(id) {
            return None;
        }

        // Cross-marketplace security check.
        if let Some(root_mkt) = root_marketplace
            && let Some(dep_id) = PluginId::parse(id)
            && &dep_id.marketplace != root_mkt
            && !allowed_cross_marketplaces.contains(&dep_id.marketplace)
        {
            return Some(DependencyResolution::CrossMarketplace {
                dependency: id.to_string(),
                required_by: required_by.to_string(),
            });
        }

        if stack.contains(&id.to_string()) {
            let mut chain: Vec<String> = stack.clone();
            chain.push(id.to_string());
            return Some(DependencyResolution::Cycle { chain });
        }
        if visited.contains(id) {
            return None;
        }
        visited.insert(id.to_string());

        let entry = match lookup(id) {
            Some(e) => e,
            None => {
                return Some(DependencyResolution::NotFound {
                    missing: id.to_string(),
                    required_by: required_by.to_string(),
                });
            }
        };

        stack.push(id.to_string());
        for raw_dep in &entry.dependencies {
            let dep = qualify_dependency(raw_dep, id);
            if let Some(err) = walk(
                &dep,
                id,
                root_id,
                root_marketplace,
                lookup,
                already_enabled,
                allowed_cross_marketplaces,
                closure,
                visited,
                stack,
            ) {
                return Some(err);
            }
        }
        stack.pop();

        closure.push(id.to_string());
        None
    }

    if let Some(err) = walk(
        root_id,
        root_id,
        root_id,
        &root_marketplace,
        lookup,
        already_enabled,
        allowed_cross_marketplaces,
        &mut closure,
        &mut visited,
        &mut stack,
    ) {
        return err;
    }

    DependencyResolution::Ok { closure }
}

/// Verify that all enabled plugins have their dependencies satisfied.
/// Returns the set of plugin IDs that should be demoted (disabled) because
/// their deps are missing.
pub fn verify_and_demote(plugins: &[LoadedPluginV2]) -> HashSet<String> {
    let enabled_ids: HashSet<String> = plugins
        .iter()
        .filter(|p| p.enabled)
        .map(|p| p.id.as_str())
        .collect();

    let enabled_names: HashSet<&str> = plugins
        .iter()
        .filter(|p| p.enabled)
        .map(|p| p.id.name.as_str())
        .collect();

    let mut demoted = HashSet::new();
    let mut changed = true;

    // Fixed-point iteration: keep demoting until stable.
    while changed {
        changed = false;
        for plugin in plugins {
            let id_str = plugin.id.as_str();
            if !plugin.enabled || demoted.contains(&id_str) {
                continue;
            }

            if let Some(ref deps) = plugin.manifest.dependencies {
                for dep in deps {
                    let qualified = qualify_dependency(dep, &id_str);
                    // Check both qualified ID and bare name.
                    let satisfied = enabled_ids.contains(&qualified)
                        && !demoted.contains(&qualified)
                        || enabled_names.contains(dep.as_str());

                    if !satisfied {
                        tracing::warn!(
                            plugin = %id_str,
                            missing_dep = %dep,
                            "demoting plugin: unsatisfied dependency"
                        );
                        demoted.insert(id_str.clone());
                        changed = true;
                        break;
                    }
                }
            }
        }
    }

    demoted
}

// ---------------------------------------------------------------------------
// Installation tracking (TS: installedPluginsManager.ts)
// ---------------------------------------------------------------------------

/// Manage the installed_plugins.json file on disk.
pub struct InstalledPluginsManager {
    file_path: PathBuf,
    data: InstalledPluginsFileV2,
}

impl InstalledPluginsManager {
    /// Load from the given path, or create empty V2 if file doesn't exist.
    pub fn load(file_path: PathBuf) -> anyhow::Result<Self> {
        let data = if file_path.exists() {
            let content = std::fs::read_to_string(&file_path)?;
            let raw: serde_json::Value = serde_json::from_str(&content)?;

            let version = raw
                .get("version")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(1) as i32;

            if version == 1 {
                // Migrate V1 -> V2
                migrate_v1_to_v2(&raw)?
            } else {
                serde_json::from_value(raw)?
            }
        } else {
            InstalledPluginsFileV2::default()
        };

        Ok(Self { file_path, data })
    }

    /// Get the V2 data.
    pub fn data(&self) -> &InstalledPluginsFileV2 {
        &self.data
    }

    /// Check if a plugin ID is installed at any scope.
    pub fn is_installed(&self, plugin_id: &str) -> bool {
        self.data
            .plugins
            .get(plugin_id)
            .is_some_and(|entries| !entries.is_empty())
    }

    /// Get all installation entries for a plugin.
    pub fn get_installations(&self, plugin_id: &str) -> &[PluginInstallationEntry] {
        self.data.plugins.get(plugin_id).map_or(&[], Vec::as_slice)
    }

    /// Record a new installation entry.
    pub fn record_installation(&mut self, plugin_id: &str, entry: PluginInstallationEntry) {
        let entries = self.data.plugins.entry(plugin_id.to_string()).or_default();

        // Replace existing entry at the same scope+project, or append.
        if let Some(existing) = entries
            .iter_mut()
            .find(|e| e.scope == entry.scope && e.project_path == entry.project_path)
        {
            *existing = entry;
        } else {
            entries.push(entry);
        }
    }

    /// Remove all installation entries for a plugin.
    pub fn remove_plugin(&mut self, plugin_id: &str) {
        self.data.plugins.remove(plugin_id);
    }

    /// Remove entries at a specific scope for a plugin.
    pub fn remove_at_scope(&mut self, plugin_id: &str, scope: PluginScope) {
        if let Some(entries) = self.data.plugins.get_mut(plugin_id) {
            entries.retain(|e| e.scope != scope);
            if entries.is_empty() {
                self.data.plugins.remove(plugin_id);
            }
        }
    }

    /// Persist the current state to disk.
    pub fn save(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(&self.data)?;
        std::fs::write(&self.file_path, content)?;
        Ok(())
    }

    /// Return all plugin IDs that are installed.
    pub fn installed_plugin_ids(&self) -> Vec<&str> {
        self.data.plugins.keys().map(String::as_str).collect()
    }
}

/// Migrate V1 format to V2 format.
fn migrate_v1_to_v2(raw: &serde_json::Value) -> anyhow::Result<InstalledPluginsFileV2> {
    let plugins_raw = raw
        .get("plugins")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();

    let mut plugins = HashMap::new();

    for (id, v1_entry) in plugins_raw {
        let install_path = v1_entry
            .get("installPath")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();

        let version = v1_entry
            .get("version")
            .and_then(serde_json::Value::as_str)
            .map(String::from);

        let installed_at = v1_entry
            .get("installedAt")
            .and_then(serde_json::Value::as_str)
            .map(String::from);

        let last_updated = v1_entry
            .get("lastUpdated")
            .and_then(serde_json::Value::as_str)
            .map(String::from);

        let git_commit_sha = v1_entry
            .get("gitCommitSha")
            .and_then(serde_json::Value::as_str)
            .map(String::from);

        let entry = PluginInstallationEntry {
            scope: PluginScope::User,
            project_path: None,
            install_path,
            version,
            installed_at,
            last_updated,
            git_commit_sha,
        };

        plugins.insert(id, vec![entry]);
    }

    Ok(InstalledPluginsFileV2 {
        version: 2,
        plugins,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Contributions discovered from a plugin's directory structure and manifest.
#[derive(Debug, Default)]
pub struct DiscoveredContributions {
    pub skills: Vec<String>,
    pub agents: Vec<String>,
    pub commands: Vec<String>,
}

/// Scan a directory for `.md` files and push their stems into `out`.
fn scan_md_dir(dir: &Path, out: &mut Vec<String>) {
    if !dir.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "md")
            && let Some(name) = path.file_stem().and_then(|s| s.to_str())
        {
            out.push(name.to_string());
        }
    }
}

/// Build a minimal manifest from a marketplace entry (non-strict mode).
fn build_manifest_from_entry(entry: &PluginMarketplaceEntry) -> PluginManifestV2 {
    PluginManifestV2 {
        name: entry.name.clone(),
        version: entry.version.clone(),
        description: entry.description.clone(),
        author: entry.author.clone(),
        homepage: entry.homepage.clone(),
        repository: None,
        license: entry.license.clone(),
        keywords: entry.keywords.clone(),
        dependencies: entry.dependencies.clone(),
        skills: None,
        hooks: None,
        agents: None,
        commands: None,
        mcp_servers: None,
        lsp_servers: None,
        output_styles: None,
        channels: None,
        user_config: None,
        settings: None,
        env_vars: None,
        min_version: None,
        max_version: None,
    }
}

/// Sanitize a string for use in a filesystem path segment.
fn sanitize_for_path(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            result.push(c);
        } else {
            result.push('-');
        }
    }
    result
}

fn format_validation_errors(errors: &[ManifestValidationError]) -> String {
    errors
        .iter()
        .map(|e| format!("{}: {}", e.field, e.message))
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
#[path = "loader.test.rs"]
mod tests;
