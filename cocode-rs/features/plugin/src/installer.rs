//! High-level plugin install/uninstall orchestration.

use std::path::PathBuf;

use tracing::info;

use crate::cache;
use crate::error::Result;
use crate::error::plugin_error::InstallationFailedSnafu;
use crate::error::plugin_error::PluginNotInstalledSnafu;
use crate::git_clone;
use crate::installed_registry::InstalledPluginEntry;
use crate::installed_registry::InstalledPluginsRegistry;
use crate::marketplace_manager::MarketplaceManager;
use crate::marketplace_types::MarketplacePluginSource;
use crate::marketplace_types::MarketplaceSource;
use crate::plugin_settings::PluginSettings;
use crate::scope::PluginScope;

/// Result of a successful installation.
#[derive(Debug)]
pub struct InstallResult {
    pub plugin_id: String,
    pub version: String,
    pub install_path: PathBuf,
}

/// Information about an installed plugin.
pub struct PluginInfo {
    pub id: String,
    pub version: String,
    pub scope: String,
    pub enabled: bool,
    pub install_path: PathBuf,
}

/// High-level plugin installer.
pub struct PluginInstaller {
    plugins_dir: PathBuf,
}

impl PluginInstaller {
    /// Create a new installer.
    pub fn new(plugins_dir: PathBuf) -> Self {
        Self { plugins_dir }
    }

    fn registry_path(&self) -> PathBuf {
        self.plugins_dir.join("installed_plugins.json")
    }

    fn settings_path(&self) -> PathBuf {
        self.plugins_dir.join("settings.json")
    }

    /// Install a plugin from a marketplace.
    pub async fn install(&self, plugin_id: &str, scope: PluginScope) -> Result<InstallResult> {
        let marketplace = MarketplaceManager::new(self.plugins_dir.clone());
        let found = marketplace.find_plugin(plugin_id).ok_or_else(|| {
            InstallationFailedSnafu {
                plugin_id: plugin_id.to_string(),
                message: "Plugin not found in any marketplace".to_string(),
            }
            .build()
        })?;

        // Determine where to fetch the plugin source from
        let tmp_path =
            std::env::temp_dir().join(format!("cocode-plugin-install-{}", std::process::id()));
        // Ensure temp dir exists and is clean
        if tmp_path.exists() {
            let _ = std::fs::remove_dir_all(&tmp_path);
        }
        std::fs::create_dir_all(&tmp_path).map_err(|e| {
            InstallationFailedSnafu {
                plugin_id: plugin_id.to_string(),
                message: format!("Failed to create temp dir: {e}"),
            }
            .build()
        })?;
        // Ensure cleanup on exit
        let _cleanup_guard = TempDirGuard(tmp_path.clone());

        let source_dir = match &found.entry.source {
            MarketplacePluginSource::RelativePath(rel) => {
                // Resolve relative to marketplace install location
                let resolved = found.marketplace_install_location.join(rel);
                if resolved.exists() {
                    resolved
                } else {
                    return Err(InstallationFailedSnafu {
                        plugin_id: plugin_id.to_string(),
                        message: format!("Plugin source not found: {}", resolved.display()),
                    }
                    .build());
                }
            }
            MarketplacePluginSource::Remote(source) => {
                let clone_target = tmp_path.join("plugin");
                clone_source(source, &clone_target).await?;
                clone_target
            }
        };

        // Read manifest from source to get version
        let manifest_path = source_dir.join("PLUGIN.toml");
        let manifest_version = if manifest_path.exists() {
            std::fs::read_to_string(&manifest_path)
                .ok()
                .and_then(|content| {
                    let manifest: toml::Value = toml::from_str(&content).ok()?;
                    manifest
                        .get("plugin")?
                        .get("version")?
                        .as_str()
                        .map(String::from)
                })
        } else {
            None
        };

        let version = cache::resolve_version(
            manifest_version.as_deref(),
            found.entry.version.as_deref(),
            Some(&source_dir),
        );

        // Copy to versioned cache
        let cache_path = cache::versioned_cache_path(
            &self.plugins_dir,
            &found.marketplace_name,
            &found.entry.name,
            &version,
        );
        cache::copy_to_versioned_cache(&source_dir, &cache_path)?;

        // Get git commit SHA if available
        let git_sha = git_clone::get_commit_sha(&source_dir).await.ok().flatten();

        // Update registry
        let mut registry = InstalledPluginsRegistry::load(&self.registry_path());
        let now = chrono::Utc::now().to_rfc3339();
        registry.add(
            &found.entry.name,
            InstalledPluginEntry {
                scope: scope.to_string(),
                version: version.clone(),
                install_path: cache_path.clone(),
                installed_at: now.clone(),
                last_updated: now,
                git_commit_sha: git_sha,
                project_path: None,
            },
        );
        registry.save(&self.registry_path())?;

        // Enable the plugin
        let mut settings = PluginSettings::load(&self.settings_path());
        settings.set_enabled(&found.entry.name, true);
        settings.save(&self.settings_path())?;

        info!(
            plugin = %found.entry.name,
            version = %version,
            scope = %scope,
            path = %cache_path.display(),
            "Plugin installed"
        );

        Ok(InstallResult {
            plugin_id: found.entry.name,
            version,
            install_path: cache_path,
        })
    }

    /// Uninstall a plugin.
    pub async fn uninstall(&self, plugin_id: &str, scope: PluginScope) -> Result<()> {
        let mut registry = InstalledPluginsRegistry::load(&self.registry_path());
        let scope_str = scope.to_string();

        let entry = registry.remove(plugin_id, &scope_str).ok_or_else(|| {
            PluginNotInstalledSnafu {
                plugin_id: plugin_id.to_string(),
            }
            .build()
        })?;

        // Delete cached files
        let cache_root = cache::cache_dir(&self.plugins_dir);
        cache::delete_plugin_cache(&entry.install_path, &cache_root)?;

        // Remove from settings
        let mut settings = PluginSettings::load(&self.settings_path());
        settings.remove(plugin_id);
        settings.save(&self.settings_path())?;

        // Save updated registry
        registry.save(&self.registry_path())?;

        info!(
            plugin = plugin_id,
            scope = %scope,
            "Plugin uninstalled"
        );

        Ok(())
    }

    /// Update a plugin (re-install from marketplace).
    pub async fn update(&self, plugin_id: &str, scope: PluginScope) -> Result<InstallResult> {
        // Refresh marketplace data first
        let marketplace = MarketplaceManager::new(self.plugins_dir.clone());
        let _ = marketplace.refresh_all().await;

        // Re-install
        self.install(plugin_id, scope).await
    }

    /// List all installed plugins with their state.
    pub fn list_installed(&self) -> Vec<PluginInfo> {
        let registry = InstalledPluginsRegistry::load(&self.registry_path());
        let settings = PluginSettings::load(&self.settings_path());

        let mut plugins = Vec::new();
        for (id, entries) in &registry.plugins {
            for entry in entries {
                plugins.push(PluginInfo {
                    id: id.clone(),
                    version: entry.version.clone(),
                    scope: entry.scope.clone(),
                    enabled: settings.is_enabled(id),
                    install_path: entry.install_path.clone(),
                });
            }
        }

        plugins
    }
}

/// RAII guard that removes a temporary directory on drop.
struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn clone_source(source: &MarketplaceSource, target: &std::path::Path) -> Result<()> {
    match source {
        MarketplaceSource::Github { repo, git_ref } => {
            let url = format!("https://github.com/{repo}.git");
            git_clone::git_clone_with_fallback(&url, target, git_ref.as_deref()).await
        }
        MarketplaceSource::Git { url, git_ref } => {
            git_clone::git_clone_with_fallback(url, target, git_ref.as_deref()).await
        }
        MarketplaceSource::Directory { path } => cache::copy_to_versioned_cache(path, target),
        MarketplaceSource::File { path } => {
            std::fs::create_dir_all(target).map_err(|e| {
                InstallationFailedSnafu {
                    plugin_id: path.display().to_string(),
                    message: format!("Failed to create dir: {e}"),
                }
                .build()
            })?;
            std::fs::copy(path, target.join("PLUGIN.toml")).map_err(|e| {
                InstallationFailedSnafu {
                    plugin_id: path.display().to_string(),
                    message: format!("Failed to copy: {e}"),
                }
                .build()
            })?;
            Ok(())
        }
        MarketplaceSource::Url { url } => {
            std::fs::create_dir_all(target).map_err(|e| {
                InstallationFailedSnafu {
                    plugin_id: url.clone(),
                    message: format!("Failed to create dir: {e}"),
                }
                .build()
            })?;

            // Download the URL content using curl
            let output = tokio::process::Command::new("curl")
                .args(["-fsSL", "-o"])
                .arg(target.join("plugin.tar.gz"))
                .arg(url)
                .output()
                .await
                .map_err(|e| {
                    InstallationFailedSnafu {
                        plugin_id: url.clone(),
                        message: format!("Failed to fetch URL: {e}"),
                    }
                    .build()
                })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(InstallationFailedSnafu {
                    plugin_id: url.clone(),
                    message: format!("Failed to download plugin: {stderr}"),
                }
                .build());
            }

            // Try to extract if it looks like a tarball
            let tar_path = target.join("plugin.tar.gz");
            let extract_output = tokio::process::Command::new("tar")
                .args(["-xzf"])
                .arg(&tar_path)
                .arg("-C")
                .arg(target)
                .output()
                .await;

            match extract_output {
                Ok(out) if out.status.success() => {
                    // Remove the tarball after extraction
                    let _ = std::fs::remove_file(&tar_path);
                }
                _ => {
                    // Not a tarball — treat the downloaded file as PLUGIN.toml directly
                    let _ = std::fs::rename(&tar_path, target.join("PLUGIN.toml"));
                }
            }

            // Verify PLUGIN.toml exists
            if !target.join("PLUGIN.toml").exists() {
                return Err(InstallationFailedSnafu {
                    plugin_id: url.clone(),
                    message: "Downloaded content does not contain PLUGIN.toml".to_string(),
                }
                .build());
            }

            Ok(())
        }
    }
}

#[cfg(test)]
#[path = "installer.test.rs"]
mod tests;
