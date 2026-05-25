//! TUI display preferences derived from `settings.json`.

use coco_config::SettingSource;
use coco_config::SettingsWithSource;
use coco_config::settings::SYNTAX_HIGHLIGHTING_DISABLED_KEY;

/// Whether language-level syntax highlighting is applied inside fenced code
/// blocks. Diff add/remove colors and other semantic highlights are separate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyntaxHighlighting {
    #[default]
    Enabled,
    Disabled,
}

impl SyntaxHighlighting {
    pub fn from_disabled(disabled: bool) -> Self {
        if disabled {
            Self::Disabled
        } else {
            Self::Enabled
        }
    }

    pub fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }

    pub fn is_disabled(self) -> bool {
        matches!(self, Self::Disabled)
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::Enabled => Self::Disabled,
            Self::Disabled => Self::Enabled,
        }
    }
}

/// Whether a display preference can be edited from the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisplaySettingEditability {
    #[default]
    Editable,
    OverriddenBy(SettingSource),
}

impl DisplaySettingEditability {
    pub fn is_editable(self) -> bool {
        matches!(self, Self::Editable)
    }

    pub fn overriding_source(self) -> Option<SettingSource> {
        match self {
            Self::Editable => None,
            Self::OverriddenBy(source) => Some(source),
        }
    }
}

/// Display-only preferences consumed by TUI renderers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DisplaySettings {
    pub syntax_highlighting: SyntaxHighlighting,
    pub syntax_highlighting_editability: DisplaySettingEditability,
    pub show_thinking: bool,
    pub copy_full_response: bool,
}

impl DisplaySettings {
    pub fn from_settings(settings: &coco_config::Settings) -> Self {
        Self {
            syntax_highlighting: SyntaxHighlighting::from_disabled(
                settings.syntax_highlighting_disabled,
            ),
            syntax_highlighting_editability: DisplaySettingEditability::Editable,
            show_thinking: settings.show_thinking,
            copy_full_response: settings.copy_full_response,
        }
    }

    pub fn from_settings_with_sources(settings: &SettingsWithSource) -> Self {
        Self {
            syntax_highlighting: SyntaxHighlighting::from_disabled(
                settings.merged.syntax_highlighting_disabled,
            ),
            syntax_highlighting_editability: syntax_highlighting_editability(settings),
            show_thinking: settings.merged.show_thinking,
            copy_full_response: settings.merged.copy_full_response,
        }
    }

    pub fn from_runtime_config(config: &coco_config::RuntimeConfig) -> Self {
        Self::from_settings_with_sources(&config.settings)
    }

    pub fn with_syntax_highlighting(self, syntax_highlighting: SyntaxHighlighting) -> Self {
        Self {
            syntax_highlighting,
            ..self
        }
    }

    pub fn with_copy_full_response(self, copy_full_response: bool) -> Self {
        Self {
            copy_full_response,
            ..self
        }
    }
}

fn syntax_highlighting_editability(settings: &SettingsWithSource) -> DisplaySettingEditability {
    settings
        .per_source
        .iter()
        .filter_map(|(source, value)| {
            if *source > SettingSource::User
                && value_contains_dotted_key(value, SYNTAX_HIGHLIGHTING_DISABLED_KEY)
            {
                Some(*source)
            } else {
                None
            }
        })
        .max()
        .map(DisplaySettingEditability::OverriddenBy)
        .unwrap_or_default()
}

fn value_contains_dotted_key(value: &serde_json::Value, key: &str) -> bool {
    let mut current = value;
    let mut parts = key.split('.').peekable();
    while let Some(part) = parts.next() {
        let Some(next) = current.get(part) else {
            return false;
        };
        if parts.peek().is_none() {
            return true;
        }
        current = next;
    }
    false
}

#[cfg(test)]
#[path = "display_settings.test.rs"]
mod tests;
