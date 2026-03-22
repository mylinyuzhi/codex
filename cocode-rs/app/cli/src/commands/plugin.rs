//! Plugin management CLI commands.
//!
//! Provides non-interactive plugin install, uninstall, enable, disable,
//! update, list, and validate operations.

use std::path::Path;

use cocode_config::ConfigManager;
use cocode_plugin::PluginInstaller;
use cocode_plugin::PluginScope;
use cocode_plugin::plugins_dir;

use crate::PluginAction;

/// Parse a scope string into a PluginScope.
fn parse_scope(s: &str) -> anyhow::Result<PluginScope> {
    match s {
        "user" => Ok(PluginScope::User),
        "project" => Ok(PluginScope::Project),
        "local" => Ok(PluginScope::Local),
        "managed" => Ok(PluginScope::Managed),
        _ => anyhow::bail!("Invalid scope: {s}. Valid scopes: user, project, local, managed"),
    }
}

/// Run a plugin management command.
pub async fn run(action: PluginAction, _config: &ConfigManager) -> anyhow::Result<()> {
    let cocode_home = cocode_config::find_cocode_home();
    let plugins = plugins_dir(&cocode_home);

    match action {
        PluginAction::Install { plugin_id, scope } => {
            let scope = parse_scope(&scope)?;
            let installer = PluginInstaller::new(plugins);
            match installer.install(&plugin_id, scope).await {
                Ok(result) => {
                    println!(
                        "Installed {} v{} at {}",
                        result.plugin_id,
                        result.version,
                        result.install_path.display()
                    );
                }
                Err(e) => {
                    anyhow::bail!("Failed to install plugin: {e}");
                }
            }
        }

        PluginAction::Uninstall { plugin_id, scope } => {
            let scope = parse_scope(&scope)?;
            let installer = PluginInstaller::new(plugins);
            installer.uninstall(&plugin_id, scope).await?;
            println!("Uninstalled {plugin_id}");
        }

        PluginAction::Enable { plugin_id } => {
            let settings_path = plugins.join("settings.json");
            let mut settings = cocode_plugin::PluginSettings::load(&settings_path);
            settings.set_enabled(&plugin_id, true);
            settings.save(&settings_path)?;
            println!("Enabled {plugin_id}");
        }

        PluginAction::Disable { plugin_id } => {
            let settings_path = plugins.join("settings.json");
            let mut settings = cocode_plugin::PluginSettings::load(&settings_path);
            settings.set_enabled(&plugin_id, false);
            settings.save(&settings_path)?;
            println!("Disabled {plugin_id}");
        }

        PluginAction::Update { plugin_id, scope } => {
            let scope = parse_scope(&scope)?;
            let installer = PluginInstaller::new(plugins);

            if plugin_id == "all" {
                let installed = installer.list_installed();
                if installed.is_empty() {
                    println!("No plugins installed.");
                    return Ok(());
                }
                let mut updated = 0;
                for info in &installed {
                    match installer.update(&info.id, scope).await {
                        Ok(result) => {
                            println!("Updated {} to v{}", result.plugin_id, result.version);
                            updated += 1;
                        }
                        Err(e) => {
                            eprintln!("Failed to update {}: {e}", info.id);
                        }
                    }
                }
                println!("{updated}/{} plugins updated.", installed.len());
            } else {
                let result = installer.update(&plugin_id, scope).await?;
                println!(
                    "Updated {} to v{} at {}",
                    result.plugin_id,
                    result.version,
                    result.install_path.display()
                );
            }
        }

        PluginAction::List => {
            let installer = PluginInstaller::new(plugins);
            let installed = installer.list_installed();
            if installed.is_empty() {
                println!("No plugins installed.");
            } else {
                println!("Installed plugins ({}):\n", installed.len());
                for info in &installed {
                    let status = if info.enabled { "enabled" } else { "disabled" };
                    println!(
                        "  {} v{} [{}] ({})",
                        info.id, info.version, info.scope, status
                    );
                }
            }
        }

        PluginAction::Validate { path } => {
            validate_plugin(&path)?;
        }
    }

    Ok(())
}

/// Validate a plugin directory structure.
fn validate_plugin(path: &Path) -> anyhow::Result<()> {
    let loader = cocode_plugin::PluginLoader::new();

    // Try to load the plugin
    let settings = cocode_plugin::PluginSettings::default();
    match loader.load(path, PluginScope::Flag, &settings) {
        Ok(plugin) => {
            println!("Plugin validation passed!\n");
            println!("  Name: {}", plugin.manifest.plugin.name);
            println!("  Version: {}", plugin.manifest.plugin.version);
            println!("  Description: {}", plugin.manifest.plugin.description);
            println!("  Contributions: {} items", plugin.contributions.len());

            let (mut skills, mut hooks, mut agents, mut mcp, mut lsp) = (0, 0, 0, 0, 0);
            for c in &plugin.contributions {
                if c.is_skill() {
                    skills += 1;
                } else if c.is_hook() {
                    hooks += 1;
                } else if c.is_agent() {
                    agents += 1;
                } else if c.is_mcp_server() {
                    mcp += 1;
                } else if c.is_lsp_server() {
                    lsp += 1;
                }
            }

            if skills > 0 {
                println!("  Skills: {skills}");
            }
            if hooks > 0 {
                println!("  Hooks: {hooks}");
            }
            if agents > 0 {
                println!("  Agents: {agents}");
            }
            if mcp > 0 {
                println!("  MCP servers: {mcp}");
            }
            if lsp > 0 {
                println!("  LSP servers: {lsp}");
            }
        }
        Err(e) => {
            anyhow::bail!("Plugin validation failed: {e}");
        }
    }

    Ok(())
}
