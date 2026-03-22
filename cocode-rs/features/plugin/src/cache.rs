//! Versioned plugin cache management.
//!
//! Plugins are cached at `~/.cocode/plugins/cache/<marketplace>/<plugin>/<version>/`.

use std::path::Path;
use std::path::PathBuf;

use tracing::debug;

use crate::error::Result;
use crate::error::plugin_error::CacheSnafu;

/// Get the default plugins directory (`~/.cocode/plugins/`).
pub fn plugins_dir(cocode_home: &Path) -> PathBuf {
    cocode_home.join("plugins")
}

/// Get the cache directory under a plugins dir.
pub fn cache_dir(plugins_dir: &Path) -> PathBuf {
    plugins_dir.join("cache")
}

/// Build the versioned cache path for a plugin.
pub fn versioned_cache_path(
    plugins_dir: &Path,
    marketplace: &str,
    plugin_name: &str,
    version: &str,
) -> PathBuf {
    cache_dir(plugins_dir)
        .join(sanitize_path_component(marketplace))
        .join(sanitize_path_component(plugin_name))
        .join(sanitize_path_component(version))
}

/// Copy a plugin directory into the versioned cache.
pub fn copy_to_versioned_cache(source: &Path, target: &Path) -> Result<()> {
    if target.exists() {
        std::fs::remove_dir_all(target).map_err(|e| {
            CacheSnafu {
                path: target.to_path_buf(),
                message: format!("Failed to clean existing cache: {e}"),
            }
            .build()
        })?;
    }

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            CacheSnafu {
                path: parent.to_path_buf(),
                message: format!("Failed to create cache directory: {e}"),
            }
            .build()
        })?;
    }

    copy_dir_recursive(source, target).map_err(|e| {
        CacheSnafu {
            path: target.to_path_buf(),
            message: format!("Failed to copy to cache: {e}"),
        }
        .build()
    })?;

    debug!(
        source = %source.display(),
        target = %target.display(),
        "Copied plugin to versioned cache"
    );

    Ok(())
}

/// Delete a plugin's cached files and clean up empty parent directories.
pub fn delete_plugin_cache(install_path: &Path, cache_root: &Path) -> Result<()> {
    if install_path.exists() {
        std::fs::remove_dir_all(install_path).map_err(|e| {
            CacheSnafu {
                path: install_path.to_path_buf(),
                message: format!("Failed to delete cache: {e}"),
            }
            .build()
        })?;

        debug!(path = %install_path.display(), "Deleted plugin cache");
    }

    // Clean up empty parent directories up to the cache root
    let mut parent = install_path.parent();
    while let Some(dir) = parent {
        if dir == cache_root || !dir.starts_with(cache_root) {
            break;
        }
        if dir.exists() && is_dir_empty(dir) {
            let _ = std::fs::remove_dir(dir);
        } else {
            break;
        }
        parent = dir.parent();
    }

    Ok(())
}

/// Resolve a version string from available sources.
///
/// Priority: manifest version > marketplace version > git-derived > "0.0.0"
pub fn resolve_version(
    manifest_version: Option<&str>,
    marketplace_version: Option<&str>,
    install_path: Option<&Path>,
) -> String {
    if let Some(v) = manifest_version
        && !v.is_empty()
    {
        return v.to_string();
    }
    if let Some(v) = marketplace_version
        && !v.is_empty()
    {
        return v.to_string();
    }
    if let Some(path) = install_path {
        // Try to read a version from the directory name
        if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && name.chars().next().map(|c| c.is_ascii_digit()) == Some(true)
        {
            return name.to_string();
        }
    }
    "0.0.0".to_string()
}

/// Replace non-[a-zA-Z0-9.\-_] characters with "-".
pub fn sanitize_path_component(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if file_type.is_dir() {
            let name = entry.file_name();
            if name == ".git" {
                continue;
            }
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn is_dir_empty(path: &Path) -> bool {
    path.read_dir()
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(false)
}

/// Clean up orphaned cache entries that are no longer referenced by any
/// installed plugin.
///
/// Uses a mark-and-sweep approach:
/// 1. Collect all "live" install paths from the installed plugins registry.
/// 2. Walk the cache directory tree.
/// 3. Remove version directories that are not in the live set and are older
///    than `grace_period`.
/// 4. Remove empty parent directories up to the cache root.
///
/// Returns the number of directories removed.
pub fn cleanup_orphaned_cache(
    plugins_dir: &Path,
    grace_period: std::time::Duration,
) -> Result<i32> {
    use std::collections::HashSet;

    let cache_root = cache_dir(plugins_dir);
    if !cache_root.exists() {
        return Ok(0);
    }

    // Build the "live" set from the installed plugins registry
    let registry_path = plugins_dir.join("installed_plugins.json");
    let registry = crate::installed_registry::InstalledPluginsRegistry::load(&registry_path);

    let live_paths: HashSet<PathBuf> = registry
        .plugins
        .values()
        .flatten()
        .filter_map(|entry| entry.install_path.canonicalize().ok())
        .collect();

    let mut removed = 0;
    let now = std::time::SystemTime::now();

    // Walk marketplace directories
    let marketplace_entries = match std::fs::read_dir(&cache_root) {
        Ok(entries) => entries,
        Err(e) => {
            return Err(CacheSnafu {
                path: cache_root,
                message: format!("Failed to read cache directory: {e}"),
            }
            .build());
        }
    };

    for market_entry in marketplace_entries.flatten() {
        if !market_entry
            .file_type()
            .map(|t| t.is_dir())
            .unwrap_or(false)
        {
            continue;
        }
        let market_dir = market_entry.path();

        // Walk plugin directories within marketplace
        let plugin_entries = match std::fs::read_dir(&market_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for plugin_entry in plugin_entries.flatten() {
            if !plugin_entry
                .file_type()
                .map(|t| t.is_dir())
                .unwrap_or(false)
            {
                continue;
            }
            let plugin_dir = plugin_entry.path();

            // Walk version directories within plugin
            let version_entries = match std::fs::read_dir(&plugin_dir) {
                Ok(entries) => entries,
                Err(_) => continue,
            };

            for version_entry in version_entries.flatten() {
                if !version_entry
                    .file_type()
                    .map(|t| t.is_dir())
                    .unwrap_or(false)
                {
                    continue;
                }
                let version_dir = version_entry.path();

                // Check if this version directory is in the live set
                let canonical = version_dir
                    .canonicalize()
                    .unwrap_or_else(|_| version_dir.clone());
                if live_paths.contains(&canonical) {
                    continue;
                }

                // Check grace period using directory modification time
                let is_expired = version_dir
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .map(|modified| {
                        now.duration_since(modified).unwrap_or_default() >= grace_period
                    })
                    .unwrap_or(true);

                if is_expired {
                    debug!(
                        path = %version_dir.display(),
                        "Removing orphaned cache entry"
                    );
                    if std::fs::remove_dir_all(&version_dir).is_ok() {
                        removed += 1;
                    }
                }
            }

            // Clean up empty plugin directory
            if is_dir_empty(&plugin_dir) {
                let _ = std::fs::remove_dir(&plugin_dir);
            }
        }

        // Clean up empty marketplace directory
        if is_dir_empty(&market_dir) {
            let _ = std::fs::remove_dir(&market_dir);
        }
    }

    if removed > 0 {
        debug!(removed, "Cleaned up orphaned cache entries");
    }

    Ok(removed)
}

/// Default grace period for orphaned cache entries (7 days).
pub const DEFAULT_CACHE_GRACE_PERIOD: std::time::Duration =
    std::time::Duration::from_secs(7 * 24 * 60 * 60);

#[cfg(test)]
#[path = "cache.test.rs"]
mod tests;
