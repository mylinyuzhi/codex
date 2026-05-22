use super::*;

#[test]
fn test_accept_edits_auto_allow() {
    assert!(is_auto_allowed_in_accept_edits("mkdir -p src/new"));
    assert!(is_auto_allowed_in_accept_edits("touch new_file.rs"));
    assert!(is_auto_allowed_in_accept_edits("rm old_file.rs"));
    assert!(is_auto_allowed_in_accept_edits("rmdir empty_dir"));
    assert!(is_auto_allowed_in_accept_edits("mv old.rs new.rs"));
    assert!(is_auto_allowed_in_accept_edits("cp template.rs new.rs"));
    assert!(is_auto_allowed_in_accept_edits(
        "sed -i 's/old/new/' file.rs"
    ));
}

#[test]
fn test_accept_edits_rejects_non_file_commands() {
    assert!(!is_auto_allowed_in_accept_edits("git push"));
    assert!(!is_auto_allowed_in_accept_edits("curl https://example.com"));
    assert!(!is_auto_allowed_in_accept_edits("cargo build"));
    assert!(!is_auto_allowed_in_accept_edits("echo hello"));
    assert!(!is_auto_allowed_in_accept_edits("chmod 755 file"));
}

#[test]
fn test_accept_edits_with_full_path() {
    assert!(is_auto_allowed_in_accept_edits("/bin/mkdir -p dir"));
    assert!(is_auto_allowed_in_accept_edits("/usr/bin/touch file"));
}

#[test]
fn test_accept_edits_with_env_vars() {
    assert!(is_auto_allowed_in_accept_edits("FOO=bar mkdir -p dir"));
}

#[test]
fn test_accept_edits_empty() {
    assert!(!is_auto_allowed_in_accept_edits(""));
    assert!(!is_auto_allowed_in_accept_edits("   "));
}
