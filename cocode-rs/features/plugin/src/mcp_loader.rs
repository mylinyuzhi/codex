//! MCP server loading from plugin directories.
//!
//! Loads mcp.json files from plugin-specified MCP server directories.

use std::path::Path;

use tracing::debug;
use tracing::warn;
use walkdir::WalkDir;

use crate::contribution::PluginContribution;
use crate::mcp::McpServerConfig;

/// MCP server manifest filename.
pub const MCP_JSON: &str = "mcp.json";

/// Recognized MCPB/DXT bundle extensions.
const BUNDLE_EXTENSIONS: &[&str] = &["mcpb", "dxt"];

/// Load MCP server configurations from a directory.
///
/// Scans the directory for mcp.json files and `.mcpb`/`.dxt` bundle files,
/// loading them into `PluginContribution::McpServer` variants.
///
/// # Arguments
/// * `dir` - Directory to scan for mcp.json files
/// * `plugin_name` - Name of the plugin providing these servers
///
/// # Example mcp.json format:
/// ```json
/// {
///   "name": "filesystem",
///   "description": "Provides file system access",
///   "auto_start": true,
///   "transport": {
///     "type": "stdio",
///     "command": "npx",
///     "args": ["-y", "@anthropic/mcp-server-filesystem"]
///   },
///   "env": {
///     "MCP_DEBUG": "true"
///   }
/// }
/// ```
pub fn load_mcp_servers_from_dir(dir: &Path, plugin_name: &str) -> Vec<PluginContribution> {
    if !dir.is_dir() {
        debug!(
            plugin = %plugin_name,
            path = %dir.display(),
            "MCP server path not found or not a directory"
        );
        return Vec::new();
    }

    let mut results = Vec::new();

    // Walk the directory looking for mcp.json files and bundle files
    for entry in WalkDir::new(dir)
        .max_depth(3)
        .follow_links(false)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        if entry.file_type().is_dir() {
            let mcp_path = entry.path().join(MCP_JSON);
            if mcp_path.is_file() {
                match load_mcp_server_from_file(&mcp_path, plugin_name) {
                    Ok(contrib) => results.push(contrib),
                    Err(e) => {
                        warn!(
                            plugin = %plugin_name,
                            path = %mcp_path.display(),
                            error = %e,
                            "Failed to load MCP server configuration"
                        );
                    }
                }
            }
        } else if entry.file_type().is_file() {
            // Check for .mcpb / .dxt bundle files
            if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                if BUNDLE_EXTENSIONS.contains(&ext) {
                    match load_mcp_bundle(entry.path(), plugin_name) {
                        Ok(contrib) => results.push(contrib),
                        Err(e) => {
                            warn!(
                                plugin = %plugin_name,
                                path = %entry.path().display(),
                                error = %e,
                                "Failed to load MCP bundle"
                            );
                        }
                    }
                }
            }
        }
    }

    debug!(
        plugin = %plugin_name,
        path = %dir.display(),
        count = results.len(),
        "Loaded MCP servers from plugin"
    );

    results
}

/// Load an MCP server configuration from a `.mcpb` or `.dxt` bundle file.
///
/// These are ZIP archives containing a `manifest.json` (or `mcp.json`) with
/// the MCP server configuration and optionally bundled server executables.
fn load_mcp_bundle(path: &Path, plugin_name: &str) -> anyhow::Result<PluginContribution> {
    // Extract the bundle to a temporary directory alongside the bundle file
    let bundle_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("bundle");
    let extract_dir = path
        .parent()
        .unwrap_or(path)
        .join(format!(".{bundle_name}-extracted"));

    // Only extract if not already extracted or bundle is newer
    let needs_extract = if extract_dir.exists() {
        let bundle_modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        let extract_modified = std::fs::metadata(&extract_dir)
            .and_then(|m| m.modified())
            .ok();
        match (bundle_modified, extract_modified) {
            (Some(b), Some(e)) => b > e,
            _ => true,
        }
    } else {
        true
    };

    if needs_extract {
        extract_zip_bundle(path, &extract_dir)?;
    }

    // Look for manifest.json or mcp.json in the extracted directory
    let manifest_path = if extract_dir.join("manifest.json").exists() {
        extract_dir.join("manifest.json")
    } else if extract_dir.join(MCP_JSON).exists() {
        extract_dir.join(MCP_JSON)
    } else {
        anyhow::bail!(
            "Bundle {} does not contain manifest.json or mcp.json",
            path.display()
        );
    };

    let content = std::fs::read_to_string(&manifest_path)?;
    let mut config: McpServerConfig = serde_json::from_str(&content)?;

    // Resolve variables using the extract directory as the plugin root
    config.resolve_variables(&extract_dir, None);

    debug!(
        plugin = %plugin_name,
        server = %config.name,
        bundle = %path.display(),
        "Loaded MCP server from bundle"
    );

    Ok(PluginContribution::McpServer {
        config,
        plugin_name: plugin_name.to_string(),
    })
}

/// Extract a ZIP bundle to a target directory.
fn extract_zip_bundle(bundle_path: &Path, target_dir: &Path) -> anyhow::Result<()> {
    use std::io::Read;

    debug!(
        bundle = %bundle_path.display(),
        target = %target_dir.display(),
        "Extracting MCP bundle"
    );

    // Clean target if it exists
    if target_dir.exists() {
        std::fs::remove_dir_all(target_dir)?;
    }
    std::fs::create_dir_all(target_dir)?;

    let file = std::fs::File::open(bundle_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let entry_path = match entry.enclosed_name() {
            Some(p) => p.to_path_buf(),
            None => continue, // Skip entries with unsafe paths
        };

        let out_path = target_dir.join(&entry_path);

        // Security: ensure path stays within target_dir
        if !out_path.starts_with(target_dir) {
            warn!(
                path = %entry_path.display(),
                "Skipping zip entry with path traversal"
            );
            continue;
        }

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            std::fs::write(&out_path, &buf)?;

            // Set executable bit on Unix for binary files
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if entry.unix_mode().is_some_and(|m| m & 0o111 != 0) {
                    let perms = std::fs::Permissions::from_mode(0o755);
                    std::fs::set_permissions(&out_path, perms)?;
                }
            }
        }
    }

    debug!(
        bundle = %bundle_path.display(),
        entries = archive.len(),
        "Extracted MCP bundle"
    );

    Ok(())
}

/// Load a single MCP server configuration from a JSON file.
///
/// Resolves variable substitution patterns (`${COCODE_PLUGIN_ROOT}`, `${env.VAR}`)
/// using the parent plugin directory as the plugin root.
fn load_mcp_server_from_file(path: &Path, plugin_name: &str) -> anyhow::Result<PluginContribution> {
    let content = std::fs::read_to_string(path)?;
    let mut config: McpServerConfig = serde_json::from_str(&content)?;

    // Resolve variables using the plugin directory as root.
    // The plugin root is the grandparent of the mcp.json file (plugin_dir/mcp/server/mcp.json).
    // Walk up to find the directory that contains plugin.json.
    let plugin_root =
        find_plugin_root(path).unwrap_or_else(|| path.parent().unwrap_or(path).to_path_buf());
    config.resolve_variables(&plugin_root, None);

    debug!(
        plugin = %plugin_name,
        server = %config.name,
        "Loaded MCP server configuration"
    );

    Ok(PluginContribution::McpServer {
        config,
        plugin_name: plugin_name.to_string(),
    })
}

/// Walk up from a path to find the nearest directory containing plugin.json
/// (either at root or in `.cocode-plugin/`).
fn find_plugin_root(from: &Path) -> Option<std::path::PathBuf> {
    let mut current = from.parent();
    while let Some(dir) = current {
        if dir.join("plugin.json").exists()
            || dir.join(".cocode-plugin").join("plugin.json").exists()
        {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

#[cfg(test)]
#[path = "mcp_loader.test.rs"]
mod tests;
