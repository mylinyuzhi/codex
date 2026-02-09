use super::*;

#[test]
fn test_skill_source_display() {
    assert_eq!(SkillSource::Builtin.to_string(), "builtin");
    assert_eq!(SkillSource::Bundled.to_string(), "bundled");
    assert_eq!(SkillSource::Mcp.to_string(), "mcp");
    assert_eq!(SkillSource::PolicySettings.to_string(), "policy-settings");

    let local = SkillSource::ProjectSettings {
        path: PathBuf::from("/project/.cocode/skills/commit"),
    };
    assert_eq!(
        local.to_string(),
        "project-settings (/project/.cocode/skills/commit)"
    );

    let global = SkillSource::UserSettings {
        path: PathBuf::from("/home/user/.cocode/skills/review"),
    };
    assert_eq!(
        global.to_string(),
        "user-settings (/home/user/.cocode/skills/review)"
    );

    let plugin = SkillSource::Plugin {
        plugin_name: "my-plugin".to_string(),
    };
    assert_eq!(plugin.to_string(), "plugin (my-plugin)");
}

#[test]
fn test_loaded_from_display() {
    assert_eq!(LoadedFrom::Builtin.to_string(), "builtin");
    assert_eq!(LoadedFrom::Bundled.to_string(), "bundled");
    assert_eq!(LoadedFrom::Mcp.to_string(), "mcp");
    assert_eq!(LoadedFrom::Plugin.to_string(), "plugin");
    assert_eq!(LoadedFrom::ProjectSettings.to_string(), "project settings");
    assert_eq!(LoadedFrom::UserSettings.to_string(), "user settings");
    assert_eq!(LoadedFrom::PolicySettings.to_string(), "policy settings");
}

#[test]
fn test_loaded_from_conversion() {
    assert_eq!(LoadedFrom::from(&SkillSource::Builtin), LoadedFrom::Builtin);
    assert_eq!(LoadedFrom::from(&SkillSource::Bundled), LoadedFrom::Bundled);
    assert_eq!(LoadedFrom::from(&SkillSource::Mcp), LoadedFrom::Mcp);
    assert_eq!(
        LoadedFrom::from(&SkillSource::ProjectSettings {
            path: PathBuf::from("/x")
        }),
        LoadedFrom::ProjectSettings
    );
    assert_eq!(
        LoadedFrom::from(&SkillSource::UserSettings {
            path: PathBuf::from("/x")
        }),
        LoadedFrom::UserSettings
    );
    assert_eq!(
        LoadedFrom::from(&SkillSource::Plugin {
            plugin_name: "p".to_string()
        }),
        LoadedFrom::Plugin
    );
    assert_eq!(
        LoadedFrom::from(&SkillSource::PolicySettings),
        LoadedFrom::PolicySettings
    );
}

#[test]
fn test_priority_ordering() {
    assert!(SkillSource::Builtin.priority() < SkillSource::Bundled.priority());
    assert!(SkillSource::Bundled.priority() < SkillSource::Mcp.priority());
    assert!(
        SkillSource::Mcp.priority()
            < SkillSource::Plugin {
                plugin_name: "x".to_string()
            }
            .priority()
    );
    assert!(
        SkillSource::Plugin {
            plugin_name: "x".to_string()
        }
        .priority()
            < SkillSource::ProjectSettings {
                path: PathBuf::from("/x")
            }
            .priority()
    );
    assert!(
        SkillSource::ProjectSettings {
            path: PathBuf::from("/x")
        }
        .priority()
            < SkillSource::UserSettings {
                path: PathBuf::from("/x")
            }
            .priority()
    );
    assert!(
        SkillSource::UserSettings {
            path: PathBuf::from("/x")
        }
        .priority()
            < SkillSource::PolicySettings.priority()
    );
}
