//! Shared directory scanner for plugin resource loading.

use std::path::Path;

use tracing::debug;
use tracing::warn;
use walkdir::WalkDir;

/// Scan a plugin directory for JSON manifest files and load each one.
///
/// Walks `dir` up to 3 levels deep, looking for directories containing
/// `filename`. Each match is passed to `load_fn` for deserialization.
pub fn scan_plugin_dir<T, F>(
    dir: &Path,
    filename: &str,
    plugin_name: &str,
    resource_type: &str,
    load_fn: F,
) -> Vec<T>
where
    F: Fn(&Path, &str) -> anyhow::Result<T>,
{
    if !dir.is_dir() {
        debug!(
            plugin = %plugin_name,
            path = %dir.display(),
            "{resource_type} path not found or not a directory"
        );
        return Vec::new();
    }

    let mut results = Vec::new();

    for entry in WalkDir::new(dir)
        .max_depth(3)
        .follow_links(false)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        if entry.file_type().is_dir() {
            let manifest_path = entry.path().join(filename);
            if manifest_path.is_file() {
                match load_fn(&manifest_path, plugin_name) {
                    Ok(item) => results.push(item),
                    Err(e) => {
                        warn!(
                            plugin = %plugin_name,
                            path = %manifest_path.display(),
                            error = %e,
                            "Failed to load {resource_type} definition"
                        );
                    }
                }
            }
        }
    }

    debug!(
        plugin = %plugin_name,
        path = %dir.display(),
        count = results.len(),
        "Loaded {resource_type} from plugin"
    );

    results
}
