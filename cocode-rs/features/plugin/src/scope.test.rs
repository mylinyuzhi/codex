use super::*;

#[test]
fn test_scope_priority() {
    assert!(PluginScope::Project.priority() > PluginScope::User.priority());
    assert!(PluginScope::User.priority() > PluginScope::Managed.priority());
}

#[test]
fn test_scope_display() {
    assert_eq!(PluginScope::Managed.to_string(), "managed");
    assert_eq!(PluginScope::User.to_string(), "user");
    assert_eq!(PluginScope::Project.to_string(), "project");
}

#[test]
fn test_scope_ordering() {
    let mut scopes = vec![
        PluginScope::Project,
        PluginScope::Managed,
        PluginScope::User,
    ];
    scopes.sort();
    assert_eq!(
        scopes,
        vec![
            PluginScope::Managed,
            PluginScope::User,
            PluginScope::Project
        ]
    );
}
