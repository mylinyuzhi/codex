use super::*;
use std::fs;

#[test]
fn test_parse_lsp_config() {
    let json = r#"{
            "name": "rust-analyzer",
            "description": "Rust language server",
            "languages": ["rust"],
            "command": "rust-analyzer",
            "args": [],
            "file_patterns": ["*.rs", "Cargo.toml"],
            "root_markers": ["Cargo.toml"]
        }"#;

    let config: LspServerConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.name, "rust-analyzer");
    assert_eq!(config.languages, vec!["rust"]);
    assert_eq!(config.command, "rust-analyzer");
}

#[test]
fn test_load_lsp_from_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let lsp_dir = tmp.path().join("lsp");
    fs::create_dir_all(&lsp_dir).unwrap();
    fs::write(
        lsp_dir.join(LSP_JSON),
        r#"{
                "name": "test-lsp",
                "languages": ["test"],
                "command": "test-lsp-server"
            }"#,
    )
    .unwrap();

    let results = load_lsp_servers_from_dir(tmp.path(), "test-plugin");
    assert_eq!(results.len(), 1);
    assert!(results[0].is_lsp_server());
    assert_eq!(results[0].name(), "test-lsp");
}

#[test]
fn test_load_lsp_from_file_array() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(".lsp.json");
    fs::write(
        &path,
        r#"[
                {"name": "lsp1", "languages": ["a"], "command": "cmd1"},
                {"name": "lsp2", "languages": ["b"], "command": "cmd2"}
            ]"#,
    )
    .unwrap();

    let results = load_lsp_servers_from_file(&path, "test-plugin");
    assert_eq!(results.len(), 2);
}
