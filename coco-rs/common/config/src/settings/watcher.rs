//! Settings file watcher — reloads on file changes.
//!
//! TS: changeDetector.ts with 1000ms stability threshold.
//! Uses utils/file-watch wrapper (debounce, coalesce).

use std::path::Path;
use std::path::PathBuf;

use super::source::SettingSource;

/// Settings watcher — monitors config files for changes.
pub struct SettingsWatcher {
    watched_paths: Vec<(SettingSource, PathBuf)>,
    // TODO: Wire to utils/file-watch when integrating
}

impl SettingsWatcher {
    /// Create a new settings watcher for the given working directory.
    pub fn new(cwd: &Path) -> Self {
        let watched_paths = vec![
            (
                SettingSource::User,
                crate::global_config::user_settings_path(),
            ),
            (
                SettingSource::Project,
                crate::global_config::project_settings_path(cwd),
            ),
            (
                SettingSource::Local,
                crate::global_config::local_settings_path(cwd),
            ),
            (
                SettingSource::Policy,
                crate::global_config::managed_settings_path(),
            ),
        ];
        Self { watched_paths }
    }

    /// Get watched paths with their sources.
    pub fn watched_paths(&self) -> &[(SettingSource, PathBuf)] {
        &self.watched_paths
    }

    /// Determine which source a path belongs to.
    pub fn source_for_path(&self, path: &Path) -> Option<SettingSource> {
        self.watched_paths
            .iter()
            .find(|(_, p)| p == path)
            .map(|(s, _)| *s)
    }
}
