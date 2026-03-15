use super::*;

#[test]
fn test_env_files() {
    assert!(is_sensitive_file(Path::new(".env")));
    assert!(is_sensitive_file(Path::new("/home/user/.env")));
    assert!(is_sensitive_file(Path::new(".env.local")));
    assert!(is_sensitive_file(Path::new(".env.production")));
    assert!(!is_sensitive_file(Path::new("src/main.rs")));
}

#[test]
fn test_key_files() {
    assert!(is_sensitive_file(Path::new("server.pem")));
    assert!(is_sensitive_file(Path::new("private.key")));
    assert!(is_sensitive_file(Path::new("credentials.json")));
}

#[test]
fn test_shell_configs() {
    assert!(is_sensitive_file(Path::new("/home/user/.bashrc")));
    assert!(is_sensitive_file(Path::new(".zshrc")));
    assert!(is_sensitive_file(Path::new(".profile")));
}

#[test]
fn test_ssh_files() {
    assert!(is_sensitive_file(Path::new("/home/user/.ssh/config")));
    assert!(is_sensitive_file(Path::new(".ssh/id_rsa")));
    assert!(is_sensitive_file(Path::new(".ssh/id_ed25519")));
    assert!(is_sensitive_file(Path::new(".ssh/authorized_keys")));
}

#[test]
fn test_cicd() {
    assert!(is_sensitive_file(Path::new(".github/workflows/deploy.yml")));
    assert!(!is_sensitive_file(Path::new(".github/CODEOWNERS")));
}

#[test]
fn test_locked_directories() {
    assert!(is_locked_directory(Path::new(".claude/settings.json")));
    assert!(is_locked_directory(Path::new(".claude/commands/my-cmd")));
    assert!(is_locked_directory(Path::new(".claude/agents/my-agent")));
    assert!(is_locked_directory(Path::new(".claude/skills/my-skill")));
    assert!(!is_locked_directory(Path::new("src/main.rs")));
}

#[test]
fn test_sensitive_directories() {
    assert!(is_sensitive_directory(Path::new(".git/config")));
    assert!(is_sensitive_directory(Path::new(".vscode/settings.json")));
    assert!(is_sensitive_directory(Path::new(".idea/workspace.xml")));
    assert!(!is_sensitive_directory(Path::new("src/main.rs")));
}

#[test]
fn test_is_outside_cwd() {
    let cwd = Path::new("/home/user/project");
    assert!(!is_outside_cwd(
        Path::new("/home/user/project/src/main.rs"),
        cwd
    ));
    assert!(is_outside_cwd(Path::new("/etc/passwd"), cwd));
    assert!(is_outside_cwd(Path::new("/home/user/other/file.txt"), cwd));
}

#[test]
fn test_new_sensitive_patterns() {
    assert!(is_sensitive_file(Path::new(".gitmodules")));
    assert!(is_sensitive_file(Path::new(".ripgreprc")));
    assert!(is_sensitive_file(Path::new(".zprofile")));
}

#[test]
fn test_service_account() {
    assert!(is_sensitive_file(Path::new("service-account.json")));
    assert!(is_sensitive_file(Path::new("service-account-prod.json")));
    assert!(!is_sensitive_file(Path::new("service-info.json")));
}

#[test]
fn test_normal_files_not_sensitive() {
    assert!(!is_sensitive_file(Path::new("src/main.rs")));
    assert!(!is_sensitive_file(Path::new("Cargo.toml")));
    assert!(!is_sensitive_file(Path::new("README.md")));
    assert!(!is_sensitive_file(Path::new("package.json")));
}
