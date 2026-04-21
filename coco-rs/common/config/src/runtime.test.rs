use std::collections::HashMap;

use coco_types::ModelRole;
use coco_types::ModelSpec;
use coco_types::ProviderApi;
use pretty_assertions::assert_eq;

use super::*;
use crate::EnvKey;
use crate::EnvSnapshot;
use crate::ModelSelection;
use crate::ProviderConfig;
use crate::RuntimeOverrides;
use crate::Settings;
use crate::settings::SettingsWithSource;

fn settings_with(settings: Settings) -> SettingsWithSource {
    SettingsWithSource {
        merged: settings,
        per_source: HashMap::new(),
    }
}

fn model_spec(provider: &str, api: ProviderApi, model_id: &str) -> ModelSpec {
    ModelSpec {
        provider: provider.to_string(),
        api,
        model_id: model_id.to_string(),
        display_name: model_id.to_string(),
    }
}

fn model_selection(provider: &str, model_id: &str) -> ModelSelection {
    ModelSelection {
        provider: provider.to_string(),
        model_id: model_id.to_string(),
    }
}

#[test]
fn test_runtime_config_resolves_provider_overrides() {
    let mut providers = HashMap::new();
    providers.insert(
        "local".to_string(),
        ProviderConfig {
            api: ProviderApi::OpenaiCompat,
            env_key: "LOCAL_API_KEY".to_string(),
            base_url: "http://localhost:8080/v1".to_string(),
            default_model: Some("local-model".to_string()),
            ..Default::default()
        },
    );
    let settings = settings_with(Settings {
        model: Some("local/local-model".to_string()),
        providers,
        ..Default::default()
    });

    let runtime = build_runtime_config(
        settings,
        EnvSnapshot::default(),
        RuntimeOverrides::default(),
    )
    .expect("runtime config");

    assert_eq!(runtime.providers["local"].name, "local");
    let main = runtime
        .model_roles
        .get(ModelRole::Main)
        .expect("main model role");
    assert_eq!(main.provider, "local");
    assert_eq!(main.api, ProviderApi::OpenaiCompat);
    assert_eq!(main.model_id, "local-model");
}

#[test]
fn test_runtime_config_env_model_override_beats_json() {
    let settings = settings_with(Settings {
        model: Some("anthropic/json-model".to_string()),
        ..Default::default()
    });
    let env = EnvSnapshot::from_pairs([(EnvKey::CocoModel, "openai/gpt-5")]);

    let runtime =
        build_runtime_config(settings, env, RuntimeOverrides::default()).expect("runtime config");

    let main = runtime
        .model_roles
        .get(ModelRole::Main)
        .expect("main model role");
    assert_eq!(main.provider, "openai");
    assert_eq!(main.api, ProviderApi::Openai);
    assert_eq!(main.model_id, "gpt-5");
}

#[test]
fn test_runtime_config_resolves_structured_model_roles() {
    let settings = settings_with(Settings {
        models: crate::ModelSelectionSettings {
            main: Some(model_selection("openai", "gpt-5")),
            fast: Some(model_selection("google", "gemini-2.5-flash")),
            ..Default::default()
        },
        ..Default::default()
    });

    let runtime = build_runtime_config(
        settings,
        EnvSnapshot::default(),
        RuntimeOverrides::default(),
    )
    .expect("runtime config");

    let main = runtime
        .model_roles
        .get(ModelRole::Main)
        .expect("main model role");
    let fast = runtime
        .model_roles
        .get(ModelRole::Fast)
        .expect("fast model role");
    assert_eq!(main, &model_spec("openai", ProviderApi::Openai, "gpt-5"));
    assert_eq!(
        fast,
        &model_spec("google", ProviderApi::Gemini, "gemini-2.5-flash")
    );
}

#[test]
fn test_runtime_config_rejects_bare_env_model_override() {
    let settings = settings_with(Settings::default());
    let env = EnvSnapshot::from_pairs([(EnvKey::CocoModel, "gpt-5")]);

    let err = build_runtime_config(settings, env, RuntimeOverrides::default())
        .expect_err("bare model override should fail");

    assert!(
        err.to_string()
            .contains("must use explicit `provider/model_id` format")
    );
}
