use super::*;

#[test]
fn test_keybinding_resolution() {
    let mut registry = KeybindingRegistry::new();
    registry.register(Keybinding {
        key: "ctrl+c".into(),
        action: "interrupt".into(),
        context: None,
        when: None,
    });
    registry.register(Keybinding {
        key: "ctrl+c".into(),
        action: "cancel_tool".into(),
        context: Some("tool_running".into()),
        when: None,
    });

    // Context-specific wins
    assert_eq!(
        registry.resolve("ctrl+c", "tool_running"),
        Some("cancel_tool")
    );
    // Global fallback
    assert_eq!(registry.resolve("ctrl+c", "idle"), Some("interrupt"));
    // Unknown key
    assert_eq!(registry.resolve("ctrl+z", "idle"), None);
}

#[test]
fn test_default_keybindings() {
    let defaults = load_default_keybindings();
    assert_eq!(defaults.len(), 7);

    // Verify all expected bindings are present
    let actions: Vec<&str> = defaults.iter().map(|b| b.action.as_str()).collect();
    assert!(actions.contains(&"interrupt"));
    assert!(actions.contains(&"quit"));
    assert!(actions.contains(&"submit"));
    assert!(actions.contains(&"cancel"));
    assert!(actions.contains(&"autocomplete"));
    assert!(actions.contains(&"clear"));
    assert!(actions.contains(&"compact"));

    // Verify ctrl+c maps to interrupt in input context
    let ctrl_c = defaults
        .iter()
        .find(|b| b.key == "ctrl+c")
        .expect("ctrl+c should exist");
    assert_eq!(ctrl_c.action, "interrupt");
    assert_eq!(ctrl_c.context.as_deref(), Some("input"));
}

#[test]
fn test_with_defaults() {
    let registry = KeybindingRegistry::with_defaults();

    assert_eq!(registry.resolve("ctrl+c", "input"), Some("interrupt"));
    assert_eq!(registry.resolve("ctrl+d", "input"), Some("quit"));
    assert_eq!(registry.resolve("enter", "input"), Some("submit"));
    assert_eq!(registry.resolve("escape", "dialog"), Some("cancel"));
    assert_eq!(registry.resolve("tab", "input"), Some("autocomplete"));
    assert_eq!(registry.resolve("ctrl+l", "global"), Some("clear"));
    assert_eq!(registry.resolve("ctrl+o", "global"), Some("compact"));
}

#[test]
fn test_resolve_by_context() {
    let registry = KeybindingRegistry::with_defaults();

    let input_bindings = registry.all_for_context("input");
    assert_eq!(input_bindings.len(), 4); // ctrl+c, ctrl+d, enter, tab

    let dialog_bindings = registry.all_for_context("dialog");
    assert_eq!(dialog_bindings.len(), 1); // escape

    let global_bindings = registry.all_for_context("global");
    assert_eq!(global_bindings.len(), 2); // ctrl+l, ctrl+o

    let empty = registry.all_for_context("nonexistent");
    assert!(empty.is_empty());
}

#[test]
fn test_register_custom() {
    let mut registry = KeybindingRegistry::with_defaults();

    // Custom binding overrides default in its context
    registry.register(Keybinding {
        key: "ctrl+c".into(),
        action: "copy".into(),
        context: Some("editor".into()),
        when: None,
    });

    // New context uses custom binding
    assert_eq!(registry.resolve("ctrl+c", "editor"), Some("copy"));
    // Original context still works
    assert_eq!(registry.resolve("ctrl+c", "input"), Some("interrupt"));
}

#[test]
fn test_register_custom_global_fallback() {
    let mut registry = KeybindingRegistry::new();

    // Register a global binding (no context)
    registry.register(Keybinding {
        key: "f1".into(),
        action: "help".into(),
        context: None,
        when: None,
    });

    // Should resolve in any context via global fallback
    assert_eq!(registry.resolve("f1", "input"), Some("help"));
    assert_eq!(registry.resolve("f1", "dialog"), Some("help"));
    assert_eq!(registry.resolve("f1", "global"), Some("help"));
}

#[test]
fn test_all_for_context_returns_correct_bindings() {
    let mut registry = KeybindingRegistry::new();
    registry.register(Keybinding {
        key: "a".into(),
        action: "action_a".into(),
        context: Some("ctx".into()),
        when: None,
    });
    registry.register(Keybinding {
        key: "b".into(),
        action: "action_b".into(),
        context: Some("ctx".into()),
        when: None,
    });
    registry.register(Keybinding {
        key: "c".into(),
        action: "action_c".into(),
        context: Some("other".into()),
        when: None,
    });

    let ctx_bindings = registry.all_for_context("ctx");
    assert_eq!(ctx_bindings.len(), 2);
    let keys: Vec<&str> = ctx_bindings.iter().map(|b| b.key.as_str()).collect();
    assert!(keys.contains(&"a"));
    assert!(keys.contains(&"b"));
}
