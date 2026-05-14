use std::collections::HashMap;

use coco_config::SettingSource;
use coco_config::Settings;
use coco_config::SettingsWithSource;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;

fn settings_with_source(
    merged: Settings,
    per_source: HashMap<SettingSource, serde_json::Value>,
) -> SettingsWithSource {
    SettingsWithSource { merged, per_source }
}

fn raw_syntax_highlighting(disabled: bool) -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::from_iter([(
        SYNTAX_HIGHLIGHTING_DISABLED_KEY.to_string(),
        json!(disabled),
    )]))
}

#[test]
fn from_settings_with_sources_allows_user_owned_syntax_highlighting() {
    let mut per_source = HashMap::new();
    per_source.insert(SettingSource::User, raw_syntax_highlighting(true));
    let settings = settings_with_source(
        Settings {
            syntax_highlighting_disabled: true,
            ..Settings::default()
        },
        per_source,
    );

    let display = DisplaySettings::from_settings_with_sources(&settings);

    assert_eq!(display.syntax_highlighting, SyntaxHighlighting::Disabled);
    assert_eq!(
        display.syntax_highlighting_editability,
        DisplaySettingEditability::Editable
    );
}

#[test]
fn from_settings_with_sources_marks_higher_priority_syntax_highlighting_as_overridden() {
    let mut per_source = HashMap::new();
    per_source.insert(SettingSource::Project, raw_syntax_highlighting(true));
    per_source.insert(SettingSource::Local, raw_syntax_highlighting(false));
    let settings = settings_with_source(Settings::default(), per_source);

    let display = DisplaySettings::from_settings_with_sources(&settings);

    assert_eq!(display.syntax_highlighting, SyntaxHighlighting::Enabled);
    assert_eq!(
        display.syntax_highlighting_editability,
        DisplaySettingEditability::OverriddenBy(SettingSource::Local)
    );
}
