use super::*;

#[test]
fn test_non_ide_tools_always_exposed() {
    assert!(should_expose_to_model("Read"));
    assert!(should_expose_to_model("Edit"));
    assert!(should_expose_to_model("Bash"));
    assert!(should_expose_to_model("mcp__other__tool"));
}

#[test]
fn test_ide_allowlisted_tools_exposed() {
    assert!(should_expose_to_model("mcp__ide__executeCode"));
    assert!(should_expose_to_model("mcp__ide__getDiagnostics"));
}

#[test]
fn test_ide_internal_tools_hidden() {
    assert!(!should_expose_to_model("mcp__ide__openDiff"));
    assert!(!should_expose_to_model("mcp__ide__close_tab"));
    assert!(!should_expose_to_model("mcp__ide__openFile"));
    assert!(!should_expose_to_model("mcp__ide__closeAllDiffTabs"));
    assert!(!should_expose_to_model("mcp__ide__set_permission_mode"));
    assert!(!should_expose_to_model("mcp__ide__getWorkspaceFolders"));
}

#[test]
fn test_is_ide_tool() {
    assert!(is_ide_tool("mcp__ide__openDiff"));
    assert!(is_ide_tool("mcp__ide__getDiagnostics"));
    assert!(!is_ide_tool("Read"));
    assert!(!is_ide_tool("mcp__other__tool"));
}
