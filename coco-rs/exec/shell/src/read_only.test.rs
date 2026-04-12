use super::*;

// ── Always-safe commands ──

#[test]
fn test_basic_safe_commands() {
    for cmd in &[
        "cat foo.txt",
        "ls -la",
        "head -20 file.rs",
        "tail -f log.txt",
        "wc -l src/*.rs",
        "grep pattern file",
        "echo hello",
        "pwd",
        "whoami",
        "uname -a",
        "which cargo",
        "stat file.txt",
        "diff a.txt b.txt",
        "env",
        "printenv HOME",
        "hostname",
        "date",
        "ps aux",
    ] {
        assert!(is_read_only_command(cmd), "expected safe: {cmd}");
    }
}

// ── git subcommands ──

#[test]
fn test_git_safe_subcommands() {
    assert!(is_read_only_command("git status"));
    assert!(is_read_only_command("git log --oneline"));
    assert!(is_read_only_command("git diff HEAD~1"));
    assert!(is_read_only_command("git show HEAD"));
    assert!(is_read_only_command("git branch --list"));
    assert!(is_read_only_command("git branch -a"));
    assert!(is_read_only_command("git branch -vv"));
    assert!(is_read_only_command("git branch --show-current"));
}

#[test]
fn test_git_unsafe_subcommands() {
    assert!(!is_read_only_command("git push"));
    assert!(!is_read_only_command("git commit -m msg"));
    assert!(!is_read_only_command("git checkout -b new-branch"));
    assert!(!is_read_only_command("git merge main"));
    assert!(!is_read_only_command("git rebase main"));
    assert!(!is_read_only_command("git reset --hard"));
}

#[test]
fn test_git_branch_mutating() {
    assert!(!is_read_only_command("git branch new-branch"));
    assert!(!is_read_only_command("git branch -d old-branch"));
    assert!(!is_read_only_command("git branch -D old-branch"));
}

#[test]
fn test_git_config_override_blocks() {
    assert!(!is_read_only_command("git -c core.pager=cat status"));
    assert!(!is_read_only_command("git --config-env=foo=bar status"));
}

#[test]
fn test_git_unsafe_output_flags() {
    assert!(!is_read_only_command("git diff --output=patch.diff"));
    assert!(!is_read_only_command("git log --exec=/bin/sh"));
}

// ── find command ──

#[test]
fn test_find_safe() {
    assert!(is_read_only_command("find . -name *.rs"));
    assert!(is_read_only_command("find /tmp -type f"));
}

#[test]
fn test_find_unsafe() {
    assert!(!is_read_only_command("find . -exec rm {} ;"));
    assert!(!is_read_only_command("find . -delete"));
    assert!(!is_read_only_command("find . -execdir cat {} ;"));
}

// ── base64 ──

#[test]
fn test_base64_safe() {
    assert!(is_read_only_command("base64 file.txt"));
    assert!(is_read_only_command("base64 -d encoded.txt"));
}

#[test]
fn test_base64_unsafe() {
    assert!(!is_read_only_command("base64 -o output.txt file.txt"));
    assert!(!is_read_only_command("base64 --output=out.txt file"));
}

// ── rg (ripgrep) ──

#[test]
fn test_rg_safe() {
    assert!(is_read_only_command("rg pattern"));
    assert!(is_read_only_command("rg -i pattern src/"));
}

#[test]
fn test_rg_unsafe() {
    assert!(!is_read_only_command("rg --pre=cat pattern"));
    assert!(!is_read_only_command("rg --search-zip pattern"));
    assert!(!is_read_only_command("rg -z pattern"));
}

// ── sed ──

#[test]
fn test_sed_safe_print() {
    assert!(is_read_only_command("sed -n 5p file.txt"));
    assert!(is_read_only_command("sed -n 1,10p file.txt"));
}

#[test]
fn test_sed_unsafe() {
    assert!(!is_read_only_command("sed -i s/old/new/ file.txt"));
    assert!(!is_read_only_command("sed s/old/new/ file.txt"));
}

// ── curl/wget ──

#[test]
fn test_curl_safe() {
    assert!(is_read_only_command("curl https://example.com"));
    assert!(is_read_only_command("curl -s https://api.example.com"));
}

#[test]
fn test_curl_unsafe() {
    assert!(!is_read_only_command(
        "curl -X POST https://api.example.com"
    ));
    assert!(!is_read_only_command(
        "curl -d data https://api.example.com"
    ));
    assert!(!is_read_only_command(
        "curl -o output.html https://example.com"
    ));
}

// ── Development tools ──

#[test]
fn test_cargo_safe() {
    assert!(is_read_only_command("cargo check"));
    assert!(is_read_only_command("cargo test"));
    assert!(is_read_only_command("cargo clippy"));
}

#[test]
fn test_cargo_unsafe() {
    assert!(!is_read_only_command("cargo install foo"));
    assert!(!is_read_only_command("cargo publish"));
}

#[test]
fn test_docker_safe() {
    assert!(is_read_only_command("docker ps"));
    assert!(is_read_only_command("docker images"));
    assert!(is_read_only_command("docker logs container_id"));
}

#[test]
fn test_docker_unsafe() {
    assert!(!is_read_only_command("docker run ubuntu"));
    assert!(!is_read_only_command("docker rm container_id"));
}

// ── Edge cases ──

#[test]
fn test_empty_command() {
    assert!(!is_read_only_command(""));
    assert!(!is_read_only_command("   "));
}

#[test]
fn test_unknown_command() {
    assert!(!is_read_only_command("rm file.txt"));
    assert!(!is_read_only_command("mv a.txt b.txt"));
    assert!(!is_read_only_command("cp a.txt b.txt"));
    assert!(!is_read_only_command("chmod 644 file"));
}

#[test]
fn test_executable_name_extraction() {
    assert_eq!(executable_name("/usr/bin/git"), "git");
    assert_eq!(executable_name("git"), "git");
    assert_eq!(executable_name("/bin/cat"), "cat");
}
