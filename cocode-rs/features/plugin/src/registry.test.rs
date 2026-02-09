use super::*;
use crate::contribution::PluginContributions;
use crate::manifest::PluginManifest;
use crate::manifest::PluginMetadata;
use std::path::PathBuf;

fn make_plugin(name: &str, scope: PluginScope) -> LoadedPlugin {
    LoadedPlugin {
        manifest: PluginManifest {
            plugin: PluginMetadata {
                name: name.to_string(),
                version: "1.0.0".to_string(),
                description: "Test plugin".to_string(),
                author: None,
                repository: None,
                license: None,
                min_cocode_version: None,
            },
            contributions: PluginContributions::default(),
        },
        path: PathBuf::from(format!("/plugins/{name}")),
        scope,
        contributions: Vec::new(),
    }
}

#[test]
fn test_register_and_get() {
    let mut registry = PluginRegistry::new();
    let plugin = make_plugin("test", PluginScope::User);

    registry.register(plugin).expect("register");

    assert!(registry.has("test"));
    assert!(!registry.has("other"));

    let plugin = registry.get("test").expect("get");
    assert_eq!(plugin.name(), "test");
}

#[test]
fn test_duplicate_registration() {
    let mut registry = PluginRegistry::new();

    registry
        .register(make_plugin("test", PluginScope::User))
        .expect("first");
    let result = registry.register(make_plugin("test", PluginScope::Project));

    assert!(result.is_err());
    #[cfg(test)]
    assert!(matches!(
        result.unwrap_err(),
        PluginError::AlreadyRegistered { .. }
    ));
}

#[test]
fn test_names() {
    let mut registry = PluginRegistry::new();
    registry
        .register(make_plugin("beta", PluginScope::User))
        .expect("register");
    registry
        .register(make_plugin("alpha", PluginScope::Project))
        .expect("register");

    let names = registry.names();
    assert_eq!(names, vec!["alpha", "beta"]);
}

#[test]
fn test_by_scope() {
    let mut registry = PluginRegistry::new();
    registry
        .register(make_plugin("user1", PluginScope::User))
        .expect("register");
    registry
        .register(make_plugin("user2", PluginScope::User))
        .expect("register");
    registry
        .register(make_plugin("project1", PluginScope::Project))
        .expect("register");

    let user_plugins = registry.by_scope(PluginScope::User);
    assert_eq!(user_plugins.len(), 2);

    let project_plugins = registry.by_scope(PluginScope::Project);
    assert_eq!(project_plugins.len(), 1);
}

#[test]
fn test_unregister() {
    let mut registry = PluginRegistry::new();
    registry
        .register(make_plugin("test", PluginScope::User))
        .expect("register");

    let removed = registry.unregister("test");
    assert!(removed.is_some());
    assert!(!registry.has("test"));
}
