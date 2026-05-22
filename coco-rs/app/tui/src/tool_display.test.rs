use coco_types::PermissionDisplayInput;
use coco_types::ToolName;
use pretty_assertions::assert_eq;

use super::permission_display_input;
use super::tool_input_preview;

#[test]
fn test_permission_display_input_formats_glob_without_json() {
    let input = serde_json::json!({
        "path": "/Users/linyuzhi/codespace/myagent/codex",
        "pattern": "**/README.md"
    });

    assert_eq!(
        permission_display_input(ToolName::Glob.as_str(), &input),
        PermissionDisplayInput::Text(
            "path: /Users/linyuzhi/codespace/myagent/codex\npattern: **/README.md".into()
        )
    );
}

#[test]
fn test_tool_input_preview_formats_glob_as_single_line() {
    let input = serde_json::json!({
        "path": "/Users/linyuzhi/codespace/myagent/codex",
        "pattern": "**/README.md"
    });

    assert_eq!(
        tool_input_preview(ToolName::Glob.as_str(), &input),
        "**/README.md in /Users/linyuzhi/codespace/myagent/codex"
    );
}

#[test]
fn test_shell_permission_display_input_keeps_command_variant() {
    let input = serde_json::json!({"command": "ls -la"});

    assert_eq!(
        permission_display_input(ToolName::Bash.as_str(), &input),
        PermissionDisplayInput::Command("ls -la".into())
    );
}
