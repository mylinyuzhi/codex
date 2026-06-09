use super::*;

#[test]
fn test_check_dangerous_path_root() {
    assert!(check_dangerous_path("rm", "/", "/home/user/project").is_some());
    assert!(check_dangerous_path("rm", "/etc", "/home/user/project").is_some());
    assert!(check_dangerous_path("rm", "/usr", "/home/user/project").is_some());
}

#[test]
fn test_check_dangerous_path_safe() {
    assert!(check_dangerous_path("rm", "file.txt", "/home/user/project").is_none());
    assert!(check_dangerous_path("rm", "/tmp/test", "/home/user/project").is_none());
}

#[test]
fn test_filter_flags() {
    assert_eq!(filter_flags(&["-la", "dir"]), vec!["dir"]);
    assert_eq!(filter_flags(&["--", "-file"]), vec!["-file"]);
}

#[test]
fn test_extract_find_paths() {
    assert_eq!(extract_find_paths(&[".", "-name", "*.rs"]), vec!["."]);
    assert_eq!(
        extract_find_paths(&["/src", "/lib", "-type", "f"]),
        vec!["/src", "/lib"]
    );
}

#[test]
fn test_extract_pattern_paths() {
    assert_eq!(
        extract_pattern_command_paths(&["pattern", "file1.rs", "file2.rs"]),
        vec!["file1.rs", "file2.rs"]
    );
}

#[test]
fn test_expand_home() {
    let expanded = expand_home("~/Documents");
    assert!(expanded.ends_with("/Documents"));
    assert!(!expanded.starts_with('~'));
}

// ── force-ask gates (P4/P15) ──

#[test]
fn test_check_dangerous_removal() {
    // Catastrophic removals → force-ask (even compounded / wrapped).
    assert!(check_dangerous_removal("rm -rf /", "/home/u/proj").is_some());
    assert!(check_dangerous_removal("rm -rf /etc", "/home/u/proj").is_some());
    assert!(check_dangerous_removal("ls && rm -rf /usr", "/home/u/proj").is_some());
    // Safe removals under cwd → no gate.
    assert!(check_dangerous_removal("rm -rf build", "/home/u/proj").is_none());
    assert!(check_dangerous_removal("rm foo.txt", "/home/u/proj").is_none());
    // Non-removal commands → no gate.
    assert!(check_dangerous_removal("ls /etc", "/home/u/proj").is_none());
}

#[test]
fn test_has_git_escape_pattern() {
    // cd + git compound → escape pattern.
    assert!(has_git_escape_pattern("cd /tmp/x && git status"));
    // mkdir of a git-internal dir then git → escape.
    assert!(has_git_escape_pattern("mkdir refs && git init"));
    // Plain git / plain cd → not an escape.
    assert!(!has_git_escape_pattern("git status"));
    assert!(!has_git_escape_pattern("cd /tmp/x && ls"));
}
