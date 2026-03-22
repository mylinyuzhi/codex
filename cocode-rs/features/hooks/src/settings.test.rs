use super::*;

#[test]
fn test_defaults() {
    let settings = HookSettings::default();
    assert!(!settings.disable_all_hooks);
    assert!(!settings.allow_managed_hooks_only);
    assert!(settings.workspace_trusted);
}

#[test]
fn test_serde_defaults() {
    let json = "{}";
    let settings: HookSettings = serde_json::from_str(json).expect("deserialize");
    assert!(!settings.disable_all_hooks);
    assert!(!settings.allow_managed_hooks_only);
    assert!(settings.workspace_trusted);
}

#[test]
fn test_serde_roundtrip() {
    let settings = HookSettings {
        disable_all_hooks: true,
        allow_managed_hooks_only: true,
        workspace_trusted: false,
    };
    let json = serde_json::to_string(&settings).expect("serialize");
    let parsed: HookSettings = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed.disable_all_hooks, settings.disable_all_hooks);
    assert_eq!(
        parsed.allow_managed_hooks_only,
        settings.allow_managed_hooks_only
    );
    assert_eq!(parsed.workspace_trusted, settings.workspace_trusted);
}

#[test]
fn test_workspace_untrusted() {
    let settings = HookSettings {
        disable_all_hooks: false,
        allow_managed_hooks_only: false,
        workspace_trusted: false,
    };
    assert!(!settings.workspace_trusted);
}
