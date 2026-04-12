use super::*;

#[test]
fn test_resolve_specific_context() {
    let mut resolver = KeybindingResolver::new();
    resolver.register(Keybinding {
        key: "enter".into(),
        action: "submit".into(),
        context: KeyContext::Input,
        description: None,
    });
    let result = resolver.resolve(KeyContext::Input, "enter");
    assert!(result.is_some());
    assert_eq!(result.unwrap().action, "submit");
}

#[test]
fn test_resolve_fallback_to_global() {
    let mut resolver = KeybindingResolver::new();
    resolver.register(Keybinding {
        key: "ctrl+c".into(),
        action: "interrupt".into(),
        context: KeyContext::Global,
        description: None,
    });
    let result = resolver.resolve(KeyContext::Input, "ctrl+c");
    assert!(result.is_some());
    assert_eq!(result.unwrap().action, "interrupt");
}

#[test]
fn test_load_defaults() {
    let mut resolver = KeybindingResolver::new();
    resolver.load_defaults();
    let global = resolver.bindings_for_context(KeyContext::Global);
    assert!(!global.is_empty());
}
