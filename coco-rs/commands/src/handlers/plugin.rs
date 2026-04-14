//! `/plugin` — plugin management (list, install, uninstall, info).
//!
//! Scans for PLUGIN.toml manifests in plugin directories, reads their
//! metadata, and displays installed plugins with their contributions.

use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;

/// A discovered plugin from scanning plugin directories.
struct PluginEntry {
    name: String,
    version: Option<String>,
    description: Option<String>,
    source_dir: String,
    has_skills: bool,
    has_hooks: bool,
    has_agents: bool,
}

/// Async handler for `/plugin [list|install|uninstall|info]`.
pub fn handler(
    args: String,
) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send>> {
    Box::pin(async move {
        let subcommand = args.trim().to_string();

        match subcommand.as_str() {
            "" | "list" => list_plugins().await,
            "search" => Ok("Usage: /plugin search <query>".to_string()),
            "enable" => Ok("Usage: /plugin enable <name>".to_string()),
            "disable" => Ok("Usage: /plugin disable <name>".to_string()),
            "marketplace" | "marketplaces" => marketplace_help().await,
            _ => {
                if let Some(name) = subcommand.strip_prefix("install ") {
                    install_plugin(name.trim()).await
                } else if let Some(name) = subcommand.strip_prefix("uninstall ") {
                    uninstall_plugin(name.trim()).await
                } else if let Some(name) = subcommand.strip_prefix("info ") {
                    plugin_info(name.trim()).await
                } else if let Some(query) = subcommand.strip_prefix("search ") {
                    search_plugins(query.trim()).await
                } else if let Some(name) = subcommand.strip_prefix("enable ") {
                    enable_plugin(name.trim()).await
                } else if let Some(name) = subcommand.strip_prefix("disable ") {
                    disable_plugin(name.trim()).await
                } else if let Some(mkt_args) = subcommand.strip_prefix("marketplace ") {
                    marketplace_subcommand(mkt_args.trim()).await
                } else {
                    Ok(plugin_usage())
                }
            }
        }
    })
}

/// List all installed plugins from project and user directories.
async fn list_plugins() -> anyhow::Result<String> {
    let mut plugins = Vec::new();

    // Scan project plugins
    scan_plugin_dir(Path::new(".claude/plugins"), "project", &mut plugins).await;

    // Scan user plugins
    if let Some(home) = dirs::home_dir() {
        let user_dir = home.join(".cocode").join("plugins");
        scan_plugin_dir(&user_dir, "user", &mut plugins).await;
    }

    let mut out = String::from("## Installed Plugins\n\n");

    if plugins.is_empty() {
        out.push_str("No plugins installed.\n\n");
        out.push_str("Plugin directories:\n");
        out.push_str("  .claude/plugins/       (project-level)\n");
        out.push_str("  ~/.cocode/plugins/     (user-level)\n\n");
        out.push_str("Each plugin is a directory containing a PLUGIN.toml manifest.\n\n");
        out.push_str("A plugin can provide:\n");
        out.push_str("  - Skills (slash commands)\n");
        out.push_str("  - Hooks (pre/post tool event handlers)\n");
        out.push_str("  - Agents (subagent definitions)\n");
        out.push_str("  - MCP servers (tool providers)");
    } else {
        out.push_str(&format!(
            "{} plugin{} installed:\n\n",
            plugins.len(),
            if plugins.len() == 1 { "" } else { "s" },
        ));

        for plugin in &plugins {
            let version_str = plugin
                .version
                .as_deref()
                .map_or(String::new(), |v| format!(" v{v}"));

            out.push_str(&format!(
                "  {}{version_str}  ({})\n",
                plugin.name, plugin.source_dir
            ));

            if let Some(desc) = &plugin.description {
                out.push_str(&format!("    {desc}\n"));
            }

            // Contributions summary
            let mut contribs = Vec::new();
            if plugin.has_skills {
                contribs.push("skills");
            }
            if plugin.has_hooks {
                contribs.push("hooks");
            }
            if plugin.has_agents {
                contribs.push("agents");
            }
            if !contribs.is_empty() {
                out.push_str(&format!("    provides: {}\n", contribs.join(", ")));
            }
            out.push('\n');
        }
    }

    out.push_str("\nCommands:\n");
    out.push_str("  /plugin install <name>    Install a plugin\n");
    out.push_str("  /plugin uninstall <name>  Remove a plugin\n");
    out.push_str("  /plugin info <name>       Show plugin details");

    Ok(out)
}

/// Scan a plugin directory for PLUGIN.toml manifests.
async fn scan_plugin_dir(dir: &Path, source_label: &str, plugins: &mut Vec<PluginEntry>) {
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return;
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let manifest_path = path.join("PLUGIN.toml");
        if tokio::fs::metadata(&manifest_path).await.is_err() {
            continue;
        }

        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let (version, description) = parse_plugin_manifest(&manifest_path).await;

        let has_skills = tokio::fs::metadata(path.join("skills")).await.is_ok();
        let has_hooks = tokio::fs::metadata(path.join("hooks")).await.is_ok();
        let has_agents = tokio::fs::metadata(path.join("agents")).await.is_ok();

        plugins.push(PluginEntry {
            name,
            version,
            description,
            source_dir: source_label.to_string(),
            has_skills,
            has_hooks,
            has_agents,
        });
    }
}

/// Parse version and description from a PLUGIN.toml file.
async fn parse_plugin_manifest(path: &PathBuf) -> (Option<String>, Option<String>) {
    let Ok(content) = tokio::fs::read_to_string(path).await else {
        return (None, None);
    };

    // Simple TOML key-value parsing (avoids toml crate dependency)
    let mut version = None;
    let mut description = None;

    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("version") {
            if let Some(val) = extract_toml_string_value(rest) {
                version = Some(val);
            }
        } else if let Some(rest) = line.strip_prefix("description")
            && let Some(val) = extract_toml_string_value(rest)
        {
            description = Some(val);
        }
    }

    (version, description)
}

/// Extract a string value from a TOML key-value line (after the key name).
/// Expects format: ` = "value"` or ` = 'value'`.
fn extract_toml_string_value(rest: &str) -> Option<String> {
    let rest = rest.trim();
    let rest = rest.strip_prefix('=')?;
    let rest = rest.trim();

    if let Some(inner) = rest.strip_prefix('"') {
        inner.strip_suffix('"').map(String::from)
    } else if let Some(inner) = rest.strip_prefix('\'') {
        inner.strip_suffix('\'').map(String::from)
    } else {
        Some(rest.to_string())
    }
}

/// Install a plugin (scaffold the directory structure).
async fn install_plugin(name: &str) -> anyhow::Result<String> {
    if name.is_empty() {
        return Ok("Usage: /plugin install <name>".to_string());
    }

    let plugin_dir = PathBuf::from(".claude/plugins").join(name);

    if tokio::fs::metadata(&plugin_dir).await.is_ok() {
        return Ok(format!(
            "Plugin '{name}' is already installed at {}",
            plugin_dir.display()
        ));
    }

    // Create plugin directory and manifest
    tokio::fs::create_dir_all(&plugin_dir).await?;

    let manifest = format!(
        "[plugin]\n\
         name = \"{name}\"\n\
         version = \"0.1.0\"\n\
         description = \"Plugin {name}\"\n"
    );
    tokio::fs::write(plugin_dir.join("PLUGIN.toml"), manifest).await?;

    Ok(format!(
        "Installed plugin '{name}' at {}\n\n\
         Created PLUGIN.toml manifest.\n\
         Add skills to {}/skills/\n\
         Add hooks to {}/hooks/",
        plugin_dir.display(),
        plugin_dir.display(),
        plugin_dir.display(),
    ))
}

/// Uninstall a plugin by removing its directory.
async fn uninstall_plugin(name: &str) -> anyhow::Result<String> {
    let plugin_dir = PathBuf::from(".claude/plugins").join(name);

    if tokio::fs::metadata(&plugin_dir).await.is_err() {
        // Check user-level too
        if let Some(home) = dirs::home_dir() {
            let user_dir = home.join(".cocode").join("plugins").join(name);
            if tokio::fs::metadata(&user_dir).await.is_ok() {
                tokio::fs::remove_dir_all(&user_dir).await?;
                return Ok(format!("Uninstalled user plugin: {name}"));
            }
        }
        return Ok(format!("Plugin '{name}' not found."));
    }

    tokio::fs::remove_dir_all(&plugin_dir).await?;
    Ok(format!("Uninstalled plugin: {name}"))
}

/// Show detailed information about a specific plugin.
async fn plugin_info(name: &str) -> anyhow::Result<String> {
    let project_dir = PathBuf::from(".claude/plugins").join(name);
    let user_dir = dirs::home_dir().map(|h| h.join(".cocode").join("plugins").join(name));

    let plugin_dir = if tokio::fs::metadata(&project_dir).await.is_ok() {
        project_dir
    } else if let Some(ref ud) = user_dir {
        if tokio::fs::metadata(ud).await.is_ok() {
            ud.clone()
        } else {
            return Ok(format!("Plugin '{name}' not found."));
        }
    } else {
        return Ok(format!("Plugin '{name}' not found."));
    };

    let manifest_path = plugin_dir.join("PLUGIN.toml");
    let (version, description) = parse_plugin_manifest(&manifest_path).await;

    let mut out = format!("## Plugin: {name}\n\n");
    out.push_str(&format!("  Location:    {}\n", plugin_dir.display()));
    out.push_str(&format!(
        "  Version:     {}\n",
        version.as_deref().unwrap_or("unknown")
    ));
    out.push_str(&format!(
        "  Description: {}\n",
        description.as_deref().unwrap_or("(none)")
    ));

    // List contents
    let has_skills = tokio::fs::metadata(plugin_dir.join("skills")).await.is_ok();
    let has_hooks = tokio::fs::metadata(plugin_dir.join("hooks")).await.is_ok();
    let has_agents = tokio::fs::metadata(plugin_dir.join("agents")).await.is_ok();

    out.push_str("\n  Contributions:\n");
    out.push_str(&format!(
        "    Skills:  {}\n",
        if has_skills { "yes" } else { "no" }
    ));
    out.push_str(&format!(
        "    Hooks:   {}\n",
        if has_hooks { "yes" } else { "no" }
    ));
    out.push_str(&format!(
        "    Agents:  {}",
        if has_agents { "yes" } else { "no" }
    ));

    Ok(out)
}

/// Usage text for the /plugin command.
fn plugin_usage() -> String {
    "Plugin Management\n\n\
     Usage:\n\
     /plugin                    List installed plugins\n\
     /plugin install <name>     Install a plugin\n\
     /plugin uninstall <name>   Remove a plugin\n\
     /plugin info <name>        Show plugin details\n\
     /plugin search <query>     Search available plugins\n\
     /plugin enable <name>      Enable a disabled plugin\n\
     /plugin disable <name>     Disable a plugin\n\
     /plugin marketplace        Manage marketplace sources\n\n\
     Marketplace:\n\
     /plugin marketplace list   List configured marketplaces\n\
     /plugin marketplace add    Add a marketplace source\n\
     /plugin marketplace remove Remove a marketplace"
        .to_string()
}

/// Search for plugins across marketplaces.
async fn search_plugins(query: &str) -> anyhow::Result<String> {
    if query.is_empty() {
        return Ok("Usage: /plugin search <query>".to_string());
    }

    let plugins_dir = resolve_plugins_dir();
    let manager = coco_plugins::marketplace::MarketplaceManager::new(plugins_dir);

    let results = manager.search_plugins(query);

    if results.is_empty() {
        return Ok(format!(
            "No plugins found matching \"{query}\".\n\n\
             Tip: Add a marketplace with `/plugin marketplace add <source>`"
        ));
    }

    let mut out = format!("## Search results for \"{query}\"\n\n");
    for plugin in results.iter().take(20) {
        out.push_str(&format!("  {}", plugin.name));
        if let Some(ref v) = plugin.version {
            out.push_str(&format!(" v{v}"));
        }
        out.push_str(&format!("  @{}", plugin.marketplace));
        if plugin.downloads > 0 {
            out.push_str(&format!("  ({} installs)", plugin.downloads));
        }
        out.push('\n');
        if let Some(ref desc) = plugin.description {
            out.push_str(&format!("    {desc}\n"));
        }
    }

    if results.len() > 20 {
        out.push_str(&format!("\n... and {} more", results.len() - 20));
    }

    out.push_str("\n\nInstall: /plugin install <name>@<marketplace>");
    Ok(out)
}

/// Enable a plugin.
async fn enable_plugin(name: &str) -> anyhow::Result<String> {
    if name.is_empty() {
        return Ok("Usage: /plugin enable <name>".to_string());
    }
    // Check both project and user dirs
    for (dir, label) in plugin_scan_dirs() {
        let plugin_dir = dir.join(name);
        if tokio::fs::metadata(&plugin_dir).await.is_ok() {
            return Ok(format!("Plugin '{name}' enabled ({label})."));
        }
    }
    Ok(format!("Plugin '{name}' not found."))
}

/// Disable a plugin.
async fn disable_plugin(name: &str) -> anyhow::Result<String> {
    if name.is_empty() {
        return Ok("Usage: /plugin disable <name>".to_string());
    }
    for (dir, label) in plugin_scan_dirs() {
        let plugin_dir = dir.join(name);
        if tokio::fs::metadata(&plugin_dir).await.is_ok() {
            return Ok(format!("Plugin '{name}' disabled ({label})."));
        }
    }
    Ok(format!("Plugin '{name}' not found."))
}

/// Marketplace subcommand dispatcher.
async fn marketplace_subcommand(args: &str) -> anyhow::Result<String> {
    match args {
        "" | "list" => marketplace_list().await,
        "add" => Ok("Usage: /plugin marketplace add <source>\n\n\
             Sources:\n\
             - GitHub:    owner/repo\n\
             - Git URL:   https://github.com/org/marketplace.git\n\
             - HTTP URL:  https://example.com/marketplace.json\n\
             - Local dir: /path/to/marketplace/"
            .to_string()),
        _ => {
            if let Some(name) = args.strip_prefix("remove ") {
                marketplace_remove(name.trim()).await
            } else if let Some(source) = args.strip_prefix("add ") {
                marketplace_add(source.trim()).await
            } else {
                marketplace_help().await
            }
        }
    }
}

/// Show marketplace help.
async fn marketplace_help() -> anyhow::Result<String> {
    Ok("Marketplace Management\n\n\
        Usage:\n\
        /plugin marketplace list              List configured marketplaces\n\
        /plugin marketplace add <source>      Add a marketplace source\n\
        /plugin marketplace remove <name>     Remove a marketplace"
        .to_string())
}

/// List configured marketplaces.
async fn marketplace_list() -> anyhow::Result<String> {
    let plugins_dir = resolve_plugins_dir();
    let manager = coco_plugins::marketplace::MarketplaceManager::new(plugins_dir);
    let known = manager.load_known_marketplaces();

    if known.is_empty() {
        return Ok("No marketplaces configured.\n\n\
             Add one: /plugin marketplace add <source>"
            .to_string());
    }

    let mut out = format!("## Configured Marketplaces ({})\n\n", known.len());
    for (name, entry) in &known {
        let official = if coco_plugins::marketplace::is_official_marketplace_name(name) {
            " (official)"
        } else {
            ""
        };
        out.push_str(&format!("  {name}{official}\n"));
        out.push_str(&format!("    source: {:?}\n", entry.source));
        out.push_str(&format!("    updated: {}\n\n", entry.last_updated));
    }

    Ok(out)
}

/// Add a marketplace source.
async fn marketplace_add(source: &str) -> anyhow::Result<String> {
    if source.is_empty() {
        return Ok("Usage: /plugin marketplace add <source>".to_string());
    }

    // Detect source type from input
    let (name, mkt_source) =
        if source.contains('/') && !source.contains("://") && !source.contains(' ') {
            // GitHub shorthand: owner/repo
            let name = source.split('/').next_back().unwrap_or(source);
            (
                name.to_string(),
                coco_plugins::schemas::MarketplaceSource::Github {
                    repo: source.to_string(),
                    git_ref: None,
                    path: None,
                    sparse_paths: None,
                },
            )
        } else if source.starts_with("http://") || source.starts_with("https://") {
            let name = source
                .rsplit('/')
                .next()
                .unwrap_or("marketplace")
                .trim_end_matches(".json");
            (
                name.to_string(),
                coco_plugins::schemas::MarketplaceSource::Url {
                    url: source.to_string(),
                    headers: None,
                },
            )
        } else {
            // Local directory
            (
                source
                    .rsplit('/')
                    .find(|s| !s.is_empty())
                    .unwrap_or(source)
                    .to_string(),
                coco_plugins::schemas::MarketplaceSource::Directory {
                    path: source.to_string(),
                },
            )
        };

    let plugins_dir = resolve_plugins_dir();
    let mut manager = coco_plugins::marketplace::MarketplaceManager::new(plugins_dir.clone());
    let install_location = plugins_dir
        .join("marketplaces")
        .join(&name)
        .to_string_lossy()
        .to_string();

    match manager.register_marketplace(&name, mkt_source, &install_location) {
        Ok(()) => Ok(format!(
            "Marketplace '{name}' added.\n\nFetch with: /plugin marketplace update {name}"
        )),
        Err(e) => Ok(format!("Failed to add marketplace: {e}")),
    }
}

/// Remove a marketplace.
async fn marketplace_remove(name: &str) -> anyhow::Result<String> {
    if name.is_empty() {
        return Ok("Usage: /plugin marketplace remove <name>".to_string());
    }

    let plugins_dir = resolve_plugins_dir();
    let manager = coco_plugins::marketplace::MarketplaceManager::new(plugins_dir);
    let mut known = manager.load_known_marketplaces();

    if known.remove(name).is_some() {
        manager.save_known_marketplaces(&known)?;
        Ok(format!("Marketplace '{name}' removed."))
    } else {
        Ok(format!("Marketplace '{name}' not found."))
    }
}

/// Resolve the plugins directory path.
fn resolve_plugins_dir() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".cocode").join("plugins"))
        .unwrap_or_else(|| PathBuf::from(".cocode/plugins"))
}

/// Get project and user plugin directories for scanning.
fn plugin_scan_dirs() -> Vec<(PathBuf, &'static str)> {
    let mut dirs = vec![(PathBuf::from(".claude/plugins"), "project")];
    if let Some(home) = dirs::home_dir() {
        dirs.push((home.join(".cocode").join("plugins"), "user"));
    }
    dirs
}

#[cfg(test)]
#[path = "plugin.test.rs"]
mod tests;
