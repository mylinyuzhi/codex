//! Output style catalog types — TS `OutputStyleConfig` mirror.
//!
//! TS source: `constants/outputStyles.ts:11-27`.

use serde::Deserialize;
use serde::Serialize;

/// A single output style definition.
///
/// Mirrors the TS `OutputStyleConfig` exactly, including the optional
/// `keep_coding_instructions` and plugin-only `force_for_plugin` flags.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OutputStyleConfig {
    /// Display name. Used both as the catalog key and shown in pickers
    /// and the per-turn reminder. For plugin styles this is namespaced
    /// as `pluginName:baseName`.
    pub name: String,

    /// Short description shown in `/config` and the picker.
    pub description: String,

    /// The system-prompt body injected when this style is active. For
    /// the sentinel `default` style this is empty (the TS catalog
    /// stores `null`; we keep an empty `OutputStyleConfig` consumers
    /// never pass into the prompt builder, so the prompt section stays
    /// absent when no style is active — see [`builtin::builtin_styles`]).
    pub prompt: String,

    /// Where the style was loaded from.
    pub source: OutputStyleSource,

    /// When `Some(true)`, the standard "Doing tasks" coding instructions
    /// stay on top of this style. When `Some(false)` or `None`, the
    /// style fully replaces them. TS frontmatter key:
    /// `keep-coding-instructions`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_coding_instructions: Option<bool>,

    /// Plugin-only: when `Some(true)`, the style is force-applied as
    /// long as the plugin is enabled, overriding `settings.output_style`.
    /// TS frontmatter key: `force-for-plugin`. Parsed-but-ignored on
    /// non-plugin sources (with a warning in the dir loader).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force_for_plugin: Option<bool>,
}

/// Where this output style came from.
///
/// TS source: the discriminator on `OutputStyleConfig.source`,
/// `'built-in' | 'plugin' | SettingSource` where `SettingSource` is one
/// of `policySettings` / `userSettings` / `projectSettings`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum OutputStyleSource {
    /// Shipped in the binary.
    BuiltIn,
    /// Loaded from a plugin.
    Plugin,
    /// Managed/policy directory (`/etc/coco/...`, organization-pushed).
    PolicySettings,
    /// User home `~/.coco/output-styles/`.
    UserSettings,
    /// Project `.coco/output-styles/`.
    ProjectSettings,
}

impl OutputStyleSource {
    /// Aggregation priority — higher value wins when names collide.
    ///
    /// TS `getAllOutputStyles` adds groups in order
    /// `[plugin, user, project, managed]` after built-ins, so a
    /// later-added group overwrites an earlier one. We translate that
    /// into a numeric priority for explicit ordering inside
    /// [`crate::resolver::aggregate`].
    pub const fn priority(self) -> u8 {
        match self {
            Self::BuiltIn => 0,
            Self::Plugin => 1,
            Self::UserSettings => 2,
            Self::ProjectSettings => 3,
            Self::PolicySettings => 4,
        }
    }
}
