//! Marketplace CRUD operations.
//!
//! Manages registered marketplace sources and provides plugin discovery.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use tracing::debug;
use tracing::info;
use tracing::warn;

use crate::error::Result;
use crate::error::plugin_error::CacheSnafu;
use crate::error::plugin_error::MarketplaceAlreadyExistsSnafu;
use crate::error::plugin_error::MarketplaceNotFoundSnafu;
use crate::git_clone;
use crate::marketplace_types::KnownMarketplace;
use crate::marketplace_types::MarketplaceManifest;
use crate::marketplace_types::MarketplacePluginEntry;
use crate::marketplace_types::MarketplaceSource;

/// A plugin found in a marketplace.
pub struct FoundPlugin {
    /// The plugin entry from the marketplace manifest.
    pub entry: MarketplacePluginEntry,
    /// Name of the marketplace this plugin was found in.
    pub marketplace_name: String,
    /// Install location of the marketplace.
    pub marketplace_install_location: PathBuf,
}

/// Manages registered marketplace sources.
pub struct MarketplaceManager {
    plugins_dir: PathBuf,
}

impl MarketplaceManager {
    /// Create a new marketplace manager.
    pub fn new(plugins_dir: PathBuf) -> Self {
        Self { plugins_dir }
    }

    fn config_path(&self) -> PathBuf {
        self.plugins_dir.join("known_marketplaces.json")
    }

    fn marketplaces_dir(&self) -> PathBuf {
        self.plugins_dir.join("marketplaces")
    }

    /// Add a marketplace source.
    pub async fn add_source(&self, source: MarketplaceSource) -> Result<String> {
        let name = source.derive_name();
        let mut config = self.load_config();

        if config.contains_key(&name) {
            return Err(MarketplaceAlreadyExistsSnafu { name: name.clone() }.build());
        }

        let install_location = self.marketplaces_dir().join(&name);

        // Fetch the marketplace source
        self.fetch_source(&source, &install_location).await?;

        config.insert(
            name.clone(),
            KnownMarketplace {
                source,
                install_location,
                last_updated: Some(now_iso()),
                auto_update: false,
            },
        );

        self.save_config(&config)?;
        info!(name = %name, "Marketplace added");

        Ok(name)
    }

    /// Remove a marketplace source.
    pub async fn remove_source(&self, name: &str) -> Result<()> {
        let mut config = self.load_config();

        let marketplace = config.remove(name).ok_or_else(|| {
            MarketplaceNotFoundSnafu {
                name: name.to_string(),
            }
            .build()
        })?;

        // Remove cached marketplace data
        if marketplace.install_location.exists() {
            let _ = tokio::fs::remove_dir_all(&marketplace.install_location).await;
        }

        self.save_config(&config)?;
        info!(name, "Marketplace removed");

        Ok(())
    }

    /// Refresh a marketplace's cached data.
    pub async fn refresh(&self, name: &str) -> Result<()> {
        let mut config = self.load_config();

        let marketplace = config.get_mut(name).ok_or_else(|| {
            MarketplaceNotFoundSnafu {
                name: name.to_string(),
            }
            .build()
        })?;

        // Remove existing cache
        if marketplace.install_location.exists() {
            let _ = tokio::fs::remove_dir_all(&marketplace.install_location).await;
        }

        self.fetch_source(&marketplace.source.clone(), &marketplace.install_location)
            .await?;
        marketplace.last_updated = Some(now_iso());

        self.save_config(&config)?;
        info!(name, "Marketplace refreshed");

        Ok(())
    }

    /// Refresh all marketplaces.
    pub async fn refresh_all(&self) -> Result<Vec<String>> {
        let names: Vec<String> = self.load_config().keys().cloned().collect();
        let mut refreshed = Vec::new();

        for name in &names {
            match self.refresh(name).await {
                Ok(()) => refreshed.push(name.clone()),
                Err(e) => warn!(name, error = %e, "Failed to refresh marketplace"),
            }
        }

        Ok(refreshed)
    }

    /// List all registered marketplaces.
    pub fn list(&self) -> HashMap<String, KnownMarketplace> {
        self.load_config()
    }

    /// Find a plugin by ID across all marketplaces.
    ///
    /// If the plugin_id contains `@`, it's parsed as `name@marketplace`.
    pub fn find_plugin(&self, plugin_id: &str) -> Option<FoundPlugin> {
        if let Some((name, marketplace)) = plugin_id.split_once('@') {
            return self.find_plugin_in_marketplace(name, marketplace);
        }

        // Search all marketplaces
        let config = self.load_config();
        for (marketplace_name, marketplace) in &config {
            if let Some(found) = self.find_plugin_in_marketplace(plugin_id, marketplace_name) {
                let _ = marketplace; // used for iteration
                return Some(found);
            }
        }

        None
    }

    /// Find a plugin in a specific marketplace.
    pub fn find_plugin_in_marketplace(
        &self,
        plugin_name: &str,
        marketplace: &str,
    ) -> Option<FoundPlugin> {
        let config = self.load_config();
        let km = config.get(marketplace)?;

        let manifest = self.load_marketplace_manifest(marketplace).ok()?;

        let entry = manifest
            .plugins
            .into_iter()
            .find(|p| p.name == plugin_name)?;

        Some(FoundPlugin {
            entry,
            marketplace_name: marketplace.to_string(),
            marketplace_install_location: km.install_location.clone(),
        })
    }

    fn load_config(&self) -> HashMap<String, KnownMarketplace> {
        let path = self.config_path();
        if !path.exists() {
            return HashMap::new();
        }

        match std::fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(e) => {
                warn!(path = %path.display(), error = %e, "Failed to read marketplace config");
                HashMap::new()
            }
        }
    }

    fn save_config(&self, config: &HashMap<String, KnownMarketplace>) -> Result<()> {
        let path = self.config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                CacheSnafu {
                    path: parent.to_path_buf(),
                    message: format!("Failed to create directory: {e}"),
                }
                .build()
            })?;
        }

        let content = serde_json::to_string_pretty(config).map_err(|e| {
            CacheSnafu {
                path: path.clone(),
                message: format!("Failed to serialize config: {e}"),
            }
            .build()
        })?;

        std::fs::write(&path, content).map_err(|e| {
            CacheSnafu {
                path: path.clone(),
                message: format!("Failed to write config: {e}"),
            }
            .build()
        })?;

        Ok(())
    }

    fn load_marketplace_manifest(&self, name: &str) -> Result<MarketplaceManifest> {
        let manifest_path = self.marketplaces_dir().join(name).join("marketplace.json");

        let content = std::fs::read_to_string(&manifest_path).map_err(|e| {
            MarketplaceNotFoundSnafu {
                name: format!("{name} (manifest: {}): {e}", manifest_path.display()),
            }
            .build()
        })?;

        serde_json::from_str(&content).map_err(|e| {
            CacheSnafu {
                path: manifest_path,
                message: format!("Invalid marketplace manifest: {e}"),
            }
            .build()
        })
    }

    async fn fetch_source(&self, source: &MarketplaceSource, target: &Path) -> Result<()> {
        match source {
            MarketplaceSource::Github { repo, git_ref } => {
                let url = format!("https://github.com/{repo}.git");
                git_clone::git_clone_with_fallback(&url, target, git_ref.as_deref()).await?;
            }
            MarketplaceSource::Git { url, git_ref } => {
                git_clone::git_clone_with_fallback(url, target, git_ref.as_deref()).await?;
            }
            MarketplaceSource::File { path } => {
                // Copy the file to the target as marketplace.json
                std::fs::create_dir_all(target).map_err(|e| {
                    CacheSnafu {
                        path: target.to_path_buf(),
                        message: format!("Failed to create target: {e}"),
                    }
                    .build()
                })?;
                std::fs::copy(path, target.join("marketplace.json")).map_err(|e| {
                    CacheSnafu {
                        path: path.clone(),
                        message: format!("Failed to copy marketplace file: {e}"),
                    }
                    .build()
                })?;
            }
            MarketplaceSource::Directory { path } => {
                // Symlink or copy the directory
                if target.exists() {
                    let _ = std::fs::remove_dir_all(target);
                }
                crate::cache::copy_to_versioned_cache(path, target)?;
            }
            MarketplaceSource::Url { url } => {
                debug!(url, target = %target.display(), "Fetching marketplace from URL");
                std::fs::create_dir_all(target).map_err(|e| {
                    CacheSnafu {
                        path: target.to_path_buf(),
                        message: format!("Failed to create target: {e}"),
                    }
                    .build()
                })?;

                // Use curl to fetch the URL
                let output = tokio::process::Command::new("curl")
                    .args(["-fsSL", "-o"])
                    .arg(target.join("marketplace.json"))
                    .arg(url)
                    .output()
                    .await
                    .map_err(|e| {
                        CacheSnafu {
                            path: target.to_path_buf(),
                            message: format!("Failed to fetch URL: {e}"),
                        }
                        .build()
                    })?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(CacheSnafu {
                        path: target.to_path_buf(),
                        message: format!("Failed to fetch URL: {stderr}"),
                    }
                    .build());
                }
            }
        }

        Ok(())
    }
}

/// Check whether a marketplace should be refreshed based on auto_update and age.
///
/// Returns `true` if `auto_update` is enabled and the marketplace hasn't
/// been refreshed in the last 24 hours.
pub fn should_refresh(marketplace: &KnownMarketplace) -> bool {
    if !marketplace.auto_update {
        return false;
    }

    let Some(last_updated) = &marketplace.last_updated else {
        return true;
    };

    let Ok(last) = chrono::DateTime::parse_from_rfc3339(last_updated) else {
        return true;
    };

    let age = chrono::Utc::now().signed_duration_since(last);
    age > chrono::Duration::hours(24)
}

impl MarketplaceManager {
    /// Refresh all marketplaces that are stale (auto_update enabled and older than 24h).
    ///
    /// Returns the names of successfully refreshed marketplaces.
    pub async fn auto_refresh_stale(&self) -> Result<Vec<String>> {
        let config = self.load_config();
        let stale: Vec<String> = config
            .iter()
            .filter(|(_, km)| should_refresh(km))
            .map(|(name, _)| name.clone())
            .collect();

        if stale.is_empty() {
            return Ok(Vec::new());
        }

        debug!(count = stale.len(), "Auto-refreshing stale marketplaces");

        let mut refreshed = Vec::new();
        for name in &stale {
            match self.refresh(name).await {
                Ok(()) => {
                    info!(name, "Auto-refreshed stale marketplace");
                    refreshed.push(name.clone());
                }
                Err(e) => {
                    warn!(name, error = %e, "Failed to auto-refresh marketplace");
                }
            }
        }

        Ok(refreshed)
    }
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
#[path = "marketplace_manager.test.rs"]
mod tests;
