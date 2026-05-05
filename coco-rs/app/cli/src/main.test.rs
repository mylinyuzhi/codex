use std::collections::HashMap;

use coco_cli::headless::DEFAULT_SYSTEM_PROMPT_IDENTITY;
use coco_cli::headless::build_system_prompt_for_model;
use coco_config::CatalogPaths;
use coco_config::EnvSnapshot;
use coco_config::RuntimeConfig;
use coco_config::RuntimeOverrides;
use coco_config::Settings;
use coco_config::SettingsWithSource;
use tempfile::TempDir;

fn runtime_for_model(selection: &str, home: &TempDir) -> RuntimeConfig {
    let settings = SettingsWithSource {
        merged: Settings {
            model: Some(selection.to_string()),
            ..Default::default()
        },
        per_source: HashMap::new(),
    };
    coco_config::build_runtime_config_with(
        settings,
        EnvSnapshot::default(),
        RuntimeOverrides::default(),
        CatalogPaths::empty_in(home.path()),
    )
    .expect("runtime config")
}

#[test]
fn build_system_prompt_uses_model_instructions_when_present() {
    let home = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();
    let runtime = runtime_for_model("openai/gpt-5-4", &home);

    let prompt = build_system_prompt_for_model(cwd.path(), &runtime, "openai", "gpt-5-4");

    assert!(
        prompt.starts_with("You are Codex, a coding agent based on GPT-5."),
        "shared headless/SDK/TUI prompt builder should use model instructions"
    );
    assert!(prompt.contains("# Personality"));
    assert!(!prompt.starts_with(DEFAULT_SYSTEM_PROMPT_IDENTITY));
}

#[test]
fn build_system_prompt_falls_back_when_model_has_no_instructions() {
    let home = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();
    let runtime = runtime_for_model("anthropic/claude-sonnet-4-6", &home);

    let prompt =
        build_system_prompt_for_model(cwd.path(), &runtime, "anthropic", "claude-sonnet-4-6");

    assert!(prompt.starts_with(DEFAULT_SYSTEM_PROMPT_IDENTITY));
    assert!(prompt.contains("# Environment"));
}
