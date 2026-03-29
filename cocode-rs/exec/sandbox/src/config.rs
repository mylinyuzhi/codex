//! Sandbox configuration types.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

/// Whether sandbox bypass was requested for a specific command.
///
/// Used instead of a bare `bool` so callsites are self-documenting:
/// `is_sandboxed(cmd, SandboxBypass::Requested)` vs `is_sandboxed(cmd, true)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxBypass {
    /// Bypass not requested; normal sandboxing applies.
    No,
    /// Bypass requested via `dangerouslyDisableSandbox` parameter.
    Requested,
}

impl SandboxBypass {
    /// Create from a boolean flag (for interop with JSON input parsing).
    pub fn from_flag(dangerously_disable_sandbox: bool) -> Self {
        if dangerously_disable_sandbox {
            Self::Requested
        } else {
            Self::No
        }
    }
}

/// Sandbox enforcement level controlling filesystem and network access.
///
/// Distinct from `cocode_protocol::SandboxMode` which represents the user's
/// intent (ReadOnly/WorkspaceWrite/FullAccess). This enum represents the
/// actual enforcement behavior applied at runtime.
///
/// Convert from protocol mode via `EnforcementLevel::from(sandbox_mode)`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EnforcementLevel {
    /// No sandbox enforcement; all operations are allowed.
    #[default]
    Disabled,
    /// Read-only mode; file writes are blocked.
    ReadOnly,
    /// Workspace-write mode; writes allowed to configured writable roots.
    WorkspaceWrite,
    /// Strict mode; only explicitly allowed paths are accessible,
    /// and network is blocked unless explicitly allowed.
    Strict,
}

impl From<cocode_protocol::SandboxMode> for EnforcementLevel {
    fn from(mode: cocode_protocol::SandboxMode) -> Self {
        match mode {
            cocode_protocol::SandboxMode::ReadOnly => Self::ReadOnly,
            cocode_protocol::SandboxMode::WorkspaceWrite => Self::WorkspaceWrite,
            cocode_protocol::SandboxMode::FullAccess => Self::Disabled,
            // ExternalSandbox: workspace-write enforcement for permission checks,
            // but platform wrapping (bwrap/Seatbelt) is skipped by SandboxState.
            cocode_protocol::SandboxMode::ExternalSandbox => Self::WorkspaceWrite,
        }
    }
}

/// A writable root directory with read-only subpath protections.
///
/// Certain subpaths (e.g., `.git`, `.cocode`) remain read-only even when
/// the parent directory is writable. This prevents the agent from modifying
/// version control or configuration state within otherwise writable projects.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WritableRoot {
    /// The root directory that is writable.
    pub path: PathBuf,
    /// Subpaths that remain read-only even under this writable root.
    #[serde(default = "default_readonly_subpaths")]
    pub readonly_subpaths: Vec<String>,
}

impl WritableRoot {
    /// Creates a writable root with default read-only subpaths.
    ///
    /// If `.git` is a pointer file (git worktrees/submodules), the actual
    /// gitdir is also added to the read-only subpaths (adopted from codex-rs).
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let mut subpaths = default_readonly_subpaths();

        // Detect git pointer files: in worktrees and submodules, `.git` is a
        // file containing `gitdir: <path>`. The actual gitdir must also be
        // protected as read-only (codex-rs pattern).
        let git_path = path.join(".git");
        if git_path.is_file()
            && let Some(gitdir) = resolve_git_pointer(&git_path)
        {
            match gitdir.strip_prefix(&path) {
                Ok(rel) => {
                    let rel_str = rel.display().to_string();
                    if !subpaths.contains(&rel_str) {
                        subpaths.push(rel_str);
                    }
                }
                Err(_) => {
                    tracing::warn!(
                        gitdir = %gitdir.display(),
                        root = %path.display(),
                        "Git pointer resolves outside writable root; cannot protect"
                    );
                }
            }
        }

        Self {
            path,
            readonly_subpaths: subpaths,
        }
    }

    /// Creates a writable root with no read-only subpath protections.
    pub fn unprotected(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            readonly_subpaths: Vec::new(),
        }
    }

    /// Check if a path is writable under this root.
    ///
    /// Returns `true` if the path is under this root AND not under any
    /// of the read-only subpaths.
    pub fn is_writable(&self, path: &Path) -> bool {
        path.starts_with(&self.path)
            && !self
                .readonly_subpaths
                .iter()
                .any(|sub| path.starts_with(self.path.join(sub)))
    }

    /// Check if a path is under this root (regardless of write permission).
    pub fn contains(&self, path: &Path) -> bool {
        path.starts_with(&self.path)
    }

    /// Resolve read-only subpaths to absolute paths (root + subpath).
    ///
    /// Used by platform enforcement (bwrap, Seatbelt) to generate
    /// mount/deny rules with full paths.
    pub fn resolved_readonly_subpaths(&self) -> Vec<PathBuf> {
        self.readonly_subpaths
            .iter()
            .map(|sub| self.path.join(sub))
            .collect()
    }
}

/// Resolve a `.git` pointer file to the actual gitdir path.
///
/// Git worktrees and submodules use a `.git` file (not directory) containing
/// `gitdir: <path>`. Returns the resolved absolute path to the actual gitdir,
/// or `None` if the file isn't a valid pointer.
fn resolve_git_pointer(git_file: &Path) -> Option<PathBuf> {
    let content = std::fs::read_to_string(git_file).ok()?;
    // Extract first line only — pointer files have `gitdir: <path>` on line 1.
    let first_line = content.lines().next()?.trim();
    let gitdir = first_line.strip_prefix("gitdir:")?.trim();
    if gitdir.is_empty() {
        return None;
    }
    let gitdir_path = PathBuf::from(gitdir);
    let resolved = if gitdir_path.is_relative() {
        git_file.parent()?.join(&gitdir_path)
    } else {
        gitdir_path
    };
    match std::fs::canonicalize(&resolved) {
        Ok(p) => Some(p),
        Err(e) => {
            tracing::debug!(path = %resolved.display(), error = %e, "Failed to resolve gitdir");
            None
        }
    }
}

fn default_readonly_subpaths() -> Vec<String> {
    vec![
        ".git".to_string(),
        ".cocode".to_string(),
        ".agents".to_string(),
    ]
}

/// Configuration for the sandbox.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SandboxConfig {
    /// The sandbox enforcement level.
    #[serde(default)]
    pub enforcement: EnforcementLevel,
    /// Writable root directories with subpath protections.
    #[serde(default)]
    pub writable_roots: Vec<WritableRoot>,
    /// Paths that are explicitly denied for both read and write (takes precedence).
    #[serde(default)]
    pub denied_paths: Vec<PathBuf>,
    /// Paths explicitly denied for reading.
    #[serde(default)]
    pub denied_read_paths: Vec<PathBuf>,
    /// Paths explicitly denied for writing (in addition to `denied_paths`).
    #[serde(default)]
    pub deny_write_paths: Vec<PathBuf>,
    /// Whether git config files are writable (`.git/config`, `~/.gitconfig`).
    #[serde(default)]
    pub allow_git_config: bool,
    /// Whether network access is allowed.
    #[serde(default)]
    pub allow_network: bool,
    /// Whether the network proxy is active (runtime-only, not persisted).
    ///
    /// Controls seccomp mode selection: `ProxyRouted` when true, `Restricted` when false.
    /// Synced from `SandboxState::network_active()` in the `config()` snapshot method.
    #[serde(skip)]
    pub proxy_active: bool,
    /// Paths to bind-mount into the sandbox (e.g., proxy bridge UDS sockets).
    #[serde(default)]
    pub extra_bind_ro: Vec<PathBuf>,
    /// Allow `com.apple.trustd.agent` mach lookup for Go TLS cert verification.
    ///
    /// Required for Go programs (gh, gcloud, terraform, kubectl) that verify
    /// TLS certificates through macOS system services rather than bundled CAs.
    #[serde(default)]
    pub weaker_network_isolation: bool,
    /// Allow pseudo-terminal access inside the sandbox (macOS).
    ///
    /// Defaults to `true`. When `false`, PTY rules are excluded from the
    /// Seatbelt profile, preventing sandboxed commands from allocating TTYs.
    #[serde(default = "default_true")]
    pub allow_pty: bool,
}

/// Filesystem access configuration for the sandbox.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct FilesystemConfig {
    /// Paths allowed for write access (defaults to CWD).
    #[serde(default)]
    pub allow_write: Vec<PathBuf>,
    /// Paths explicitly denied for writing.
    #[serde(default)]
    pub deny_write: Vec<PathBuf>,
    /// Paths explicitly denied for reading.
    #[serde(default)]
    pub deny_read: Vec<PathBuf>,
    /// Allow writing to `.git/config` and `~/.gitconfig`.
    #[serde(default)]
    pub allow_git_config: bool,
}

/// Network access mode controlling HTTP method enforcement.
///
/// In `Limited` mode, only safe HTTP methods (GET, HEAD, OPTIONS) are allowed.
/// CONNECT tunnels and SOCKS5 are blocked (cannot inspect methods through tunnels).
/// In `Full` mode (default), all methods are permitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkMode {
    /// Full network access: all HTTP methods allowed, CONNECT tunneled, SOCKS5 active.
    #[default]
    Full,
    /// Limited (read-only) access: only GET/HEAD/OPTIONS. CONNECT and SOCKS5 blocked.
    Limited,
}

impl NetworkMode {
    /// Check if an HTTP method is allowed in this mode.
    pub fn allows_method(&self, method: &str) -> bool {
        match self {
            Self::Full => true,
            Self::Limited => matches!(method, "GET" | "HEAD" | "OPTIONS"),
        }
    }
}

/// Network access configuration for the sandbox.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct NetworkConfig {
    /// Network access mode (Full or Limited).
    #[serde(default)]
    pub mode: NetworkMode,
    /// Domain allow list. When non-empty, only these domains are permitted.
    /// Empty means all domains allowed (unless denied).
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    /// Domain deny list (takes precedence over allow list).
    #[serde(default)]
    pub denied_domains: Vec<String>,
    /// Unix socket paths allowed inside the sandbox.
    #[serde(default)]
    pub allow_unix_sockets: Vec<PathBuf>,
    /// Allow all Unix sockets (disables socket blocking).
    #[serde(default)]
    pub allow_all_unix_sockets: bool,
    /// Allow binding to localhost ports inside the sandbox.
    #[serde(default)]
    pub allow_local_binding: bool,
    /// Fixed HTTP proxy port (auto-assigned if `None`).
    #[serde(default)]
    pub http_proxy_port: Option<u16>,
    /// Fixed SOCKS proxy port (auto-assigned if `None`).
    #[serde(default)]
    pub socks_proxy_port: Option<u16>,
    /// MITM proxy configuration for HTTPS inspection.
    #[serde(default)]
    pub mitm_proxy: Option<MitmProxyConfig>,
    /// Block connections to non-public IP addresses (SSRF prevention).
    ///
    /// When enabled, the proxy filter rejects connections to loopback,
    /// private (RFC 1918), link-local, CGNAT, TEST-NET, and other
    /// reserved IP ranges.
    #[serde(default)]
    pub block_non_public_ips: bool,
}

/// MITM proxy configuration for HTTPS traffic inspection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MitmProxyConfig {
    /// Unix socket path for the MITM proxy.
    pub socket_path: PathBuf,
    /// Domains to intercept via MITM.
    #[serde(default)]
    pub domains: Vec<String>,
}

/// Per-command violation ignore patterns.
///
/// Keys are command patterns (or `"*"` for global).
/// Values are lists of violation operations to ignore.
pub type IgnoreViolationsConfig = HashMap<String, Vec<String>>;

/// User/policy-level sandbox settings.
///
/// These settings control whether sandboxing is enabled and how bypass requests
/// are handled. Sandbox is **optional and disabled by default**.
///
/// # Default Behavior
///
/// By default, sandbox is disabled (`enabled: false`), which means:
/// - Commands execute directly without any sandbox wrapping
/// - No platform enforcement (Seatbelt/bubblewrap) is applied
/// - `is_sandboxed()` returns `false`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SandboxSettings {
    /// Enable sandbox mode.
    ///
    /// When `false` (default), commands run directly without sandbox wrapping.
    /// When `true`, commands are wrapped with platform-specific sandbox
    /// (Seatbelt on macOS, bubblewrap on Linux).
    #[serde(default)]
    pub enabled: bool,

    /// Auto-approve bash commands when running in sandbox mode.
    ///
    /// When `true` (default), bash commands that would normally require
    /// approval can run automatically if the sandbox is enabled.
    #[serde(default = "default_true")]
    pub auto_allow_bash_if_sandboxed: bool,

    /// Allow the `dangerously_disable_sandbox` parameter to bypass sandbox.
    ///
    /// When `true` (default), individual commands can request sandbox bypass
    /// using the `dangerously_disable_sandbox` flag.
    #[serde(default = "default_true")]
    pub allow_unsandboxed_commands: bool,

    /// Platforms where sandbox may be enabled.
    #[serde(default = "default_enabled_platforms")]
    pub enabled_platforms: Vec<String>,

    /// Commands excluded from sandbox wrapping.
    ///
    /// Supports three pattern types:
    /// - Exact: `"git"` matches `git` and `git <subcommand>`
    /// - Prefix: `"npm:*"` matches `npm` and all subcommands
    /// - Wildcard: `"npm run *"` matches `npm run build`, etc.
    #[serde(default)]
    pub excluded_commands: Vec<String>,

    /// Filesystem access configuration.
    #[serde(default)]
    pub filesystem: FilesystemConfig,

    /// Network access configuration.
    #[serde(default)]
    pub network: NetworkConfig,

    /// Per-command violation ignore patterns.
    ///
    /// Keys are command patterns (or `"*"` for global).
    /// Values are lists of violation operations to ignore.
    #[serde(default)]
    pub ignore_violations: IgnoreViolationsConfig,

    /// Enable weaker nested sandbox (for Docker/WSL environments).
    #[serde(default)]
    pub enable_weaker_nested_sandbox: bool,

    /// Enable weaker network isolation (allow trustd.agent for Go TLS on macOS).
    #[serde(default)]
    pub enable_weaker_network_isolation: bool,

    /// Allow pseudo-terminal access inside the sandbox (macOS).
    #[serde(default)]
    pub allow_pty: bool,

    /// Directory search depth for mandatory deny path discovery (Linux).
    #[serde(default = "default_mandatory_deny_search_depth")]
    pub mandatory_deny_search_depth: i32,
}

fn default_true() -> bool {
    true
}

fn default_enabled_platforms() -> Vec<String> {
    vec![
        "macos".to_string(),
        "linux".to_string(),
        "windows".to_string(),
    ]
}

fn default_mandatory_deny_search_depth() -> i32 {
    3
}

impl Default for SandboxSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_allow_bash_if_sandboxed: true,
            allow_unsandboxed_commands: true,
            enabled_platforms: default_enabled_platforms(),
            excluded_commands: Vec::new(),
            filesystem: FilesystemConfig::default(),
            network: NetworkConfig::default(),
            ignore_violations: HashMap::new(),
            enable_weaker_nested_sandbox: false,
            enable_weaker_network_isolation: false,
            allow_pty: false,
            mandatory_deny_search_depth: default_mandatory_deny_search_depth(),
        }
    }
}

impl SandboxSettings {
    /// Creates settings with sandbox enabled.
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            ..Default::default()
        }
    }

    /// Creates settings with sandbox disabled (same as default).
    pub fn disabled() -> Self {
        Self::default()
    }

    /// Check if a command should run in sandbox mode.
    ///
    /// Returns `false` (no sandbox) if:
    /// 1. Sandbox is disabled (`!self.enabled`)
    /// 2. Bypass requested and allowed
    /// 3. Command is empty
    /// 4. Command matches an excluded prefix
    pub fn is_sandboxed(&self, command: &str, bypass: SandboxBypass) -> bool {
        if !self.enabled {
            return false;
        }

        if matches!(bypass, SandboxBypass::Requested) && self.allow_unsandboxed_commands {
            return false;
        }

        if command.trim().is_empty() {
            return false;
        }

        if self.is_excluded_command(command) {
            return false;
        }

        true
    }

    /// Check if a command matches any excluded command pattern using BFS
    /// variant expansion.
    ///
    /// For each command segment, builds variants by:
    /// 1. Original token
    /// 2. Strip leading env assignments (`FOO=bar npm` -> `npm`)
    /// 3. Extract basename (`/usr/bin/npm` -> `npm`)
    ///
    /// Matches variants against patterns:
    /// - `"cmd:*"` — prefix match (`npm:*` matches `npm install`)
    /// - `"cmd *"` — wildcard glob (`npm run *` matches `npm run build`)
    /// - `"cmd"` — exact first-word match (`git` matches `git status`)
    fn is_excluded_command(&self, command: &str) -> bool {
        if self.excluded_commands.is_empty() {
            return false;
        }

        let trimmed = command.trim();
        let variants = build_command_variants(trimmed);

        self.excluded_commands.iter().any(|pattern| {
            let pattern = pattern.trim();
            variants
                .iter()
                .any(|variant| matches_exclusion_pattern(variant, pattern))
        })
    }

    /// Check if the current platform is in the enabled platforms list.
    pub fn is_platform_enabled(&self) -> bool {
        let current = if cfg!(target_os = "macos") {
            "macos"
        } else if cfg!(target_os = "linux") {
            "linux"
        } else if cfg!(target_os = "windows") {
            "windows"
        } else {
            return false;
        };
        self.enabled_platforms
            .iter()
            .any(|p| p.eq_ignore_ascii_case(current))
    }
}

/// Build command variants via BFS expansion for exclusion matching.
///
/// Given a command string, produces a set of normalized forms:
/// 1. The original trimmed command
/// 2. With leading env assignments stripped (`A=1 B=2 npm install` -> `npm install`)
/// 3. With the first token replaced by its basename (`/usr/bin/npm install` -> `npm install`)
fn build_command_variants(command: &str) -> Vec<String> {
    use std::collections::HashSet;

    let mut variants = HashSet::new();
    let mut queue = std::collections::VecDeque::new();

    variants.insert(command.to_string());
    queue.push_back(command.to_string());

    while let Some(current) = queue.pop_front() {
        // Variant: strip leading env assignments (KEY=VALUE prefix)
        let stripped = strip_env_prefix(&current);
        if stripped != current && variants.insert(stripped.clone()) {
            queue.push_back(stripped);
        }

        // Variant: extract basename of first token
        let basenamed = replace_first_token_with_basename(&current);
        if basenamed != current && variants.insert(basenamed.clone()) {
            queue.push_back(basenamed);
        }
    }

    variants.into_iter().collect()
}

/// Strip leading `KEY=VALUE` assignments from a command.
///
/// `FOO=bar BAZ=1 npm install` -> `npm install`
fn strip_env_prefix(command: &str) -> String {
    let mut rest = command;
    loop {
        rest = rest.trim_start();
        // Check if the next token looks like KEY=VALUE
        if let Some(space_idx) = rest.find(|c: char| c.is_whitespace()) {
            let token = &rest[..space_idx];
            if token.contains('=') && !token.starts_with('=') && !token.starts_with('-') {
                rest = &rest[space_idx..];
                continue;
            }
        }
        break;
    }
    rest.trim_start().to_string()
}

/// Replace the first token of a command with its basename.
///
/// `/usr/bin/npm install` -> `npm install`
/// `./node_modules/.bin/jest test` -> `jest test`
fn replace_first_token_with_basename(command: &str) -> String {
    let trimmed = command.trim_start();
    let (first, rest) = match trimmed.find(|c: char| c.is_whitespace()) {
        Some(idx) => (&trimmed[..idx], &trimmed[idx..]),
        None => (trimmed, ""),
    };

    if let Some(basename) = Path::new(first).file_name().and_then(|n| n.to_str())
        && basename != first
    {
        return format!("{basename}{rest}");
    }
    command.to_string()
}

/// Match a command variant against an exclusion pattern.
///
/// Pattern types:
/// - `"cmd:*"` — colon-prefix: matches `cmd` and `cmd <anything>`
/// - `"cmd*"` — trailing wildcard: matches anything starting with `cmd`
/// - `"cmd"` — exact first-word: matches `cmd` alone or `cmd <args>`
fn matches_exclusion_pattern(variant: &str, pattern: &str) -> bool {
    // Colon-prefix pattern: "npm:*" matches "npm", "npm install", etc.
    if let Some(prefix) = pattern.strip_suffix(":*") {
        return variant == prefix
            || variant.starts_with(&format!("{prefix} "))
            || variant.starts_with(&format!("{prefix}\t"));
    }

    // Trailing wildcard: "git*" matches "git", "gitk", "git status"
    if pattern.ends_with('*') {
        let prefix = pattern.trim_end_matches('*');
        return variant.starts_with(prefix);
    }

    // Exact first-word match: "git" matches "git" and "git status"
    variant == pattern || variant.starts_with(&format!("{pattern} "))
}

#[cfg(test)]
#[path = "config.test.rs"]
mod tests;
