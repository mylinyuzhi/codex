use super::*;

#[test]
fn test_priority_order() {
    assert!(HookScope::Policy < HookScope::Plugin);
    assert!(HookScope::Plugin < HookScope::Session);
    assert!(HookScope::Session < HookScope::Agent);
    assert!(HookScope::Agent < HookScope::Skill);
}

#[test]
fn test_sorting() {
    let mut scopes = vec![
        HookScope::Skill,
        HookScope::Policy,
        HookScope::Agent,
        HookScope::Session,
        HookScope::Plugin,
    ];
    scopes.sort();
    assert_eq!(
        scopes,
        vec![
            HookScope::Policy,
            HookScope::Plugin,
            HookScope::Session,
            HookScope::Agent,
            HookScope::Skill,
        ]
    );
}

#[test]
fn test_display() {
    assert_eq!(format!("{}", HookScope::Policy), "policy");
    assert_eq!(format!("{}", HookScope::Plugin), "plugin");
    assert_eq!(format!("{}", HookScope::Session), "session");
    assert_eq!(format!("{}", HookScope::Agent), "agent");
    assert_eq!(format!("{}", HookScope::Skill), "skill");
}

#[test]
fn test_serde_roundtrip() {
    let scope = HookScope::Session;
    let json = serde_json::to_string(&scope).expect("serialize");
    assert_eq!(json, "\"session\"");
    let parsed: HookScope = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed, scope);
}

// HookSource tests

#[test]
fn test_hook_source_scope() {
    assert_eq!(HookSource::Policy.scope(), HookScope::Policy);
    assert_eq!(
        HookSource::Plugin {
            name: "test".to_string()
        }
        .scope(),
        HookScope::Plugin
    );
    assert_eq!(HookSource::Session.scope(), HookScope::Session);
    assert_eq!(
        HookSource::Agent {
            name: "test".to_string()
        }
        .scope(),
        HookScope::Agent
    );
    assert_eq!(
        HookSource::Skill {
            name: "test".to_string()
        }
        .scope(),
        HookScope::Skill
    );
}

#[test]
fn test_hook_source_is_managed() {
    assert!(HookSource::Policy.is_managed());
    assert!(
        HookSource::Plugin {
            name: "test".to_string()
        }
        .is_managed()
    );
    assert!(!HookSource::Session.is_managed());
    assert!(
        !HookSource::Agent {
            name: "test".to_string()
        }
        .is_managed()
    );
    assert!(
        !HookSource::Skill {
            name: "test".to_string()
        }
        .is_managed()
    );
}

#[test]
fn test_hook_source_name() {
    assert!(HookSource::Policy.name().is_none());
    assert_eq!(
        HookSource::Plugin {
            name: "my-plugin".to_string()
        }
        .name(),
        Some("my-plugin")
    );
    assert!(HookSource::Session.name().is_none());
    assert_eq!(
        HookSource::Agent {
            name: "my-agent".to_string()
        }
        .name(),
        Some("my-agent")
    );
    assert_eq!(
        HookSource::Skill {
            name: "my-skill".to_string()
        }
        .name(),
        Some("my-skill")
    );
}

#[test]
fn test_hook_source_display() {
    assert_eq!(format!("{}", HookSource::Policy), "policy");
    assert_eq!(
        format!(
            "{}",
            HookSource::Plugin {
                name: "my-plugin".to_string()
            }
        ),
        "plugin:my-plugin"
    );
    assert_eq!(format!("{}", HookSource::Session), "session");
    assert_eq!(
        format!(
            "{}",
            HookSource::Agent {
                name: "my-agent".to_string()
            }
        ),
        "agent:my-agent"
    );
    assert_eq!(
        format!(
            "{}",
            HookSource::Skill {
                name: "my-skill".to_string()
            }
        ),
        "skill:my-skill"
    );
}

#[test]
fn test_hook_source_default() {
    assert_eq!(HookSource::default(), HookSource::Session);
}

#[test]
fn test_hook_source_serde_roundtrip() {
    let sources = vec![
        HookSource::Policy,
        HookSource::Plugin {
            name: "test-plugin".to_string(),
        },
        HookSource::Session,
        HookSource::Agent {
            name: "test-agent".to_string(),
        },
        HookSource::Skill {
            name: "test-skill".to_string(),
        },
    ];

    for source in sources {
        let json = serde_json::to_string(&source).expect("serialize");
        let parsed: HookSource = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, source);
    }
}

#[test]
fn test_hook_source_serde_format() {
    let policy = HookSource::Policy;
    let json = serde_json::to_string(&policy).expect("serialize");
    assert!(json.contains("\"type\":\"policy\""));

    let plugin = HookSource::Plugin {
        name: "test".to_string(),
    };
    let json = serde_json::to_string(&plugin).expect("serialize");
    assert!(json.contains("\"type\":\"plugin\""));
    assert!(json.contains("\"name\":\"test\""));

    let agent = HookSource::Agent {
        name: "verify".to_string(),
    };
    let json = serde_json::to_string(&agent).expect("serialize");
    assert!(json.contains("\"type\":\"agent\""));
    assert!(json.contains("\"name\":\"verify\""));
}
