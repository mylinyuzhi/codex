use super::*;

#[test]
fn test_strip_safe_env_vars() {
    assert_eq!(
        strip_safe_wrappers("RUST_LOG=debug cargo test"),
        "cargo test"
    );
    assert_eq!(
        strip_safe_wrappers("NO_COLOR=1 RUST_BACKTRACE=1 cargo build"),
        "cargo build"
    );
}

#[test]
fn test_strip_unsafe_env_var_kept() {
    // LD_PRELOAD is not safe — should NOT be stripped
    assert_eq!(
        strip_safe_wrappers("LD_PRELOAD=/evil.so cat /etc/passwd"),
        "LD_PRELOAD=/evil.so cat /etc/passwd"
    );
}

#[test]
fn test_strip_wrappers() {
    assert_eq!(strip_safe_wrappers("nohup cargo build"), "cargo build");
    assert_eq!(strip_safe_wrappers("time cargo test"), "cargo test");
}

#[test]
fn test_strip_combined() {
    assert_eq!(
        strip_safe_wrappers("RUST_LOG=info timeout 60 cargo test"),
        "cargo test"
    );
}

#[test]
fn test_get_command_prefix() {
    assert_eq!(get_command_prefix("git status"), Some("git status".into()));
    assert_eq!(
        get_command_prefix("git commit -m msg"),
        Some("git commit".into())
    );
    assert_eq!(
        get_command_prefix("cargo test --release"),
        Some("cargo test".into())
    );
    assert_eq!(get_command_prefix("ls -la"), Some("ls".into()));
}

#[test]
fn test_get_command_prefix_rejects_shell() {
    assert_eq!(get_command_prefix("bash -c 'evil'"), None);
    assert_eq!(get_command_prefix("sudo rm -rf /"), None);
    assert_eq!(get_command_prefix("env malicious"), None);
}

#[test]
fn test_split_compound() {
    let parts = split_compound_command("git add . && git commit -m msg");
    assert_eq!(parts, vec!["git add .", "git commit -m msg"]);
}

#[test]
fn test_split_pipe() {
    let parts = split_compound_command("cat file | grep pattern");
    assert_eq!(parts, vec!["cat file", "grep pattern"]);
}

#[test]
fn test_split_semicolon() {
    let parts = split_compound_command("cd /tmp; ls");
    assert_eq!(parts, vec!["cd /tmp", "ls"]);
}

#[test]
fn test_split_preserves_quotes() {
    let parts = split_compound_command("echo 'hello && world'");
    assert_eq!(parts, vec!["echo 'hello && world'"]);
}

#[test]
fn test_strip_all_env_vars() {
    assert_eq!(
        strip_all_env_vars("FOO=bar BAZ=qux command", /*check_hijack*/ false),
        "command"
    );
}

#[test]
fn test_strip_all_env_vars_blocks_hijack() {
    let result = strip_all_env_vars("LD_PRELOAD=/evil.so command", /*check_hijack*/ true);
    assert_eq!(result, "LD_PRELOAD=/evil.so command");
}

#[test]
fn test_is_dangerous_bare_prefix() {
    assert!(is_dangerous_bare_prefix("bash"));
    assert!(is_dangerous_bare_prefix("sudo"));
    assert!(is_dangerous_bare_prefix("env"));
    assert!(!is_dangerous_bare_prefix("git"));
    assert!(!is_dangerous_bare_prefix("cargo"));
}
