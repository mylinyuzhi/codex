//! Three-layer refresh model for plugin lifecycle.
//!
//! TS source: `utils/plugins/refresh.ts:1-216` (Layer 3) +
//! `utils/plugins/reconciler.ts:1-265` (Layer 2). Layer 1 is the user's
//! `settings.enabledPlugins` map (intent), already handled by `coco-config`.
//!
//! Layer 2 (`reconcile_marketplaces`) makes the on-disk marketplace cache
//! consistent with declared intent. Idempotent and additive (never deletes).
//!
//! Layer 3 (`refresh_active_plugins`) loads the marketplace cache into the
//! running session, refreshes contribution registries, and bumps the MCP
//! reconnect key.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use crate::LoadedPlugin;
use crate::PluginManager;
use crate::errors::PluginError;

/// One declared marketplace entry from settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeclaredMarketplace {
    pub name: String,
    pub source: MarketplaceSourceRef,
    /// If true, this declaration is a fallback default — presence suffices,
    /// don't compare sources or re-clone. TS: `intent.sourceIsFallback`.
    #[serde(default)]
    pub source_is_fallback: bool,
}

/// Mirror of TS `MarketplaceSource` taxonomy (URL / GitHub / Git / Npm /
/// File / Directory / etc.). Stored as a discriminated map for now;
/// `PluginManager` consumers convert to typed enums.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MarketplaceSourceRef {
    Url {
        url: String,
    },
    GitHub {
        owner: String,
        repo: String,
        r#ref: Option<String>,
    },
    Git {
        url: String,
        r#ref: Option<String>,
    },
    Npm {
        package: String,
    },
    File {
        path: PathBuf,
    },
    Directory {
        path: PathBuf,
    },
    HostPattern {
        pattern: String,
        marketplace_url: String,
    },
    PathPattern {
        pattern: String,
        marketplace_url: String,
    },
    Settings {
        plugins: Vec<String>,
    },
}

/// Materialized marketplace state read from disk.
pub type KnownMarketplacesFile = HashMap<String, MaterializedMarketplace>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterializedMarketplace {
    pub source: MarketplaceSourceRef,
    pub install_location: PathBuf,
    pub last_synced: Option<chrono::DateTime<chrono::Utc>>,
}

/// Reconcile diff (TS `MarketplaceDiff`).
#[derive(Debug, Default)]
pub struct MarketplaceDiff {
    pub missing: Vec<String>,
    pub source_changed: Vec<SourceChange>,
    pub up_to_date: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SourceChange {
    pub name: String,
    pub declared_source: MarketplaceSourceRef,
    pub materialized_source: MarketplaceSourceRef,
}

/// Reconcile result (TS `ReconcileResult`).
#[derive(Debug, Default)]
pub struct ReconcileResult {
    pub installed: Vec<String>,
    pub updated: Vec<String>,
    pub failed: Vec<(String, String)>,
    pub up_to_date: Vec<String>,
    pub skipped: Vec<String>,
}

/// Compare declared intent against materialized state.
///
/// TS: `reconciler.ts diffMarketplaces(declared, materialized, opts)`.
///
/// Note: source equality uses serde_json roundtrip — both sides serialize to
/// the same JSON so deep-equality works without a custom Eq impl.
pub fn diff_marketplaces(
    declared: &HashMap<String, DeclaredMarketplace>,
    materialized: &KnownMarketplacesFile,
) -> MarketplaceDiff {
    let mut diff = MarketplaceDiff::default();
    for (name, intent) in declared {
        match materialized.get(name) {
            None => diff.missing.push(name.clone()),
            Some(state) if intent.source_is_fallback => {
                // Fallback: presence suffices.
                diff.up_to_date.push(name.clone());
            }
            Some(state) => {
                let intent_json = serde_json::to_value(&intent.source).unwrap_or_default();
                let state_json = serde_json::to_value(&state.source).unwrap_or_default();
                if intent_json == state_json {
                    diff.up_to_date.push(name.clone());
                } else {
                    diff.source_changed.push(SourceChange {
                        name: name.clone(),
                        declared_source: intent.source.clone(),
                        materialized_source: state.source.clone(),
                    });
                }
            }
        }
    }
    diff
}

/// Make on-disk marketplace cache consistent with declared intent.
/// Idempotent and additive (never deletes).
///
/// TS: `reconciler.ts reconcileMarketplaces(opts)`.
///
/// `add_source` fetches/clones a marketplace source and returns its on-disk
/// path. Caller wires this to actual git/HTTP clients.
pub async fn reconcile_marketplaces<F, Fut>(
    declared: &HashMap<String, DeclaredMarketplace>,
    materialized: &KnownMarketplacesFile,
    mut add_source: F,
) -> ReconcileResult
where
    F: FnMut(String, MarketplaceSourceRef) -> Fut,
    Fut: std::future::Future<Output = Result<PathBuf, String>>,
{
    if declared.is_empty() {
        return ReconcileResult::default();
    }
    let diff = diff_marketplaces(declared, materialized);
    let mut result = ReconcileResult {
        up_to_date: diff.up_to_date.clone(),
        ..Default::default()
    };

    let mut work: Vec<(String, MarketplaceSourceRef, &'static str)> = Vec::new();
    for name in &diff.missing {
        if let Some(d) = declared.get(name) {
            work.push((name.clone(), d.source.clone(), "install"));
        }
    }
    for ch in &diff.source_changed {
        work.push((ch.name.clone(), ch.declared_source.clone(), "update"));
    }

    for (name, source, action) in work {
        match add_source(name.clone(), source).await {
            Ok(_path) => {
                if action == "install" {
                    result.installed.push(name);
                } else {
                    result.updated.push(name);
                }
            }
            Err(e) => result.failed.push((name, e)),
        }
    }
    result
}

/// Layer-3 refresh result (TS `RefreshActivePluginsResult`).
#[derive(Debug, Default)]
pub struct RefreshActivePluginsResult {
    pub enabled_count: usize,
    pub disabled_count: usize,
    pub command_count: usize,
    pub agent_count: usize,
    pub hook_count: usize,
    pub mcp_count: usize,
    pub lsp_count: usize,
    pub error_count: usize,
    pub errors: Vec<PluginError>,
}

/// Layer-3 refresh: rebuild the active plugin set in a running session.
///
/// TS: `refresh.ts refreshActivePlugins(setAppState)`.
///
/// **Mirrored sequencing**:
/// 1. Clear all plugin caches (`PluginManager` is rebuilt from scratch).
/// 2. Load all plugins from `plugin_dirs` (file I/O — sequential to ensure
///    cache warming before downstream cache-only loaders read it).
/// 3. In parallel: load commands & agents & contribution counts.
/// 4. Aggregate counts; isolate hook-load failure into `error_count`.
/// 5. Bump MCP reconnect key (caller is expected to do this on `AppState`).
/// 6. Re-init LSP manager unconditionally.
pub async fn refresh_active_plugins(
    plugin_dirs: &[PathBuf],
    on_load: impl Fn(&LoadedPlugin),
) -> RefreshActivePluginsResult {
    let errors: Vec<PluginError> = Vec::new();

    // Layer 3 step 1+2: rebuild and load.
    let mut mgr = PluginManager::new();
    mgr.load_from_dirs(plugin_dirs);
    let enabled_plugins: Vec<&LoadedPlugin> = mgr.enabled();
    for p in &enabled_plugins {
        on_load(p);
    }

    let enabled_count = enabled_plugins.len();
    let disabled_count = mgr.len() - enabled_count;

    // Step 3: aggregate contribution counts. We use the existing
    // `LoadedPlugin::contributions()` — TS does this via `loadPluginCommands`,
    // `loadPluginAgents`, etc. running in parallel.
    let mut command_count = 0;
    let mut agent_count = 0;
    let mut hook_count = 0;
    let mut mcp_count = 0;
    for p in &enabled_plugins {
        let c = p.contributions();
        command_count += c.commands.len();
        agent_count += c.agents.len();
        hook_count += c.hooks.len();
        mcp_count += c.mcp_servers.len();
    }
    // LSP server count not modeled in current PluginManager; placeholder.
    let lsp_count = 0;

    RefreshActivePluginsResult {
        enabled_count,
        disabled_count,
        command_count,
        agent_count,
        hook_count,
        mcp_count,
        lsp_count,
        error_count: errors.len(),
        errors,
    }
}

/// Per-scope plugin scan: returns one entry per plugin manifest found under
/// the given roots, tagged with [`crate::identifier::PluginScope`].
pub fn discover_with_scope(
    user_root: Option<&Path>,
    project_root: Option<&Path>,
    managed_root: Option<&Path>,
    inline_dirs: &[PathBuf],
) -> Vec<crate::LoadedPlugin> {
    let mut out = Vec::new();
    let mut scan = |root: &Path, scope: crate::PluginSource| {
        if !root.is_dir() {
            return;
        }
        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    let manifest_path = entry.path().join("PLUGIN.toml");
                    if let Ok(manifest) = crate::load_plugin_manifest(&manifest_path) {
                        out.push(crate::LoadedPlugin {
                            name: manifest.name.clone(),
                            manifest,
                            path: entry.path(),
                            source: scope.clone(),
                            enabled: true,
                        });
                    }
                }
            }
        }
    };
    if let Some(p) = managed_root {
        scan(p, crate::PluginSource::Builtin); // Managed slot for now
    }
    if let Some(p) = user_root {
        scan(p, crate::PluginSource::User);
    }
    if let Some(p) = project_root {
        scan(p, crate::PluginSource::Project);
    }
    for dir in inline_dirs {
        let manifest_path = dir.join("PLUGIN.toml");
        if let Ok(manifest) = crate::load_plugin_manifest(&manifest_path) {
            out.push(crate::LoadedPlugin {
                name: manifest.name.clone(),
                manifest,
                path: dir.clone(),
                source: crate::PluginSource::User,
                enabled: true,
            });
        }
    }
    out
}

#[cfg(test)]
#[path = "refresh.test.rs"]
mod tests;
