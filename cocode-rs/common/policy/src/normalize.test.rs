use super::*;

#[test]
fn test_no_normalization_needed() {
    assert_eq!(normalize_command("git push"), "git push");
    assert_eq!(normalize_command("npm run test"), "npm run test");
}

#[test]
fn test_strip_single_env_var() {
    assert_eq!(normalize_command("LANG=C git push"), "git push");
    assert_eq!(normalize_command("CC=/usr/bin/gcc make"), "make");
}

#[test]
fn test_strip_multiple_env_vars() {
    assert_eq!(normalize_command("LANG=C LC_ALL=C git push"), "git push");
    assert_eq!(normalize_command("FOO=bar BAZ=qux npm test"), "npm test");
}

#[test]
fn test_strip_timeout() {
    assert_eq!(normalize_command("timeout 30 git push"), "git push");
    assert_eq!(normalize_command("timeout -k 5 30 git push"), "git push");
}

#[test]
fn test_strip_nice() {
    assert_eq!(normalize_command("nice -n 10 npm test"), "npm test");
    assert_eq!(normalize_command("nice -10 npm test"), "npm test");
    assert_eq!(normalize_command("nice npm test"), "npm test");
}

#[test]
fn test_strip_nohup() {
    assert_eq!(normalize_command("nohup git pull"), "git pull");
}

#[test]
fn test_strip_time() {
    assert_eq!(normalize_command("time git status"), "git status");
    assert_eq!(normalize_command("time -p git status"), "git status");
}

#[test]
fn test_strip_env_command() {
    assert_eq!(normalize_command("env git push"), "git push");
    assert_eq!(normalize_command("env FOO=bar git push"), "git push");
    assert_eq!(
        normalize_command("env -u HOME FOO=bar git push"),
        "git push"
    );
}

#[test]
fn test_strip_command() {
    assert_eq!(normalize_command("command git push"), "git push");
    assert_eq!(normalize_command("command -v git"), "git");
    assert_eq!(normalize_command("command -p git push"), "git push");
}

#[test]
fn test_combined_env_and_wrapper() {
    assert_eq!(normalize_command("LANG=C timeout 5 git push"), "git push");
    assert_eq!(
        normalize_command("nice -n 10 env FOO=bar npm test"),
        "npm test"
    );
}

#[test]
fn test_no_strip_env_var_only() {
    // If the command is ONLY an env var with no command, don't strip.
    assert_eq!(normalize_command("FOO=bar"), "FOO=bar");
}

#[test]
fn test_preserves_whitespace_trimmed() {
    assert_eq!(normalize_command("  git push  "), "git push");
}

#[test]
fn test_empty_command() {
    assert_eq!(normalize_command(""), "");
    assert_eq!(normalize_command("  "), "");
}

#[test]
fn test_non_wrapper_not_stripped() {
    assert_eq!(normalize_command("sudo git push"), "sudo git push");
    assert_eq!(
        normalize_command("bash -c 'git push'"),
        "bash -c 'git push'"
    );
}
