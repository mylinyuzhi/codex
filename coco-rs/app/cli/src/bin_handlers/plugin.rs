//! `coco plugin <action>` — local plugin install/uninstall/list/validate.
//!
//! TS: `src/cli/handlers/plugins.ts` — full handler is ~878 lines covering
//! marketplace integration, scopes, lockfiles. Rust currently implements the
//! local-disk subset: list, install-from-path, uninstall, validate.
//! URL/marketplace installs require porting the marketplace module.

use anyhow::Result;

use coco_cli::PluginAction;
use coco_config::global_config;

pub async fn run_plugin_subcommand(action: &PluginAction) -> Result<()> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let config_home = global_config::config_home();
    let plugin_dirs = coco_plugins::get_plugin_dirs(&config_home, &cwd);

    match action {
        PluginAction::List => {
            let mut manager = coco_plugins::PluginManager::new();
            manager.load_from_dirs(&plugin_dirs);
            if manager.is_empty() {
                println!("No plugins installed.");
                return Ok(());
            }
            println!("Installed plugins:");
            let mut plugins: Vec<_> = manager.enabled();
            plugins.sort_by_key(|p| p.name.clone());
            for plugin in plugins {
                let version = plugin.manifest.version.as_deref().unwrap_or("—");
                let source = match &plugin.source {
                    coco_plugins::PluginSource::Builtin => "builtin".into(),
                    coco_plugins::PluginSource::User => "user".into(),
                    coco_plugins::PluginSource::Project => "project".into(),
                    coco_plugins::PluginSource::Repository { url } => format!("repo {url}"),
                };
                println!(
                    "  {name} {version} ({source})  — {desc}",
                    name = plugin.name,
                    desc = plugin.manifest.description,
                );
            }
            Ok(())
        }
        PluginAction::Install { name } => {
            let src = std::path::Path::new(name);
            if !src.is_dir() {
                anyhow::bail!(
                    "plugin source '{name}' is not a local directory; \
                     marketplace/URL installs are not yet implemented"
                );
            }
            if !src.join("PLUGIN.toml").is_file() {
                anyhow::bail!("'{name}' does not contain a PLUGIN.toml manifest");
            }
            let manifest = coco_plugins::load_plugin_manifest(&src.join("PLUGIN.toml"))?;
            // Reject manifest names that could traverse the install root.
            // `Path::join` treats "../" literally and does not escape the root on
            // disk, but a normalized `..` chain can still confuse audit tooling.
            if manifest.name.is_empty()
                || manifest.name.contains('/')
                || manifest.name.contains('\\')
                || manifest.name == ".."
                || manifest.name == "."
            {
                anyhow::bail!(
                    "plugin manifest name '{}' contains path separators or reserved \
                     component; refusing to install",
                    manifest.name
                );
            }
            let dest_root = config_home.join("plugins");
            std::fs::create_dir_all(&dest_root)?;
            let dest = dest_root.join(&manifest.name);
            if dest.exists() {
                anyhow::bail!(
                    "plugin '{}' already installed at {}; uninstall first",
                    manifest.name,
                    dest.display()
                );
            }
            copy_dir_recursive(src, &dest)?;
            println!("Installed plugin '{}' → {}", manifest.name, dest.display());
            Ok(())
        }
        PluginAction::Uninstall { name } => {
            let dest = config_home.join("plugins").join(name);
            if !dest.is_dir() {
                anyhow::bail!("plugin '{name}' is not installed at {}", dest.display());
            }
            std::fs::remove_dir_all(&dest)?;
            println!("Uninstalled plugin '{name}'");
            Ok(())
        }
        PluginAction::Validate { path } => {
            let path = std::path::Path::new(path);
            let manifest_path = if path.is_file() {
                path.to_path_buf()
            } else {
                path.join("PLUGIN.toml")
            };
            if !manifest_path.is_file() {
                anyhow::bail!("no PLUGIN.toml found at {}", manifest_path.display());
            }
            let manifest = coco_plugins::load_plugin_manifest(&manifest_path)?;
            println!(
                "✓ {} v{}",
                manifest.name,
                manifest.version.as_deref().unwrap_or("—")
            );
            println!("  {}", manifest.description);
            if !manifest.skills.is_empty() {
                println!("  skills: {}", manifest.skills.join(", "));
            }
            if !manifest.hooks.is_empty() {
                println!("  hooks: {} event(s)", manifest.hooks.len());
            }
            if !manifest.mcp_servers.is_empty() {
                println!("  mcp_servers: {}", manifest.mcp_servers.len());
            }
            Ok(())
        }
    }
}

/// Recursively copy `src` into `dst`. Used by plugin install.
///
/// Symlinks are skipped with a warning — following them lets a hostile plugin
/// exfiltrate host files (e.g. `~/.ssh/id_rsa`) into the install tree. Use
/// `symlink_metadata()` so the check doesn't follow; `file_type().is_dir()`
/// and `is_file()` otherwise follow by default.
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let meta = std::fs::symlink_metadata(&src_path)?;
        let ty = meta.file_type();
        if ty.is_symlink() {
            eprintln!(
                "warning: skipping symlink in plugin source: {}",
                src_path.display()
            );
            continue;
        }
        let dest_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else if ty.is_file() {
            std::fs::copy(&src_path, &dest_path)?;
        }
    }
    Ok(())
}
