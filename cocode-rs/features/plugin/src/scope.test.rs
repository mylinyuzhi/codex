use super::*;

#[test]
fn test_scope_priority() {
    assert!(PluginScope::Flag.priority() > PluginScope::Local.priority());
    assert!(PluginScope::Local.priority() > PluginScope::Project.priority());
    assert!(PluginScope::Project.priority() > PluginScope::User.priority());
    assert!(PluginScope::User.priority() > PluginScope::Managed.priority());
}

#[test]
fn test_scope_display() {
    assert_eq!(PluginScope::Managed.to_string(), "managed");
    assert_eq!(PluginScope::User.to_string(), "user");
    assert_eq!(PluginScope::Project.to_string(), "project");
    assert_eq!(PluginScope::Local.to_string(), "local");
    assert_eq!(PluginScope::Flag.to_string(), "flag");
}

#[test]
fn test_scope_ordering() {
    let mut scopes = vec![
        PluginScope::Flag,
        PluginScope::Project,
        PluginScope::Managed,
        PluginScope::Local,
        PluginScope::User,
    ];
    scopes.sort();
    assert_eq!(
        scopes,
        vec![
            PluginScope::Managed,
            PluginScope::User,
            PluginScope::Project,
            PluginScope::Local,
            PluginScope::Flag,
        ]
    );
}

#[test]
fn test_scope_default_dir() {
    // User, Local, Project, and Flag have no default directories (require runtime context)
    assert!(PluginScope::User.default_dir().is_none());
    assert!(PluginScope::Local.default_dir().is_none());
    assert!(PluginScope::Flag.default_dir().is_none());
    assert!(PluginScope::Project.default_dir().is_none());
}
