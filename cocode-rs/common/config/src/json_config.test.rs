use super::*;
use cocode_protocol::Feature;

#[test]
fn test_app_config_default() {
    let config = AppConfig::default();
    assert!(config.models.is_none());
    assert!(config.profile.is_none());
    assert!(config.logging.is_none());
    assert!(config.features.is_none());
    assert!(config.profiles.is_empty());
}

#[test]
fn test_app_config_parse_minimal() {
    let json_str = r#"{
        "models": {
            "main": "openai/gpt-5"
        }
    }"#;
    let config: AppConfig = serde_json::from_str(json_str).unwrap();
    let models = config.models.as_ref().unwrap();
    assert_eq!(models.main.as_ref().unwrap().provider, "openai");
    assert_eq!(models.main.as_ref().unwrap().model, "gpt-5");
}

#[test]
fn test_app_config_parse_full() {
    let json_str = r#"{
        "models": {
            "main": "genai/gemini-3-pro",
            "fast": "genai/gemini-3-flash"
        },
        "profile": "coding",
        "logging": {
            "level": "debug",
            "location": true,
            "target": false
        },
        "features": {
            "web_fetch": true
        }
    }"#;
    let config: AppConfig = serde_json::from_str(json_str).unwrap();
    let models = config.models.as_ref().unwrap();
    assert_eq!(models.main.as_ref().unwrap().provider, "genai");
    assert_eq!(models.main.as_ref().unwrap().model, "gemini-3-pro");
    assert_eq!(config.profile, Some("coding".to_string()));

    let logging = config.logging.unwrap();
    assert_eq!(logging.level, Some("debug".to_string()));
    assert_eq!(logging.location, Some(true));
    assert_eq!(logging.target, Some(false));

    let features = config.features.unwrap();
    assert_eq!(features.get("web_fetch"), Some(true));
}

#[test]
fn test_app_config_parse_with_profiles() {
    let json_str = r#"{
        "models": {
            "main": "openai/gpt-5"
        },
        "profile": "fast",
        "logging": {
            "level": "info"
        },
        "features": {
            "web_fetch": true
        },
        "profiles": {
            "anthropic": {
                "models": {
                    "main": "anthropic/claude-opus-4"
                }
            },
            "fast": {
                "models": {
                    "main": "openai/gpt-5-mini"
                },
                "features": {
                    "web_fetch": false
                }
            },
            "debug": {
                "logging": {
                    "level": "debug",
                    "location": true
                }
            }
        }
    }"#;
    let config: AppConfig = serde_json::from_str(json_str).unwrap();

    // Verify top-level
    let models = config.models.as_ref().unwrap();
    assert_eq!(models.main.as_ref().unwrap().model, "gpt-5");
    assert_eq!(config.profile, Some("fast".to_string()));

    // Verify profiles parsed
    assert_eq!(config.profiles.len(), 3);
    assert!(config.has_profile("anthropic"));
    assert!(config.has_profile("fast"));
    assert!(config.has_profile("debug"));

    // Check profile contents
    let anthropic = &config.profiles["anthropic"];
    let anthropic_models = anthropic.models.as_ref().unwrap();
    assert_eq!(
        anthropic_models.main.as_ref().unwrap().provider,
        "anthropic"
    );
    assert_eq!(
        anthropic_models.main.as_ref().unwrap().model,
        "claude-opus-4"
    );

    let fast = &config.profiles["fast"];
    assert!(fast.features.is_some());
    let fast_models = fast.models.as_ref().unwrap();
    assert_eq!(fast_models.main.as_ref().unwrap().model, "gpt-5-mini");

    let debug = &config.profiles["debug"];
    assert!(debug.logging.is_some());
    assert_eq!(
        debug.logging.as_ref().unwrap().level,
        Some("debug".to_string())
    );
}

#[test]
fn test_resolve_with_no_profile() {
    let json_str = r#"{
        "models": {
            "main": "openai/gpt-5"
        },
        "features": {
            "web_fetch": true
        }
    }"#;
    let config: AppConfig = serde_json::from_str(json_str).unwrap();
    let resolved = config.resolve();

    let main = resolved.models.main().unwrap();
    assert_eq!(main.provider, "openai");
    assert_eq!(main.model, "gpt-5");
    assert!(resolved.features.enabled(Feature::WebFetch));
}

#[test]
fn test_resolve_with_profile_override() {
    let json_str = r#"{
        "models": {
            "main": "openai/gpt-5"
        },
        "profile": "fast",
        "features": {
            "web_fetch": true
        },
        "profiles": {
            "fast": {
                "models": {
                    "main": "openai/gpt-5-mini"
                },
                "features": {
                    "web_fetch": false
                }
            }
        }
    }"#;
    let config: AppConfig = serde_json::from_str(json_str).unwrap();
    let resolved = config.resolve();

    // main model from profile
    let main = resolved.models.main().unwrap();
    assert_eq!(main.model, "gpt-5-mini");
    // features: profile overrides web_fetch to false
    assert!(!resolved.features.enabled(Feature::WebFetch));
}

#[test]
fn test_resolve_provider_override() {
    let json_str = r#"{
        "models": {
            "main": "openai/gpt-5"
        },
        "profile": "anthropic",
        "profiles": {
            "anthropic": {
                "models": {
                    "main": "anthropic/claude-opus-4"
                }
            }
        }
    }"#;
    let config: AppConfig = serde_json::from_str(json_str).unwrap();
    let resolved = config.resolve();

    let main = resolved.models.main().unwrap();
    assert_eq!(main.provider, "anthropic");
    assert_eq!(main.model, "claude-opus-4");
}

#[test]
fn test_resolve_logging_merge() {
    let json_str = r#"{
        "models": {
            "main": "openai/gpt-5"
        },
        "profile": "debug",
        "logging": {
            "level": "info",
            "target": true
        },
        "profiles": {
            "debug": {
                "logging": {
                    "level": "debug",
                    "location": true
                }
            }
        }
    }"#;
    let config: AppConfig = serde_json::from_str(json_str).unwrap();
    let resolved = config.resolve();

    let logging = resolved.logging.unwrap();
    // level from profile
    assert_eq!(logging.level, Some("debug".to_string()));
    // location from profile
    assert_eq!(logging.location, Some(true));
    // target from base (not overridden)
    assert_eq!(logging.target, Some(true));
}

#[test]
fn test_resolve_nonexistent_profile() {
    let json_str = r#"{
        "models": {
            "main": "openai/gpt-5"
        },
        "profile": "nonexistent"
    }"#;
    let config: AppConfig = serde_json::from_str(json_str).unwrap();
    let resolved = config.resolve();

    // Falls back to top-level
    let main = resolved.models.main().unwrap();
    assert_eq!(main.provider, "openai");
    assert_eq!(main.model, "gpt-5");
}

#[test]
fn test_features_config_into_features() {
    let mut entries = BTreeMap::new();
    entries.insert("web_fetch".to_string(), true);
    entries.insert("ls".to_string(), false);

    let features_config = FeaturesConfig { entries };
    let features = features_config.into_features();

    // web_fetch should be enabled (it was set to true)
    assert!(features.enabled(Feature::WebFetch));
    // ls should be disabled (it was set to false, overriding default true)
    assert!(!features.enabled(Feature::Ls));
}

#[test]
fn test_logging_config_default() {
    let config = LoggingConfig::default();
    assert!(config.level.is_none());
    assert!(config.location.is_none());
    assert!(config.target.is_none());
}

#[test]
fn test_app_config_resolve_features_with_features() {
    let mut entries = BTreeMap::new();
    entries.insert("web_fetch".to_string(), true);

    let config = AppConfig {
        features: Some(FeaturesConfig { entries }),
        ..Default::default()
    };

    let features = config.resolve_features();
    assert!(features.enabled(Feature::WebFetch));
}

#[test]
fn test_app_config_resolve_features_without_features() {
    let config = AppConfig::default();
    let features = config.resolve_features();

    // Should return defaults
    assert!(features.enabled(Feature::Ls));
    assert!(!features.enabled(Feature::WebFetch));
}

#[test]
fn test_features_config_unknown_keys_empty() {
    let mut entries = BTreeMap::new();
    entries.insert("web_fetch".to_string(), true);
    entries.insert("ls".to_string(), false);

    let features = FeaturesConfig { entries };
    assert!(features.unknown_keys().is_empty());
}

#[test]
fn test_features_config_unknown_keys_with_unknown() {
    let mut entries = BTreeMap::new();
    entries.insert("web_fetch".to_string(), true);
    entries.insert("unknown_feature".to_string(), true);
    entries.insert("another_unknown".to_string(), false);

    let features = FeaturesConfig { entries };
    let unknown = features.unknown_keys();

    assert_eq!(unknown.len(), 2);
    assert!(unknown.contains(&"unknown_feature".to_string()));
    assert!(unknown.contains(&"another_unknown".to_string()));
}

#[test]
fn test_list_profiles() {
    let json_str = r#"{
        "profiles": {
            "main": {
                "models": {"main": "openai/gpt-5"}
            },
            "fast": {
                "models": {"main": "openai/gpt-5-mini"}
            }
        }
    }"#;
    let config: AppConfig = serde_json::from_str(json_str).unwrap();
    let profiles = config.list_profiles();

    assert_eq!(profiles.len(), 2);
    assert!(profiles.contains(&"main"));
    assert!(profiles.contains(&"fast"));
}

#[test]
fn test_selected_profile() {
    let json_str = r#"{
        "profile": "fast",
        "profiles": {
            "fast": {
                "models": {"main": "openai/gpt-5-mini"}
            }
        }
    }"#;
    let config: AppConfig = serde_json::from_str(json_str).unwrap();
    let profile = config.selected_profile();

    assert!(profile.is_some());
    let models = profile.unwrap().models.as_ref().unwrap();
    assert_eq!(models.main.as_ref().unwrap().model, "gpt-5-mini");
}

#[test]
fn test_merge_logging() {
    let base = LoggingConfig {
        level: Some("info".to_string()),
        location: Some(false),
        target: Some(true),
        timezone: Some("local".to_string()),
        modules: Some(vec!["cocode_core=info".to_string()]),
    };
    let override_config = LoggingConfig {
        level: Some("debug".to_string()),
        location: Some(true),
        target: None,
        timezone: None,
        modules: Some(vec!["cocode_core=debug".to_string()]),
    };

    let merged = merge_logging(&base, &override_config);

    assert_eq!(merged.level, Some("debug".to_string()));
    assert_eq!(merged.location, Some(true));
    assert_eq!(merged.target, Some(true)); // Kept from base
    assert_eq!(merged.timezone, Some("local".to_string())); // Kept from base
    assert_eq!(merged.modules, Some(vec!["cocode_core=debug".to_string()])); // From override
}

#[test]
fn test_resolve_with_models_field() {
    let json_str = r#"{
        "models": {
            "main": "anthropic/claude-opus-4",
            "fast": "anthropic/claude-haiku",
            "vision": "openai/gpt-4o"
        }
    }"#;
    let config: AppConfig = serde_json::from_str(json_str).unwrap();
    let resolved = config.resolve();

    // models field should be populated
    let main = resolved.models.main().unwrap();
    assert_eq!(main.provider, "anthropic");
    assert_eq!(main.model, "claude-opus-4");

    let fast = resolved.models.fast.as_ref().unwrap();
    assert_eq!(fast.provider, "anthropic");
    assert_eq!(fast.model, "claude-haiku");

    let vision = resolved.models.vision.as_ref().unwrap();
    assert_eq!(vision.provider, "openai");
    assert_eq!(vision.model, "gpt-4o");
}

#[test]
fn test_resolve_profile_models_override() {
    let json_str = r#"{
        "models": {
            "main": "anthropic/claude-opus-4",
            "fast": "anthropic/claude-haiku"
        },
        "profile": "openai",
        "profiles": {
            "openai": {
                "models": {
                    "main": "openai/gpt-5",
                    "vision": "openai/gpt-4o"
                }
            }
        }
    }"#;
    let config: AppConfig = serde_json::from_str(json_str).unwrap();
    let resolved = config.resolve();

    // main overridden by profile
    let main = resolved.models.main().unwrap();
    assert_eq!(main.provider, "openai");
    assert_eq!(main.model, "gpt-5");

    // fast NOT overridden (kept from base)
    let fast = resolved.models.fast.as_ref().unwrap();
    assert_eq!(fast.model, "claude-haiku");

    // vision added by profile
    let vision = resolved.models.vision.as_ref().unwrap();
    assert_eq!(vision.model, "gpt-4o");
}

#[test]
fn test_app_config_parse_with_models() {
    let json_str = r#"{
        "models": {
            "main": "anthropic/claude-opus-4",
            "fast": "anthropic/claude-haiku"
        }
    }"#;
    let config: AppConfig = serde_json::from_str(json_str).unwrap();

    assert!(config.models.is_some());
    let models = config.models.as_ref().unwrap();
    assert!(models.main.is_some());
    assert!(models.fast.is_some());
}

#[test]
fn test_config_profile_with_models() {
    let json_str = r#"{
        "profiles": {
            "test": {
                "models": {
                    "main": "openai/gpt-5"
                }
            }
        }
    }"#;
    let config: AppConfig = serde_json::from_str(json_str).unwrap();

    let profile = &config.profiles["test"];
    assert!(profile.models.is_some());
    let models = profile.models.as_ref().unwrap();
    assert_eq!(models.main.as_ref().unwrap().model, "gpt-5");
}

#[test]
fn test_resolved_models_empty_when_no_config() {
    let config = AppConfig::default();
    let resolved = config.resolve();

    // models should be empty default
    assert!(resolved.models.is_empty());
    assert!(resolved.models.main().is_none());
}
