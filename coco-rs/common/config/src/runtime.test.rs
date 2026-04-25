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

fn role_slots_of(provider: &str, model_id: &str) -> crate::RoleSlots<ModelSelection> {
    crate::RoleSlots::new(model_selection(provider, model_id))
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
            main: Some(role_slots_of("openai", "gpt-5")),
            fast: Some(role_slots_of("google", "gemini-2.5-flash")),
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

#[test]
fn test_cli_fallback_model_overrides_populate_main_chain_in_order() {
    // `--fallback-model X --fallback-model Y` produces an ordered
    // Main role fallback chain. Flag order = chain priority.
    let settings = settings_with(Settings::default());
    let env = EnvSnapshot::default();
    let overrides = RuntimeOverrides {
        fallback_model_overrides: vec![
            "anthropic/claude-sonnet-4-6".to_string(),
            "openai/gpt-5".to_string(),
        ],
        ..Default::default()
    };
    let runtime =
        build_runtime_config(settings, env, overrides).expect("runtime with fallback chain");
    let fallbacks = runtime.model_roles.fallbacks(ModelRole::Main);
    assert_eq!(fallbacks.len(), 2);
    assert_eq!(fallbacks[0].provider, "anthropic");
    assert_eq!(fallbacks[0].model_id, "claude-sonnet-4-6");
    assert_eq!(fallbacks[1].provider, "openai");
    assert_eq!(fallbacks[1].model_id, "gpt-5");
}

#[test]
fn test_cli_fallback_model_rejects_duplicate_of_primary() {
    // Configuring primary + fallback to the same slug makes the
    // fallback useless; hard-fail at startup rather than silently
    // accept a degenerate config.
    let settings = settings_with(Settings {
        model: Some("anthropic/claude-opus-4-6".into()),
        ..Default::default()
    });
    let env = EnvSnapshot::default();
    let overrides = RuntimeOverrides {
        fallback_model_overrides: vec!["anthropic/claude-opus-4-6".to_string()],
        ..Default::default()
    };
    let err = build_runtime_config(settings, env, overrides)
        .expect_err("duplicate primary+fallback must fail");
    assert!(
        err.to_string().contains("duplicates primary"),
        "expected duplicate-primary error, got: {err}"
    );
}

#[test]
fn test_cli_fallback_model_rejects_duplicate_within_chain() {
    let settings = settings_with(Settings::default());
    let env = EnvSnapshot::default();
    let overrides = RuntimeOverrides {
        fallback_model_overrides: vec![
            "anthropic/claude-sonnet-4-6".to_string(),
            "anthropic/claude-sonnet-4-6".to_string(),
        ],
        ..Default::default()
    };
    let err = build_runtime_config(settings, env, overrides)
        .expect_err("duplicate fallback within chain must fail");
    assert!(
        err.to_string().contains("duplicates earlier fallback"),
        "expected duplicate-earlier-fallback error, got: {err}"
    );
}

#[test]
fn test_cli_fallback_model_with_unknown_provider_fails_startup() {
    let settings = settings_with(Settings::default());
    let env = EnvSnapshot::default();
    let overrides = RuntimeOverrides {
        fallback_model_overrides: vec!["nonexistent/some-model".to_string()],
        ..Default::default()
    };
    let err = build_runtime_config(settings, env, overrides)
        .expect_err("unknown provider must fail fast at startup");
    let msg = err.to_string();
    assert!(
        msg.contains("unknown provider") || msg.contains("nonexistent"),
        "expected unknown-provider error, got: {msg}",
    );
}

#[test]
fn test_cli_fallback_overrides_take_precedence_over_settings_fallbacks() {
    // When settings.models.main has fallbacks AND --fallback-model
    // is supplied, CLI wins. Ensures users can override a config
    // they can't edit (project/policy level) from the command line.
    use crate::model::ModelSelection;
    use crate::model::RoleSlots;
    let settings = settings_with(Settings {
        models: crate::ModelSelectionSettings {
            main: Some(
                RoleSlots::new(ModelSelection {
                    provider: "anthropic".into(),
                    model_id: "claude-opus-4-6".into(),
                })
                .with_fallback(ModelSelection {
                    provider: "anthropic".into(),
                    model_id: "claude-sonnet-4-6".into(),
                }),
            ),
            ..Default::default()
        },
        ..Default::default()
    });
    let overrides = RuntimeOverrides {
        fallback_model_overrides: vec!["openai/gpt-5".into()],
        ..Default::default()
    };
    let runtime = build_runtime_config(settings, EnvSnapshot::default(), overrides)
        .expect("CLI should override settings fallbacks");
    let fallbacks = runtime.model_roles.fallbacks(ModelRole::Main);
    assert_eq!(fallbacks.len(), 1, "CLI replaces settings chain entirely");
    assert_eq!(fallbacks[0].provider, "openai");
    assert_eq!(fallbacks[0].model_id, "gpt-5");
}
