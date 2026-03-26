use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_registry_has_19_entries() {
    // 4 VS Code family + 15 JetBrains = 19
    assert_eq!(IDE_REGISTRY.len(), 19);
}

#[test]
fn test_registry_keys_are_unique() {
    let mut keys: Vec<&str> = IDE_REGISTRY.iter().map(|ide| ide.key).collect();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), IDE_REGISTRY.len());
}

#[test]
fn test_ide_for_key_found() {
    let vscode = ide_for_key("vscode");
    assert!(vscode.is_some());
    let vscode = vscode.expect("vscode should exist");
    assert_eq!(vscode.display_name, "VS Code");
    assert_eq!(vscode.kind, IdeKind::VsCode);
}

#[test]
fn test_ide_for_key_not_found() {
    assert!(ide_for_key("nonexistent").is_none());
}

#[test]
fn test_vscode_family_count() {
    let vscode_count = IDE_REGISTRY
        .iter()
        .filter(|ide| ide.kind == IdeKind::VsCode)
        .count();
    assert_eq!(vscode_count, 4);
}

#[test]
fn test_jetbrains_family_count() {
    let jb_count = IDE_REGISTRY
        .iter()
        .filter(|ide| ide.kind == IdeKind::JetBrains)
        .count();
    assert_eq!(jb_count, 15);
}

#[test]
fn test_addon_term() {
    assert_eq!(IdeKind::VsCode.addon_term(), "extension");
    assert_eq!(IdeKind::JetBrains.addon_term(), "plugin");
}
