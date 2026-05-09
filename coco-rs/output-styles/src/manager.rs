//! [`OutputStyleManager`] — single-shot resolved catalog used by the CLI.
//!
//! The manager wraps an [`Aggregated`] catalog and the active
//! [`OutputStyleConfig`] (or `None` for the default sentinel). It is
//! constructed once at session bootstrap from settings + on-disk
//! markdown + enabled plugins, then passed (cheap to clone) into the
//! system-prompt builder, the SDK init bootstrap, and the per-turn
//! reminder pipeline.

use std::path::PathBuf;

use crate::catalog::OutputStyleConfig;
use crate::catalog::OutputStyleSource;
use crate::dir_loader::load_dir_styles;
use crate::plugin_loader::PluginOutputStyleSource;
use crate::plugin_loader::load_plugin_output_styles;
use crate::resolver::Aggregated;
use crate::resolver::ForceForPluginVerdict;
use crate::resolver::aggregate;
use crate::resolver::resolve_active_style;

/// CLI-facing builder for the resolved catalog.
#[derive(Debug, Default)]
pub struct OutputStyleManagerBuilder {
    settings_name: Option<String>,
    user_dir: Option<PathBuf>,
    project_dirs: Vec<PathBuf>,
    managed_dir: Option<PathBuf>,
    plugins: Vec<PluginOutputStyleSource>,
}

impl OutputStyleManagerBuilder {
    /// Settings value (`Settings.output_style`). Default `"default"` if
    /// `None`.
    pub fn settings_name(mut self, name: Option<String>) -> Self {
        self.settings_name = name;
        self
    }

    /// User-home output styles dir (`~/.claude/output-styles` or
    /// `~/.coco/output-styles`).
    pub fn user_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.user_dir = dir;
        self
    }

    /// Project-tree output styles dirs, ordered closest-to-cwd first.
    /// The CLI walks from `<cwd>` up to the git root (or home),
    /// collecting `.claude/output-styles` along the way.
    pub fn project_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.project_dirs = dirs;
        self
    }

    /// Managed/policy directory (`/etc/coco/.claude/output-styles`).
    pub fn managed_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.managed_dir = dir;
        self
    }

    /// Enabled plugin output-style sources.
    pub fn plugins(mut self, plugins: Vec<PluginOutputStyleSource>) -> Self {
        self.plugins = plugins;
        self
    }

    /// Build the manager, performing all I/O up front.
    pub fn build(self) -> OutputStyleManager {
        // Order matters — see `resolver::aggregate` doc.
        let user_styles = self
            .user_dir
            .as_deref()
            .map(|d| load_dir_styles(d, OutputStyleSource::UserSettings))
            .unwrap_or_default();

        let mut project_styles = Vec::new();
        for dir in &self.project_dirs {
            project_styles.extend(load_dir_styles(dir, OutputStyleSource::ProjectSettings));
        }

        let managed_styles = self
            .managed_dir
            .as_deref()
            .map(|d| load_dir_styles(d, OutputStyleSource::PolicySettings))
            .unwrap_or_default();

        let plugin_styles = load_plugin_output_styles(&self.plugins);

        let dir_groups = vec![user_styles, project_styles, managed_styles];
        let aggregated = aggregate(&dir_groups, &plugin_styles);

        let (active, verdict) = resolve_active_style(&aggregated, self.settings_name.as_deref());

        if let ForceForPluginVerdict::Selected { winner, competing } = &verdict {
            if !competing.is_empty() {
                tracing::warn!(
                    target: "coco_output_styles::manager",
                    winner = %winner,
                    competing = ?competing,
                    "multiple plugins set force-for-plugin; using the first alphabetically"
                );
            } else {
                tracing::debug!(
                    target: "coco_output_styles::manager",
                    winner = %winner,
                    "applying plugin-forced output style"
                );
            }
        }

        let active = active.cloned();
        OutputStyleManager {
            aggregated,
            active,
            verdict,
        }
    }
}

/// Resolved catalog + active style.
#[derive(Debug, Clone)]
pub struct OutputStyleManager {
    aggregated: Aggregated,
    active: Option<OutputStyleConfig>,
    verdict: ForceForPluginVerdict,
}

impl OutputStyleManager {
    /// Empty manager with no active style — useful as a default in
    /// tests and headless paths that don't yet read settings.
    pub fn empty() -> Self {
        Self {
            aggregated: Aggregated::default(),
            active: None,
            verdict: ForceForPluginVerdict::None,
        }
    }

    /// Builder for normal CLI construction.
    pub fn builder() -> OutputStyleManagerBuilder {
        OutputStyleManagerBuilder::default()
    }

    /// The active style, or `None` for the `default` sentinel /
    /// unknown name.
    pub fn active(&self) -> Option<&OutputStyleConfig> {
        self.active.as_ref()
    }

    /// All loaded style names (sorted) including built-ins. Never
    /// includes the `default` sentinel — callers that need it for the
    /// SDK `available_output_styles` field prepend it themselves to
    /// preserve TS wire shape.
    pub fn names(&self) -> Vec<String> {
        self.aggregated.names()
    }

    /// Underlying catalog. Useful for the picker / config UI.
    pub fn aggregated(&self) -> &Aggregated {
        &self.aggregated
    }

    /// Force-for-plugin outcome, for diagnostics.
    pub fn force_for_plugin_verdict(&self) -> &ForceForPluginVerdict {
        &self.verdict
    }

    /// Convenience: name to advertise on the SDK init message. Always
    /// non-empty: returns the resolved name when a force-for-plugin or
    /// settings match exists, otherwise the literal `default` sentinel.
    pub fn active_name_for_sdk(&self) -> String {
        self.active
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_else(|| crate::DEFAULT_OUTPUT_STYLE_NAME.to_string())
    }
}

#[cfg(test)]
#[path = "manager.test.rs"]
mod tests;
