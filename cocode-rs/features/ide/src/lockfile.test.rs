use std::path::Path;

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_parse_lockfile() {
    let json = r#"{
        "workspaceFolders": ["/home/user/project"],
        "pid": 12345,
        "ideName": "vscode",
        "transport": "ws",
        "runningInWindows": false,
        "authToken": "abc123"
    }"#;

    let lockfile: IdeLockfile = serde_json::from_str(json).expect("should parse");
    assert_eq!(lockfile.pid, 12345);
    assert_eq!(lockfile.ide_name, "vscode");
    assert_eq!(lockfile.transport, "ws");
    assert!(!lockfile.running_in_windows);
    assert_eq!(lockfile.auth_token, "abc123");
    assert_eq!(lockfile.workspace_folders, vec!["/home/user/project"]);
}

#[test]
fn test_parse_lockfile_minimal() {
    let json = r#"{"pid": 1, "ideName": "cursor"}"#;

    let lockfile: IdeLockfile = serde_json::from_str(json).expect("should parse");
    assert_eq!(lockfile.pid, 1);
    assert_eq!(lockfile.ide_name, "cursor");
    assert_eq!(lockfile.transport, "");
    assert!(!lockfile.running_in_windows);
    assert_eq!(lockfile.auth_token, "");
    assert!(lockfile.workspace_folders.is_empty());
}

#[test]
fn test_workspace_matches_exact() {
    let folders = vec!["/home/user/project".to_string()];
    assert!(workspace_matches(&folders, Path::new("/home/user/project")));
}

#[test]
fn test_workspace_matches_subdirectory() {
    let folders = vec!["/home/user/project".to_string()];
    assert!(workspace_matches(
        &folders,
        Path::new("/home/user/project/src/main.rs")
    ));
}

#[test]
fn test_workspace_no_match() {
    let folders = vec!["/home/user/other-project".to_string()];
    assert!(!workspace_matches(
        &folders,
        Path::new("/home/user/project")
    ));
}

#[test]
fn test_workspace_no_false_prefix_match() {
    // /home/user/project must NOT match /home/user/project2
    let folders = vec!["/home/user/project".to_string()];
    assert!(!workspace_matches(
        &folders,
        Path::new("/home/user/project2")
    ));
}

#[test]
fn test_workspace_matches_trailing_slash() {
    let folders = vec!["/home/user/project/".to_string()];
    assert!(workspace_matches(&folders, Path::new("/home/user/project")));
    assert!(workspace_matches(
        &folders,
        Path::new("/home/user/project/src")
    ));
}

#[test]
fn test_workspace_empty_folders() {
    let folders: Vec<String> = vec![];
    assert!(!workspace_matches(
        &folders,
        Path::new("/home/user/project")
    ));
}

#[test]
fn test_resolved_lockfile_mcp_url_websocket() {
    let resolved = ResolvedLockfile {
        lockfile: IdeLockfile {
            workspace_folders: vec![],
            pid: 1,
            ide_name: "vscode".into(),
            transport: "ws".into(),
            running_in_windows: false,
            auth_token: "token".into(),
        },
        ide_type: crate::detection::ide_for_key("vscode").expect("vscode exists"),
        port: 8080,
        host: "127.0.0.1".into(),
    };

    assert_eq!(resolved.mcp_url(), "ws://127.0.0.1:8080");
    assert!(resolved.is_websocket());
}

#[test]
fn test_resolved_lockfile_mcp_url_sse() {
    let resolved = ResolvedLockfile {
        lockfile: IdeLockfile {
            workspace_folders: vec![],
            pid: 1,
            ide_name: "cursor".into(),
            transport: "sse".into(),
            running_in_windows: false,
            auth_token: "token".into(),
        },
        ide_type: crate::detection::ide_for_key("cursor").expect("cursor exists"),
        port: 3000,
        host: "127.0.0.1".into(),
    };

    assert_eq!(resolved.mcp_url(), "http://127.0.0.1:3000/sse");
    assert!(!resolved.is_websocket());
}
