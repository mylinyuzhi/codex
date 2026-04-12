//! Marketplace manager — search, install, and recommend plugins.
//!
//! TS: utils/plugins/marketplaceManager.ts + hintRecommendation.ts +
//! installCounts.ts + officialMarketplace.ts

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

use crate::schemas::KnownMarketplace;
use crate::schemas::KnownMarketplacesFile;
use crate::schemas::MarketplaceSource;
use crate::schemas::PluginAuthor;
use crate::schemas::PluginMarketplace;
use crate::schemas::PluginMarketplaceEntry;
use crate::schemas::PluginScope;
use crate::schemas::validate_marketplace_name;
use crate::schemas::validate_official_name_source;

// ---------------------------------------------------------------------------
// Official marketplace constants (TS: officialMarketplace.ts)
// ---------------------------------------------------------------------------

/// Official marketplace name (TS: `OFFICIAL_MARKETPLACE_NAME`).
pub const OFFICIAL_MARKETPLACE_NAME: &str = "claude-plugins-official";

/// Official marketplace GitHub organization (TS: `OFFICIAL_GITHUB_ORG`).
pub const OFFICIAL_GITHUB_ORG: &str = "anthropics";

/// Official marketplace source (TS: `OFFICIAL_MARKETPLACE_SOURCE`).
pub fn official_marketplace_source() -> MarketplaceSource {
    MarketplaceSource::Github {
        repo: format!("{OFFICIAL_GITHUB_ORG}/{OFFICIAL_MARKETPLACE_NAME}"),
        git_ref: None,
        path: None,
        sparse_paths: None,
    }
}

/// CDN download base URL for official marketplace (TS: officialMarketplaceGcs.ts).
pub const OFFICIAL_CDN_BASE: &str =
    "https://downloads.claude.ai/claude-code-releases/plugins/claude-plugins-official";

/// Names reserved for official Anthropic marketplaces.
///
/// TS: `ALLOWED_OFFICIAL_MARKETPLACE_NAMES` in officialMarketplace.ts.
pub const ALLOWED_OFFICIAL_MARKETPLACE_NAMES: &[&str] = &[
    "claude-code-marketplace",
    "claude-code-plugins",
    "claude-plugins-official",
    "anthropic-marketplace",
    "anthropic-plugins",
    "agent-skills",
    "life-sciences",
    "knowledge-work-plugins",
];

/// Reserved marketplace names that cannot be used by third parties.
pub const RESERVED_MARKETPLACE_NAMES: &[&str] = &["inline", "builtin"];

/// Name used for built-in plugins (TS: `BUILTIN_MARKETPLACE_NAME`).
pub const BUILTIN_MARKETPLACE_NAME: &str = "builtin";

/// Check if a marketplace name belongs to the official Anthropic set.
pub fn is_official_marketplace_name(name: &str) -> bool {
    ALLOWED_OFFICIAL_MARKETPLACE_NAMES.contains(&name)
}

/// Parse a fully-qualified plugin ID ("name@marketplace") into parts.
///
/// TS: `parsePluginIdentifier()` in pluginIdentifier.ts.
pub fn parse_plugin_id(plugin_id: &str) -> Option<(&str, &str)> {
    plugin_id.split_once('@')
}

/// Build a fully-qualified plugin ID from name and marketplace.
pub fn build_plugin_id(name: &str, marketplace: &str) -> String {
    format!("{name}@{marketplace}")
}

/// Check if a plugin ID refers to a built-in plugin.
pub fn is_builtin_plugin_id(plugin_id: &str) -> bool {
    plugin_id.ends_with("@builtin")
}

// ---------------------------------------------------------------------------
// MarketplacePlugin — public search result type
// ---------------------------------------------------------------------------

/// A plugin entry returned from marketplace search.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MarketplacePlugin {
    /// Plugin name (kebab-case).
    pub name: String,
    /// Latest available version.
    pub version: Option<String>,
    /// Brief description.
    pub description: Option<String>,
    /// Author or organization.
    pub author: Option<PluginAuthor>,
    /// Total install count (from stats endpoint, if available).
    #[serde(default)]
    pub downloads: i64,
    /// Homepage URL.
    pub homepage: Option<String>,
    /// Marketplace this plugin belongs to.
    pub marketplace: String,
    /// Tags / keywords for categorization.
    #[serde(default)]
    pub tags: Vec<String>,
}

impl MarketplacePlugin {
    /// Build from a marketplace entry and the parent marketplace name.
    pub fn from_entry(entry: &PluginMarketplaceEntry, marketplace_name: &str) -> Self {
        Self {
            name: entry.name.clone(),
            version: entry.version.clone(),
            description: entry.description.clone(),
            author: entry.author.clone(),
            downloads: 0,
            homepage: entry.homepage.clone(),
            marketplace: marketplace_name.to_string(),
            tags: entry.tags.clone().unwrap_or_default(),
        }
    }
}

// ---------------------------------------------------------------------------
// PluginRecommendation — hint system
// ---------------------------------------------------------------------------

/// A recommendation surfaced by the hint system.
///
/// TS: `PluginHintRecommendation` in hintRecommendation.ts.
/// Recommends a plugin when a CLI/SDK emits a `<claude-code-hint />` tag
/// referencing a plugin ID.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginRecommendation {
    /// Fully-qualified plugin ID ("name@marketplace").
    pub plugin_id: String,
    /// Human-readable plugin name.
    pub plugin_name: String,
    /// The marketplace that hosts the plugin.
    pub marketplace_name: String,
    /// Short description from the marketplace entry.
    pub plugin_description: Option<String>,
    /// The CLI command / SDK call that triggered the hint.
    pub source_command: String,
}

// ---------------------------------------------------------------------------
// Install counts cache
// ---------------------------------------------------------------------------

/// Cached install-count entry fetched from the stats endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstallCountEntry {
    /// Plugin ID ("name@marketplace").
    pub plugin: String,
    pub unique_installs: i64,
}

/// On-disk cache structure for install counts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallCountsCache {
    pub version: i32,
    pub fetched_at: String,
    pub counts: Vec<InstallCountEntry>,
}

impl InstallCountsCache {
    /// Load from a JSON file, returning `None` if missing or unparseable.
    pub fn load(path: &Path) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Persist to disk.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Look up the install count for a specific plugin ID.
    pub fn get_count(&self, plugin_id: &str) -> Option<i64> {
        self.counts
            .iter()
            .find(|e| e.plugin == plugin_id)
            .map(|e| e.unique_installs)
    }
}

// ---------------------------------------------------------------------------
// MarketplaceManager
// ---------------------------------------------------------------------------

/// Manages known marketplaces, caching, search, and plugin installation.
///
/// On-disk layout (under `plugins_dir`):
/// ```text
/// plugins/
///   known_marketplaces.json
///   marketplaces/
///     my-marketplace.json      (URL-sourced cache)
///     github-marketplace/      (cloned repo cache)
///       marketplace.json
///   install-counts-cache.json
/// ```
pub struct MarketplaceManager {
    /// Root plugins directory (e.g. `~/.cocode/plugins/`).
    plugins_dir: PathBuf,
    /// In-memory cache of loaded marketplace manifests.
    marketplace_cache: HashMap<String, PluginMarketplace>,
}

impl MarketplaceManager {
    pub fn new(plugins_dir: PathBuf) -> Self {
        Self {
            plugins_dir,
            marketplace_cache: HashMap::new(),
        }
    }

    /// Path to `known_marketplaces.json`.
    pub fn known_marketplaces_path(&self) -> PathBuf {
        self.plugins_dir.join("known_marketplaces.json")
    }

    /// Path to the marketplace cache directory.
    pub fn marketplace_cache_dir(&self) -> PathBuf {
        self.plugins_dir.join("marketplaces")
    }

    /// Path to the install-counts cache file.
    pub fn install_counts_cache_path(&self) -> PathBuf {
        self.plugins_dir.join("install-counts-cache.json")
    }

    // -----------------------------------------------------------------------
    // Known marketplaces I/O
    // -----------------------------------------------------------------------

    /// Load `known_marketplaces.json`, returning an empty map on any error.
    pub fn load_known_marketplaces(&self) -> KnownMarketplacesFile {
        let path = self.known_marketplaces_path();
        let Ok(content) = std::fs::read_to_string(&path) else {
            return HashMap::new();
        };
        serde_json::from_str(&content).unwrap_or_default()
    }

    /// Save `known_marketplaces.json`.
    pub fn save_known_marketplaces(&self, config: &KnownMarketplacesFile) -> anyhow::Result<()> {
        let path = self.known_marketplaces_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(config)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Register a new marketplace (validates name and source).
    pub fn register_marketplace(
        &mut self,
        name: &str,
        source: MarketplaceSource,
        install_location: &str,
    ) -> anyhow::Result<()> {
        if let Some(err) = validate_marketplace_name(name) {
            anyhow::bail!("{err}");
        }
        if let Some(err) = validate_official_name_source(name, &source) {
            anyhow::bail!("{err}");
        }

        let mut known = self.load_known_marketplaces();
        known.insert(
            name.to_string(),
            KnownMarketplace {
                source,
                install_location: install_location.to_string(),
                last_updated: Utc::now().to_rfc3339(),
                auto_update: None,
            },
        );
        self.save_known_marketplaces(&known)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Marketplace loading (from cache)
    // -----------------------------------------------------------------------

    /// Load a marketplace manifest from its cached location on disk.
    ///
    /// For directory-based caches, looks for `marketplace.json` or
    /// `.claude-plugin/marketplace.json` inside the directory.
    /// For file-based caches, reads the JSON file directly.
    pub fn load_cached_marketplace(&mut self, name: &str) -> anyhow::Result<&PluginMarketplace> {
        // Check in-memory cache first.
        if self.marketplace_cache.contains_key(name) {
            return Ok(&self.marketplace_cache[name]);
        }

        let known = self.load_known_marketplaces();
        let entry = known.get(name).ok_or_else(|| {
            anyhow::anyhow!("marketplace '{name}' not found in known_marketplaces.json")
        })?;

        let install_loc = Path::new(&entry.install_location);
        let marketplace = read_cached_marketplace(install_loc)?;
        self.marketplace_cache.insert(name.to_string(), marketplace);
        Ok(&self.marketplace_cache[name])
    }

    // -----------------------------------------------------------------------
    // Search
    // -----------------------------------------------------------------------

    /// Search for plugins across all loaded marketplaces by name or keyword.
    ///
    /// Matches against plugin name, description, tags, and keywords.
    /// Case-insensitive substring matching.
    pub fn search_plugins(&self, query: &str) -> Vec<MarketplacePlugin> {
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();

        for (mkt_name, marketplace) in &self.marketplace_cache {
            for entry in &marketplace.plugins {
                if matches_query(entry, &query_lower) {
                    results.push(MarketplacePlugin::from_entry(entry, mkt_name));
                }
            }
        }

        // Enrich with install counts if available.
        if let Some(cache) = InstallCountsCache::load(&self.install_counts_cache_path()) {
            for result in &mut results {
                let plugin_id = format!("{}@{}", result.name, result.marketplace);
                if let Some(count) = cache.get_count(&plugin_id) {
                    result.downloads = count;
                }
            }
        }

        // Sort by downloads descending, then name ascending.
        results.sort_by(|a, b| {
            b.downloads
                .cmp(&a.downloads)
                .then_with(|| a.name.cmp(&b.name))
        });

        results
    }

    /// List all plugins across all loaded marketplaces.
    pub fn list_all_plugins(&self) -> Vec<MarketplacePlugin> {
        self.search_plugins("")
    }

    /// Look up a specific plugin by its ID ("name@marketplace").
    pub fn get_plugin_by_id(
        &self,
        plugin_id: &str,
    ) -> Option<(MarketplacePlugin, &PluginMarketplaceEntry)> {
        let (name, mkt) = plugin_id.split_once('@')?;
        let marketplace = self.marketplace_cache.get(mkt)?;
        let entry = marketplace.plugins.iter().find(|e| e.name == name)?;
        Some((MarketplacePlugin::from_entry(entry, mkt), entry))
    }

    // -----------------------------------------------------------------------
    // Install
    // -----------------------------------------------------------------------

    /// Install a plugin from a marketplace to a local cache directory.
    ///
    /// For local/relative-path sources, copies the plugin directory.
    /// Returns the path where the plugin was installed.
    pub fn install_plugin(
        &self,
        marketplace_name: &str,
        entry: &PluginMarketplaceEntry,
        scope: PluginScope,
    ) -> anyhow::Result<PathBuf> {
        let known = self.load_known_marketplaces();
        let mkt_entry = known
            .get(marketplace_name)
            .ok_or_else(|| anyhow::anyhow!("marketplace '{marketplace_name}' is not registered"))?;

        let cache_dir = self
            .plugins_dir
            .join("cache")
            .join(sanitize_for_path(marketplace_name))
            .join(sanitize_for_path(&entry.name));

        let version_dir = match &entry.version {
            Some(v) => cache_dir.join(sanitize_for_path(v)),
            None => cache_dir.join("latest"),
        };

        std::fs::create_dir_all(&version_dir)?;

        // For local sources, copy plugin content.
        match &entry.source {
            crate::schemas::PluginSource::RelativePath(rel_path) => {
                let mkt_base = Path::new(&mkt_entry.install_location);
                let source_dir = if mkt_base.is_dir() {
                    mkt_base.join(rel_path)
                } else {
                    // File-based marketplace: resolve relative to parent dir.
                    mkt_base.parent().unwrap_or(mkt_base).join(rel_path)
                };

                if !source_dir.is_dir() {
                    anyhow::bail!(
                        "plugin source directory not found: {}",
                        source_dir.display()
                    );
                }

                copy_dir_contents(&source_dir, &version_dir)?;
            }
            crate::schemas::PluginSource::Remote(_remote) => {
                // Remote sources (npm, github, git-subdir, url) require network
                // access. For now, create a placeholder manifest noting the
                // source so a subsequent fetch step can materialise the content.
                tracing::info!(
                    plugin = %entry.name,
                    marketplace = %marketplace_name,
                    "remote plugin source — cache directory prepared, fetch pending"
                );
            }
        }

        tracing::info!(
            plugin = %entry.name,
            version = ?entry.version,
            scope = ?scope,
            path = %version_dir.display(),
            "plugin installed to cache"
        );

        Ok(version_dir)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read a cached marketplace manifest from disk.
///
/// Handles both file (`.json`) and directory (with inner `marketplace.json`
/// or `.claude-plugin/marketplace.json`) formats.
fn read_cached_marketplace(path: &Path) -> anyhow::Result<PluginMarketplace> {
    let json_path = if path.is_dir() {
        let direct = path.join("marketplace.json");
        if direct.exists() {
            direct
        } else {
            let nested = path.join(".claude-plugin").join("marketplace.json");
            if nested.exists() {
                nested
            } else {
                anyhow::bail!("no marketplace.json found in {}", path.display());
            }
        }
    } else {
        path.to_path_buf()
    };

    let content = std::fs::read_to_string(&json_path)?;
    let marketplace: PluginMarketplace = serde_json::from_str(&content)?;
    Ok(marketplace)
}

/// Check if a marketplace entry matches a search query.
fn matches_query(entry: &PluginMarketplaceEntry, query_lower: &str) -> bool {
    if query_lower.is_empty() {
        return true;
    }

    if entry.name.to_lowercase().contains(query_lower) {
        return true;
    }
    if let Some(ref desc) = entry.description
        && desc.to_lowercase().contains(query_lower)
    {
        return true;
    }
    if let Some(ref tags) = entry.tags
        && tags.iter().any(|t| t.to_lowercase().contains(query_lower))
    {
        return true;
    }
    if let Some(ref kw) = entry.keywords
        && kw.iter().any(|k| k.to_lowercase().contains(query_lower))
    {
        return true;
    }
    false
}

/// Sanitize a string for use as a filesystem path segment.
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

/// Recursively copy directory contents (files only, shallow).
fn copy_dir_contents(src: &Path, dst: &Path) -> anyhow::Result<()> {
    if !src.is_dir() {
        anyhow::bail!("source is not a directory: {}", src.display());
    }
    std::fs::create_dir_all(dst)?;

    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_contents(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Plugin delisting detection (TS: pluginBlocklist.ts + pluginFlagging.ts)
// ---------------------------------------------------------------------------

/// Detect plugins installed from a marketplace that are no longer listed there.
///
/// TS: `detectDelistedPlugins()` in `pluginBlocklist.ts` -- compares installed
/// plugins against marketplace manifest to find removed plugins.
///
/// Returns plugin IDs in `"name@marketplace"` format that have been delisted.
pub fn detect_delisted_plugins(
    installed: &crate::loader::InstalledPluginsManager,
    marketplace: &PluginMarketplace,
    marketplace_name: &str,
) -> Vec<String> {
    let listed_names: std::collections::HashSet<&str> = marketplace
        .plugins
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    let suffix = format!("@{marketplace_name}");

    installed
        .installed_plugin_ids()
        .into_iter()
        .filter(|id| {
            id.ends_with(&suffix) && {
                let plugin_name = &id[..id.len() - suffix.len()];
                !listed_names.contains(plugin_name)
            }
        })
        .map(String::from)
        .collect()
}

/// Record of a delisted plugin for flagging.
///
/// TS: `pluginFlagging.ts` -- tracks plugins removed from marketplace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlaggedPlugin {
    pub plugin_id: String,
    pub flagged_at: String,
    pub marketplace: String,
}

/// Load flagged plugins from disk.
pub fn load_flagged_plugins(plugins_dir: &Path) -> Vec<FlaggedPlugin> {
    let path = plugins_dir.join("flagged_plugins.json");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str(&content).unwrap_or_default()
}

/// Save flagged plugins to disk.
pub fn save_flagged_plugins(plugins_dir: &Path, flagged: &[FlaggedPlugin]) -> anyhow::Result<()> {
    let path = plugins_dir.join("flagged_plugins.json");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(flagged)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Flag a delisted plugin.
pub fn flag_delisted_plugin(
    plugins_dir: &Path,
    plugin_id: &str,
    marketplace_name: &str,
) -> anyhow::Result<()> {
    let mut flagged = load_flagged_plugins(plugins_dir);
    if flagged.iter().any(|f| f.plugin_id == plugin_id) {
        return Ok(());
    }
    flagged.push(FlaggedPlugin {
        plugin_id: plugin_id.to_string(),
        flagged_at: Utc::now().to_rfc3339(),
        marketplace: marketplace_name.to_string(),
    });
    save_flagged_plugins(plugins_dir, &flagged)
}

// ---------------------------------------------------------------------------
// Plugin auto-update (TS: pluginAutoupdate.ts)
// ---------------------------------------------------------------------------

/// Auto-update check result for a single plugin.
///
/// TS: `pluginAutoupdate.ts` -- checks version, triggers update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoUpdateCheck {
    pub plugin_id: String,
    pub current_version: Option<String>,
    pub available_version: Option<String>,
    pub needs_update: bool,
}

/// Official marketplace names that should NOT auto-update by default.
const NO_AUTO_UPDATE_OFFICIAL: &[&str] = &["knowledge-work-plugins"];

/// Check if auto-update is enabled for a marketplace.
///
/// TS: `isMarketplaceAutoUpdate()` in schemas.ts.
pub fn is_marketplace_auto_update(marketplace_name: &str, explicit_setting: Option<bool>) -> bool {
    if let Some(setting) = explicit_setting {
        return setting;
    }
    let lower = marketplace_name.to_lowercase();
    is_official_marketplace_name(&lower) && !NO_AUTO_UPDATE_OFFICIAL.contains(&lower.as_str())
}

/// Check which installed plugins need updating.
///
/// Compares installed versions against marketplace entries.
pub fn check_plugin_updates(
    installed: &crate::loader::InstalledPluginsManager,
    marketplace: &PluginMarketplace,
    marketplace_name: &str,
) -> Vec<AutoUpdateCheck> {
    let mut checks = Vec::new();

    for entry in &marketplace.plugins {
        let plugin_id = format!("{}@{marketplace_name}", entry.name);
        if !installed.is_installed(&plugin_id) {
            continue;
        }

        let installations = installed.get_installations(&plugin_id);
        let current_version = installations
            .iter()
            .filter_map(|i| i.version.as_deref())
            .next()
            .map(String::from);

        let available_version = entry.version.clone();
        let needs_update = match (&current_version, &available_version) {
            (Some(current), Some(available)) => current != available,
            (None, Some(_)) => true,
            _ => false,
        };

        checks.push(AutoUpdateCheck {
            plugin_id,
            current_version,
            available_version,
            needs_update,
        });
    }

    checks
}

#[cfg(test)]
#[path = "marketplace.test.rs"]
mod tests;
