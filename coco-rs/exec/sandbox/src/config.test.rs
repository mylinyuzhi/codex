use std::path::PathBuf;

use super::*;

// ==========================================================================
// SandboxBypass tests
// ==========================================================================

#[test]
fn test_sandbox_bypass_from_flag() {
    assert_eq!(SandboxBypass::from_flag(false), SandboxBypass::No);
    assert_eq!(SandboxBypass::from_flag(true), SandboxBypass::Requested);
}

// ==========================================================================
// EnforcementLevel tests
// ==========================================================================

#[test]
fn test_enforcement_level_default() {
    assert_eq!(EnforcementLevel::default(), EnforcementLevel::Disabled);
}

#[test]
fn test_enforcement_level_serde_roundtrip() {
    for level in [
        EnforcementLevel::Disabled,
        EnforcementLevel::ReadOnly,
        EnforcementLevel::WorkspaceWrite,
        EnforcementLevel::Strict,
    ] {
        let json = serde_json::to_string(&level).expect("serialize");
        let parsed: EnforcementLevel = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, level);
    }
}

#[test]
fn test_enforcement_level_kebab_case() {
    assert_eq!(
        serde_json::to_string(&EnforcementLevel::Disabled).expect("serialize"),
        "\"disabled\""
    );
    assert_eq!(
        serde_json::to_string(&EnforcementLevel::ReadOnly).expect("serialize"),
        "\"read-only\""
    );
    assert_eq!(
        serde_json::to_string(&EnforcementLevel::WorkspaceWrite).expect("serialize"),
        "\"workspace-write\""
    );
    assert_eq!(
        serde_json::to_string(&EnforcementLevel::Strict).expect("serialize"),
        "\"strict\""
    );
}

#[test]
fn test_enforcement_level_from_protocol() {
    assert_eq!(
        EnforcementLevel::from(SandboxMode::ReadOnly),
        EnforcementLevel::ReadOnly
    );
    assert_eq!(
        EnforcementLevel::from(SandboxMode::WorkspaceWrite),
        EnforcementLevel::WorkspaceWrite
    );
    assert_eq!(
        EnforcementLevel::from(SandboxMode::FullAccess),
        EnforcementLevel::Disabled
    );
    assert_eq!(
        EnforcementLevel::from(SandboxMode::ExternalSandbox),
        EnforcementLevel::WorkspaceWrite
    );
}

// ==========================================================================
// WritableRoot tests
// ==========================================================================

#[test]
fn test_writable_root_default_subpaths() {
    let root = WritableRoot::new("/home/user/project");
    assert_eq!(root.readonly_subpaths, vec![".git", ".coco", ".agents"]);
}

#[test]
fn test_writable_root_is_writable() {
    let root = WritableRoot::new("/home/user/project");
    // Normal files under root are writable
    assert!(root.is_writable(Path::new("/home/user/project/src/main.rs")));
    // .git subpath is read-only
    assert!(!root.is_writable(Path::new("/home/user/project/.git/config")));
    assert!(!root.is_writable(Path::new("/home/user/project/.git")));
    // .coco subpath is read-only
    assert!(!root.is_writable(Path::new("/home/user/project/.coco/config.json")));
    // .agents subpath is read-only
    assert!(!root.is_writable(Path::new("/home/user/project/.agents/skills")));
    // Paths outside root are not writable
    assert!(!root.is_writable(Path::new("/etc/passwd")));
}

#[test]
fn test_writable_root_resolved_readonly_subpaths() {
    let root = WritableRoot::new("/home/user/project");
    let resolved = root.resolved_readonly_subpaths();
    assert_eq!(resolved.len(), 3);
    assert_eq!(resolved[0], Path::new("/home/user/project/.git"));
    assert_eq!(resolved[1], Path::new("/home/user/project/.coco"));
    assert_eq!(resolved[2], Path::new("/home/user/project/.agents"));
}

#[test]
fn test_writable_root_unprotected() {
    let root = WritableRoot::unprotected("/tmp/work");
    assert!(root.is_writable(Path::new("/tmp/work/.git/config")));
    assert!(root.is_writable(Path::new("/tmp/work/file.txt")));
}

#[test]
fn test_writable_root_contains() {
    let root = WritableRoot::new("/home/user/project");
    assert!(root.contains(Path::new("/home/user/project/src")));
    assert!(root.contains(Path::new("/home/user/project/.git")));
    assert!(!root.contains(Path::new("/home/user/other")));
}

#[test]
fn test_writable_root_serde_roundtrip() {
    let root = WritableRoot::new("/home/user/project");
    let json = serde_json::to_string(&root).expect("serialize");
    let parsed: WritableRoot = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed, root);
}

#[test]
fn test_writable_root_serde_default_subpaths() {
    // JSON without readonly_subpaths should use defaults
    let json = r#"{"path":"/tmp/work"}"#;
    let parsed: WritableRoot = serde_json::from_str(json).expect("parse");
    assert_eq!(parsed.readonly_subpaths, vec![".git", ".coco", ".agents"]);
}

// ==========================================================================
// SandboxConfig tests
// ==========================================================================

#[test]
fn test_sandbox_config_default() {
    let config = SandboxConfig::default();
    assert_eq!(config.enforcement, EnforcementLevel::Disabled);
    assert!(config.writable_roots.is_empty());
    assert!(config.denied_paths.is_empty());
    assert!(!config.allow_network);
}

#[test]
fn test_sandbox_config_serde_roundtrip() {
    let config = SandboxConfig {
        enforcement: EnforcementLevel::Strict,
        writable_roots: vec![WritableRoot::new("/home/user/project")],
        denied_paths: vec![PathBuf::from("/etc/passwd")],
        allow_network: true,
        ..Default::default()
    };

    let json = serde_json::to_string(&config).expect("serialize");
    let parsed: SandboxConfig = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed.enforcement, EnforcementLevel::Strict);
    assert_eq!(parsed.writable_roots.len(), 1);
    assert_eq!(parsed.denied_paths.len(), 1);
    assert!(parsed.allow_network);
}

#[test]
fn test_sandbox_config_from_empty_json() {
    let config: SandboxConfig = serde_json::from_str("{}").expect("parse");
    assert_eq!(config.enforcement, EnforcementLevel::Disabled);
    assert!(config.writable_roots.is_empty());
    assert!(config.denied_paths.is_empty());
    assert!(!config.allow_network);
}

#[test]
fn test_sandbox_config_partial_json() {
    let config: SandboxConfig = serde_json::from_str(r#"{"enforcement":"strict"}"#).expect("parse");
    assert_eq!(config.enforcement, EnforcementLevel::Strict);
    assert!(config.writable_roots.is_empty());
    assert!(!config.allow_network);
}

// ==========================================================================
// SandboxSettings tests
// ==========================================================================

#[test]
fn test_sandbox_settings_default_disabled() {
    let settings = SandboxSettings::default();
    assert!(!settings.enabled);
    assert!(settings.auto_allow_bash_if_sandboxed);
    assert!(settings.allow_unsandboxed_commands);
    assert_eq!(
        settings.enabled_platforms,
        vec!["macos", "linux", "windows"]
    );
    assert!(settings.excluded_commands.is_empty());
    assert!(settings.network.allowed_domains.is_empty());
    assert!(settings.network.denied_domains.is_empty());
    assert!(settings.filesystem.allow_write.is_empty());
    assert!(settings.filesystem.deny_write.is_empty());
    assert!(settings.filesystem.deny_read.is_empty());
    assert!(!settings.filesystem.allow_git_config);
    assert!(settings.ignore_violations.is_empty());
    assert!(!settings.enable_weaker_nested_sandbox);
    assert!(!settings.enable_weaker_network_isolation);
    assert!(!settings.allow_pty);
    assert_eq!(settings.mandatory_deny_search_depth, 3);
}

#[test]
fn test_sandbox_settings_enabled_constructor() {
    let settings = SandboxSettings::enabled();
    assert!(settings.enabled);
    assert!(settings.auto_allow_bash_if_sandboxed);
    assert!(settings.allow_unsandboxed_commands);
}

#[test]
fn test_sandbox_settings_disabled_constructor() {
    let settings = SandboxSettings::disabled();
    assert!(!settings.enabled);
}

#[test]
fn test_is_sandboxed_disabled_by_default() {
    let settings = SandboxSettings::default();
    assert!(!settings.is_sandboxed("echo hello", SandboxBypass::No));
    assert!(!settings.is_sandboxed("rm -rf /", SandboxBypass::No));
    assert!(!settings.is_sandboxed("echo hello", SandboxBypass::Requested));
}

#[test]
fn test_is_sandboxed_enabled() {
    let settings = SandboxSettings::enabled();
    assert!(settings.is_sandboxed("echo hello", SandboxBypass::No));
    assert!(settings.is_sandboxed("rm -rf /", SandboxBypass::No));
}

#[test]
fn test_is_sandboxed_bypass_allowed() {
    let settings = SandboxSettings::enabled();
    assert!(!settings.is_sandboxed("echo hello", SandboxBypass::Requested));
}

#[test]
fn test_is_sandboxed_bypass_disallowed() {
    let mut settings = SandboxSettings::enabled();
    settings.allow_unsandboxed_commands = false;
    assert!(settings.is_sandboxed("echo hello", SandboxBypass::Requested));
}

#[test]
fn test_is_sandboxed_empty_command() {
    let settings = SandboxSettings::enabled();
    assert!(!settings.is_sandboxed("", SandboxBypass::No));
    assert!(!settings.is_sandboxed("   ", SandboxBypass::No));
    assert!(!settings.is_sandboxed("\t\n", SandboxBypass::No));
}

#[test]
fn test_is_sandboxed_excluded_exact() {
    let mut settings = SandboxSettings::enabled();
    settings.excluded_commands = vec!["docker".to_string()];
    // "docker" alone matches
    assert!(!settings.is_sandboxed("docker", SandboxBypass::No));
    // "docker ps" matches (prefix + space)
    assert!(!settings.is_sandboxed("docker ps", SandboxBypass::No));
    // "dockerize" does NOT match (no space separator)
    assert!(settings.is_sandboxed("dockerize", SandboxBypass::No));
}

#[test]
fn test_is_sandboxed_excluded_wildcard() {
    let mut settings = SandboxSettings::enabled();
    settings.excluded_commands = vec!["git*".to_string()];
    assert!(!settings.is_sandboxed("git status", SandboxBypass::No));
    assert!(!settings.is_sandboxed("git", SandboxBypass::No));
    assert!(!settings.is_sandboxed("gitk", SandboxBypass::No));
}

#[test]
fn test_is_sandboxed_excluded_phrase() {
    let mut settings = SandboxSettings::enabled();
    settings.excluded_commands = vec!["git push".to_string()];
    assert!(!settings.is_sandboxed("git push origin main", SandboxBypass::No));
    assert!(settings.is_sandboxed("git status", SandboxBypass::No));
}

// ==========================================================================
// BFS Command Exclusion tests
// ==========================================================================

#[test]
fn test_excluded_env_stripped() {
    let mut settings = SandboxSettings::enabled();
    settings.excluded_commands = vec!["npm".to_string()];
    // FOO=bar npm install -> strips env -> "npm install" matches "npm"
    assert!(!settings.is_sandboxed("FOO=bar npm install", SandboxBypass::No));
    assert!(!settings.is_sandboxed("A=1 B=2 npm run build", SandboxBypass::No));
    // Not a match when the base command differs
    assert!(settings.is_sandboxed("FOO=bar yarn install", SandboxBypass::No));
}

#[test]
fn test_excluded_basename_extraction() {
    let mut settings = SandboxSettings::enabled();
    settings.excluded_commands = vec!["npm".to_string()];
    // /usr/bin/npm -> basename "npm" matches
    assert!(!settings.is_sandboxed("/usr/bin/npm install", SandboxBypass::No));
    assert!(!settings.is_sandboxed("./node_modules/.bin/npm run test", SandboxBypass::No));
}

#[test]
fn test_excluded_env_and_basename_combined() {
    let mut settings = SandboxSettings::enabled();
    settings.excluded_commands = vec!["npm".to_string()];
    // FOO=bar /usr/bin/npm install -> strip env -> /usr/bin/npm install -> basename -> npm install
    assert!(!settings.is_sandboxed("FOO=bar /usr/bin/npm install", SandboxBypass::No));
}

#[test]
fn test_excluded_colon_prefix_pattern() {
    let mut settings = SandboxSettings::enabled();
    settings.excluded_commands = vec!["npm:*".to_string()];
    assert!(!settings.is_sandboxed("npm", SandboxBypass::No));
    assert!(!settings.is_sandboxed("npm install", SandboxBypass::No));
    assert!(!settings.is_sandboxed("npm run build", SandboxBypass::No));
    // "npmx" should not match (colon-prefix requires exact first word)
    assert!(settings.is_sandboxed("npmx", SandboxBypass::No));
}

#[test]
fn test_excluded_empty_list() {
    let settings = SandboxSettings::enabled();
    // No excluded commands -> everything sandboxed
    assert!(settings.is_sandboxed("npm install", SandboxBypass::No));
}

#[test]
fn test_excluded_does_not_match_unrelated_env_command() {
    let mut settings = SandboxSettings::enabled();
    settings.excluded_commands = vec!["yarn".to_string()];
    // "FOO=bar npm install" strips env to "npm install" which doesn't match "yarn"
    assert!(settings.is_sandboxed("FOO=bar npm install", SandboxBypass::No));
    // But "FOO=bar yarn install" strips to "yarn install" which matches
    assert!(!settings.is_sandboxed("FOO=bar yarn install", SandboxBypass::No));
}

#[test]
fn test_is_platform_enabled() {
    let settings = SandboxSettings::default();
    // On any supported platform (macos or linux), this should be true
    if cfg!(target_os = "macos") || cfg!(target_os = "linux") {
        assert!(settings.is_platform_enabled());
    }

    let empty = SandboxSettings {
        enabled_platforms: vec![],
        ..Default::default()
    };
    assert!(!empty.is_platform_enabled());
}

#[test]
fn test_sandbox_settings_serde_roundtrip() {
    let settings = SandboxSettings {
        enabled: true,
        auto_allow_bash_if_sandboxed: false,
        allow_unsandboxed_commands: false,
        enabled_platforms: vec!["linux".to_string()],
        excluded_commands: vec!["docker".to_string()],
        network: NetworkConfig {
            allowed_domains: vec!["github.com".to_string()],
            denied_domains: vec!["evil.com".to_string()],
            ..Default::default()
        },
        filesystem: FilesystemConfig {
            deny_read: vec![PathBuf::from("/etc/shadow")],
            allow_git_config: true,
            ..Default::default()
        },
        ..Default::default()
    };

    let json = serde_json::to_string(&settings).expect("serialize");
    let parsed: SandboxSettings = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed.enabled, settings.enabled);
    assert_eq!(
        parsed.auto_allow_bash_if_sandboxed,
        settings.auto_allow_bash_if_sandboxed
    );
    assert_eq!(
        parsed.allow_unsandboxed_commands,
        settings.allow_unsandboxed_commands
    );
    assert_eq!(parsed.enabled_platforms, settings.enabled_platforms);
    assert_eq!(parsed.excluded_commands, settings.excluded_commands);
    assert_eq!(
        parsed.network.allowed_domains,
        settings.network.allowed_domains
    );
    assert_eq!(
        parsed.network.denied_domains,
        settings.network.denied_domains
    );
    assert_eq!(parsed.filesystem.deny_read, settings.filesystem.deny_read);
    assert!(parsed.filesystem.allow_git_config);
}

#[test]
fn test_sandbox_settings_from_empty_json() {
    let settings: SandboxSettings = serde_json::from_str("{}").expect("parse");
    assert!(!settings.enabled);
    assert!(settings.auto_allow_bash_if_sandboxed);
    assert!(settings.allow_unsandboxed_commands);
    assert_eq!(
        settings.enabled_platforms,
        vec!["macos", "linux", "windows"]
    );
    assert!(settings.network.allowed_domains.is_empty());
    assert!(settings.filesystem.deny_read.is_empty());
    assert_eq!(settings.mandatory_deny_search_depth, 3);
}

#[test]
fn test_sandbox_settings_partial_json() {
    let settings: SandboxSettings = serde_json::from_str(r#"{"enabled":true}"#).expect("parse");
    assert!(settings.enabled);
    assert!(settings.auto_allow_bash_if_sandboxed);
    assert!(settings.allow_unsandboxed_commands);
}

// ==========================================================================
// FilesystemConfig tests
// ==========================================================================

#[test]
fn test_filesystem_config_default() {
    let config = FilesystemConfig::default();
    assert!(config.allow_write.is_empty());
    assert!(config.deny_write.is_empty());
    assert!(config.deny_read.is_empty());
    assert!(!config.allow_git_config);
}

#[test]
fn test_filesystem_config_serde_roundtrip() {
    let config = FilesystemConfig {
        allow_write: vec![PathBuf::from("/home/user/project")],
        deny_write: vec![PathBuf::from("/etc")],
        deny_read: vec![PathBuf::from("/etc/shadow")],
        allow_git_config: true,
    };
    let json = serde_json::to_string(&config).expect("serialize");
    let parsed: FilesystemConfig = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed, config);
}

#[test]
fn test_filesystem_config_from_empty_json() {
    let config: FilesystemConfig = serde_json::from_str("{}").expect("parse");
    assert!(config.allow_write.is_empty());
    assert!(!config.allow_git_config);
}

// ==========================================================================
// NetworkConfig tests
// ==========================================================================

#[test]
fn test_network_config_default() {
    let config = NetworkConfig::default();
    assert_eq!(config.mode, NetworkMode::Full);
    assert!(config.allowed_domains.is_empty());
    assert!(config.denied_domains.is_empty());
    assert!(config.allow_unix_sockets.is_empty());
    assert!(!config.allow_all_unix_sockets);
    assert!(!config.allow_local_binding);
    assert!(config.http_proxy_port.is_none());
    assert!(config.socks_proxy_port.is_none());
    assert!(config.mitm_proxy.is_none());
}

#[test]
fn test_network_mode_allows_method() {
    assert!(NetworkMode::Full.allows_method("GET"));
    assert!(NetworkMode::Full.allows_method("POST"));
    assert!(NetworkMode::Full.allows_method("CONNECT"));

    assert!(NetworkMode::Limited.allows_method("GET"));
    assert!(NetworkMode::Limited.allows_method("HEAD"));
    assert!(NetworkMode::Limited.allows_method("OPTIONS"));
    assert!(!NetworkMode::Limited.allows_method("POST"));
    assert!(!NetworkMode::Limited.allows_method("PUT"));
    assert!(!NetworkMode::Limited.allows_method("DELETE"));
    assert!(!NetworkMode::Limited.allows_method("PATCH"));
    assert!(!NetworkMode::Limited.allows_method("CONNECT"));
}

#[test]
fn test_network_mode_serde() {
    let json_full = r#""full""#;
    let json_limited = r#""limited""#;
    assert_eq!(
        serde_json::from_str::<NetworkMode>(json_full).expect("parse full"),
        NetworkMode::Full
    );
    assert_eq!(
        serde_json::from_str::<NetworkMode>(json_limited).expect("parse limited"),
        NetworkMode::Limited
    );
    assert_eq!(
        serde_json::to_string(&NetworkMode::Full).expect("ser"),
        json_full
    );
    assert_eq!(
        serde_json::to_string(&NetworkMode::Limited).expect("ser"),
        json_limited
    );
}

#[test]
fn test_network_config_serde_roundtrip() {
    let config = NetworkConfig {
        mode: NetworkMode::Limited,
        allowed_domains: vec!["github.com".to_string()],
        denied_domains: vec!["evil.com".to_string()],
        allow_unix_sockets: vec![PathBuf::from("/var/run/docker.sock")],
        allow_all_unix_sockets: false,
        allow_local_binding: true,
        http_proxy_port: Some(3128),
        socks_proxy_port: Some(1080),
        mitm_proxy: Some(MitmProxyConfig {
            socket_path: PathBuf::from("/tmp/mitm.sock"),
            domains: vec!["api.example.com".to_string()],
        }),
        block_non_public_ips: true,
    };
    let json = serde_json::to_string(&config).expect("serialize");
    let parsed: NetworkConfig = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed, config);
}

#[test]
fn test_network_config_from_empty_json() {
    let config: NetworkConfig = serde_json::from_str("{}").expect("parse");
    assert!(config.allowed_domains.is_empty());
    assert!(config.http_proxy_port.is_none());
}

// ==========================================================================
// Nested config in SandboxSettings tests
// ==========================================================================

#[test]
fn test_sandbox_settings_nested_json() {
    let json = r#"{
        "enabled": true,
        "filesystem": {
            "allow_write": ["/home/user/project"],
            "deny_read": ["/etc/shadow"],
            "allow_git_config": true
        },
        "network": {
            "allowed_domains": ["github.com"],
            "allow_local_binding": true,
            "http_proxy_port": 3128
        },
        "allow_pty": true,
        "mandatory_deny_search_depth": 5
    }"#;

    let settings: SandboxSettings = serde_json::from_str(json).expect("parse");
    assert!(settings.enabled);
    assert_eq!(
        settings.filesystem.allow_write,
        vec![PathBuf::from("/home/user/project")]
    );
    assert_eq!(
        settings.filesystem.deny_read,
        vec![PathBuf::from("/etc/shadow")]
    );
    assert!(settings.filesystem.allow_git_config);
    assert_eq!(
        settings.network.allowed_domains,
        vec!["github.com".to_string()]
    );
    assert!(settings.network.allow_local_binding);
    assert_eq!(settings.network.http_proxy_port, Some(3128));
    assert!(settings.allow_pty);
    assert_eq!(settings.mandatory_deny_search_depth, 5);
}

#[test]
fn test_sandbox_settings_ignore_violations() {
    let json = r#"{
        "ignore_violations": {
            "*": ["mach-lookup"],
            "npm install": ["file-write-data", "network-outbound"]
        }
    }"#;

    let settings: SandboxSettings = serde_json::from_str(json).expect("parse");
    assert_eq!(settings.ignore_violations.len(), 2);
    assert_eq!(
        settings.ignore_violations.get("*").unwrap(),
        &vec!["mach-lookup".to_string()]
    );
    assert_eq!(
        settings.ignore_violations.get("npm install").unwrap().len(),
        2
    );
}

// ==========================================================================
// Git pointer file detection
// ==========================================================================

#[test]
fn test_writable_root_detects_git_pointer_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let gitdir = root.join("actual_gitdir");
    std::fs::create_dir_all(&gitdir).expect("create gitdir");

    // Create a .git pointer file like git worktrees use
    std::fs::write(root.join(".git"), format!("gitdir: {}", gitdir.display())).expect("write");

    let wr = WritableRoot::new(root);
    // Should contain default subpaths plus the resolved gitdir
    assert!(wr.readonly_subpaths.contains(&".git".to_string()));
    assert!(wr.readonly_subpaths.contains(&".coco".to_string()));
    let gitdir_rel = gitdir
        .strip_prefix(root)
        .expect("strip")
        .display()
        .to_string();
    assert!(
        wr.readonly_subpaths.contains(&gitdir_rel),
        "Should contain resolved gitdir: {gitdir_rel}, got: {:?}",
        wr.readonly_subpaths
    );
}

#[test]
fn test_writable_root_git_dir_no_pointer_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    // Create a normal .git directory (not a pointer file)
    std::fs::create_dir_all(root.join(".git")).expect("create .git");

    let wr = WritableRoot::new(root);
    // Default subpaths only — no extra gitdir resolution
    assert_eq!(wr.readonly_subpaths, default_readonly_subpaths());
}

#[test]
fn test_writable_root_no_git_at_all() {
    let dir = tempfile::tempdir().expect("tempdir");
    let wr = WritableRoot::new(dir.path());
    assert_eq!(wr.readonly_subpaths, default_readonly_subpaths());
}

#[test]
fn test_writable_root_git_pointer_relative_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let gitdir = root.join("..").join("shared_git");
    std::fs::create_dir_all(&gitdir).expect("create gitdir");

    // Create .git pointer with relative path
    std::fs::write(root.join(".git"), "gitdir: ../shared_git").expect("write");

    let wr = WritableRoot::new(root);
    // Relative gitdir outside root → should warn but not add (can't strip_prefix)
    // Just check it doesn't panic and has default subpaths
    assert!(wr.readonly_subpaths.contains(&".git".to_string()));
}

#[test]
fn test_writable_root_git_pointer_invalid_content() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    // Create .git with invalid content (no "gitdir:" prefix)
    std::fs::write(root.join(".git"), "not a valid pointer").expect("write");

    let wr = WritableRoot::new(root);
    // Should fall back to default subpaths
    assert_eq!(wr.readonly_subpaths, default_readonly_subpaths());
}

#[test]
fn test_writable_root_git_pointer_multiline() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let gitdir = root.join("actual_gitdir");
    std::fs::create_dir_all(&gitdir).expect("create gitdir");

    // Multi-line content — only first line should be parsed
    std::fs::write(
        root.join(".git"),
        format!("gitdir: {}\nextra line\n", gitdir.display()),
    )
    .expect("write");

    let wr = WritableRoot::new(root);
    let gitdir_rel = gitdir
        .strip_prefix(root)
        .expect("strip")
        .display()
        .to_string();
    assert!(
        wr.readonly_subpaths.contains(&gitdir_rel),
        "Multi-line pointer should resolve correctly"
    );
}
