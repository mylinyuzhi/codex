# coco-shell — Crate Plan (REWRITE from TS)

TS source: `src/utils/bash/` (12K LOC), `src/utils/Shell.ts`, `src/utils/shell/`, `src/tools/BashTool/bashPermissions.ts` (700+), `src/tools/BashTool/bashSecurity.ts`, `src/tools/BashTool/readOnlyValidation.ts` (68K), `src/tools/BashTool/commandSemantics.ts`, `src/tools/BashTool/destructiveCommandWarning.ts` (103), `src/tools/BashTool/shouldUseSandbox.ts` (154), `src/tools/BashTool/modeValidation.ts` (116), `src/tools/BashTool/sedEditParser.ts` (200+)

**Strategy**: REWRITE from TS. TS has 23K LOC of battle-tested validation; cocode-rs only 6.2K.

## Dependencies

```
coco-shell depends on:
  - coco-types       (SandboxMode, PermissionMode)
  - utils/shell-parser (command parsing, security analysis — HYBRID: cocode-rs structure + TS corpus)
  - coco-sandbox     (SandboxSettings, sandbox enforcement)
  - tokio, tokio-util (CancellationToken, process spawning)

coco-shell does NOT depend on:
  - coco-tool     (no Tool trait awareness)
  - coco-config   (no settings — receives options via ExecOptions parameter)
  - coco-permissions (no permission evaluation — caller handles that)
  - any app/ crate
```

## Data Definitions

### Parsed Command (from `utils/bash/ParsedCommand.ts`)

```rust
pub trait ParsedCommand {
    fn original(&self) -> &str;
    /// Returns pipe segments as borrowed slices (no heap allocation on hot path).
    fn pipe_segments(&self) -> Vec<&str>;
    fn without_output_redirections(&self) -> String;
    fn redirections(&self) -> &[OutputRedirection];
}

pub struct OutputRedirection {
    pub target: String,
    pub operator: RedirectOp,  // Append, Overwrite
    pub fd: Option<i32>,       // 1 = stdout, 2 = stderr, None = default
}

// Two implementations (from TS):
// 1. TreeSitterParsedCommand — accurate, quote-aware
// 2. RegexParsedCommand — fallback when tree-sitter unavailable
```

### Bash Security (from `bashSecurity.ts` — 20+ check categories)

```rust
/// Security check IDs. Uses newtype for extensibility without recompilation.
/// Each check is a compile-time constant — no magic numbers in code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SecurityCheckId(i32);

impl SecurityCheckId {
    pub const INCOMPLETE_COMMANDS: Self = Self(1);
    pub const JQ_SYSTEM_FUNCTION: Self = Self(2);
    pub const OBFUSCATED_FLAGS: Self = Self(4);
    pub const SHELL_METACHARACTERS: Self = Self(5);
    pub const DANGEROUS_VARIABLES: Self = Self(6);
    pub const COMMAND_SUBSTITUTION: Self = Self(8);
    pub const INPUT_REDIRECTION: Self = Self(9);
    pub const OUTPUT_REDIRECTION: Self = Self(10);
    pub const PROCESS_SUBSTITUTION: Self = Self(11);
    pub const HERE_DOC_INJECTION: Self = Self(12);
    pub const ALIAS_OVERRIDE: Self = Self(13);
    pub const FUNCTION_OVERRIDE: Self = Self(14);
    pub const SIGNAL_TRAP_HIJACK: Self = Self(15);
    pub const BRACE_EXPANSION: Self = Self(16);
    pub const CONTROL_CHARACTERS: Self = Self(17);
    pub const UNICODE_WHITESPACE: Self = Self(18);
    pub const PATH_TRAVERSAL: Self = Self(19);
    pub const ZSH_DANGEROUS_COMMANDS: Self = Self(20);
    pub const SYMLINK_ATTACK: Self = Self(21);
    pub const UNC_PATH_CREDENTIAL_LEAK: Self = Self(22);  // Windows only
    pub const GLOB_EXPANSION_DANGER: Self = Self(23);
    pub const BACKGROUND_EXEC: Self = Self(24);
}

pub static ZSH_DANGEROUS_COMMANDS: LazyLock<HashSet<&str>> = ...; // zmodload, emulate, sysopen, sysread, syswrite, zpty, ztcp, zsocket, mapfile, zf_rm, zf_mv, zf_ln, zf_chmod

pub fn check_security(command: &str) -> Vec<SecurityCheck>;
pub fn strip_safe_redirections(command: &str) -> String;  // remove 2>&1, >/dev/null
pub fn has_unescaped_char(content: &str, ch: char) -> bool;
pub fn extract_quoted_content(command: &str) -> QuotedContent;
```

### Bash Permissions (from `bashPermissions.ts`, 700+ LOC)

```rust
pub fn get_simple_command_prefix(command: &str) -> Option<String>;
// "git commit -m 'fix'" -> "git commit"
// Skips safe env vars (NODE_ENV, RUST_LOG, etc.)

pub fn get_first_word_prefix(command: &str) -> Option<String>;
// UI fallback, excludes BARE_SHELL_PREFIXES

pub static SAFE_ENV_VARS: LazyLock<HashSet<&str>> = ...; // NODE_ENV, RUST_LOG, PYTHON_ENV, GO_ENV...
pub static BARE_SHELL_PREFIXES: LazyLock<HashSet<&str>> = ...; // sh, bash, zsh, env, xargs, sudo, nohup...

pub const MAX_SUBCOMMANDS_FOR_SECURITY_CHECK: usize = 50;

/// Full bash permission analysis pipeline:
/// 1. parseForSecurityFromAst() — tree-sitter bash AST parsing
/// 2. checkSemantics() — command semantics validation
/// 3. checkCommandOperatorPermissions() — pipe/redirect analysis
/// 4. checkPathConstraints() — file path whitelist/blacklist
/// 5. checkSedConstraints() — sed in-place edit validation
/// 6. shouldUseSandbox() — sandbox decision
/// 7. checkPermissionMode() — mode-specific auto-allow
///
/// Compound command splitting: handles cmd1 && cmd2 | cmd3 branching
/// Env var stripping: FOO=bar cmd → analyze cmd
/// Wrapper stripping: timeout 30 cmd → analyze cmd
/// Complexity guard: 50-command limit to prevent event loop starvation
```

### Read-Only Validation (from `readOnlyValidation.ts`, 68K)

```rust
pub struct CommandConfig {
    pub safe_flags: HashMap<String, FlagArgType>,
    pub regex: Option<Regex>,
    /// Extra validation for commands needing context-aware checks.
    /// Uses enum dispatch instead of function pointers (avoids fn pointer anti-pattern).
    pub extra_check: Option<ExtraValidation>,
    pub respects_double_dash: bool,
}

pub enum ExtraValidation {
    /// git pickaxe flags (-S, -G, -O) must take an argument
    GitPickaxeArgRequired,
    /// docker: reject write subcommands (rm, rmi, exec, etc.)
    DockerReadOnly,
    /// gh: reject mutating API methods (POST, PUT, DELETE)
    GhReadOnly,
}

pub enum FlagArgType { None, StringArg, NumberArg, CharArg }

/// Allowlisted safe commands with flag validation:
/// - git (show, log, diff, status, branch, blame, help, rev-list, reflog, shortlog — 200+ safe flags)
/// - file, sort, man, netstat, ps, xargs, head, tail, wc
/// - ripgrep, docker (read-only ops), pyright
/// - gh (pr view, issue view, workflow run — ant-only, network-safe)
///
/// SECURITY: Pickaxe search flags (-S, -G, -O) must take arguments
/// to prevent "git diff -S -- --output=/tmp/pwned" bypass.
/// UNC path detection: containsVulnerableUncPath() for Windows credential leak prevention.
pub static COMMAND_ALLOWLIST: LazyLock<HashMap<&str, CommandConfig>> = ...;

pub fn is_read_only_command(command: &str) -> bool;
```

## Destructive Command Warning (from `destructiveCommandWarning.ts`, 103 LOC)

```rust
/// Pattern-based destructive command detection (~20 patterns).
/// Returns a human-readable warning string if command is destructive.
/// Called by: BashTool in coco-tools (during tool execution, before exec).
///
/// Covered patterns:
///   git reset --hard       → "may discard uncommitted changes"
///   git push --force/-f    → "may overwrite remote history"
///   git clean -f (no -n)   → "may permanently delete untracked files"
///   git checkout .          → "may discard all working tree changes"
///   git --no-verify         → "may skip safety hooks"
///   git commit --amend      → "may rewrite the last commit"
///   rm -rf                  → "may recursively force-remove files"
///   DROP/TRUNCATE TABLE     → "may drop or truncate database"
///   kubectl delete          → "may delete Kubernetes resources"
///   terraform destroy       → "may destroy Terraform infrastructure"
///   docker rm/rmi --force   → "may remove containers/images"
///   ... (~10 more patterns)
pub fn get_destructive_command_warning(command: &str) -> Option<String>;
```

## Sandbox Decision (from `shouldUseSandbox.ts`, 154 LOC)

```rust
/// Complex sandbox decision logic:
/// 1. Check SandboxManager.is_sandboxing_enabled()
/// 2. Check dangerously_disable_sandbox override + policy
/// 3. Check contains_excluded_command(command):
///    - Dynamic GrowthBook: tengu_sandbox_disabled_commands
///    - User config: settings.sandbox.excluded_commands
///    - Wildcard + prefix matching on command candidates
/// 4. Return true if sandboxing applies
pub fn should_use_sandbox(command: Option<&str>, disable_override: bool) -> bool;

/// Check if command matches any excluded patterns
fn contains_excluded_command(command: &str) -> bool;
```

## Mode Validation (from `modeValidation.ts`, 116 LOC)

```rust
/// Permission mode-specific auto-allow logic.
///
/// acceptEdits mode: auto-allow SAFE filesystem commands only.
/// SECURITY: Commands are only auto-allowed if their flags are safe.
/// - `rm` without `-r`/`-f` flags only (no recursive force-delete)
/// - `sed` without `-i` flag only (no in-place edits to arbitrary files)
/// - `cp`/`mv` without `--force` only
/// Other commands (mkdir, touch, rmdir) are inherently safe for filesystem ops.
const ACCEPT_EDITS_ALLOWED_COMMANDS: &[&str] = &[
    "mkdir", "touch", "rmdir"  // Inherently safe
];

/// Commands allowed in acceptEdits mode only with flag validation.
/// Caller MUST also call `validate_flags_for_accept_edits()` for these.
const ACCEPT_EDITS_FLAG_VALIDATED_COMMANDS: &[&str] = &[
    "rm", "mv", "cp", "sed"
];

/// Internal to check_permission_mode() — NOT a public API.
/// Called automatically when command matches ACCEPT_EDITS_FLAG_VALIDATED_COMMANDS.
/// Returns false if dangerous flags detected:
/// - rm: rejects -r, -R, --recursive, -f, --force
/// - sed: rejects -i, --in-place
/// - cp/mv: rejects --force to arbitrary destinations outside CWD
fn validate_flags_for_accept_edits(command: &str) -> bool;

/// Check if permission mode auto-allows this command.
/// - acceptEdits + safe filesystem command → allow
/// - acceptEdits + flag-validated command + safe flags → allow
/// - acceptEdits + flag-validated command + dangerous flags → passthrough (ask user)
/// - bypassPermissions / dontAsk → passthrough (handled elsewhere)
/// - default / auto / plan → passthrough
pub fn check_permission_mode(
    command: &str,
    mode: &PermissionMode,
) -> PermissionCheckResult;

pub enum PermissionCheckResult {
    Allow,
    Passthrough,  // No mode-specific handling; let caller decide
}

pub fn get_auto_allowed_commands(mode: &PermissionMode) -> &[&str];
```

### Command Semantics (from `commandSemantics.ts`)

```rust
pub struct CommandSemantics {
    pub is_search: bool,
    pub is_read: bool,
    pub is_list: bool,
    pub is_destructive: bool,
}

pub fn classify_command(command: &str) -> CommandSemantics;
```

## Core Logic

### Shell Execution (from `utils/Shell.ts`)

```rust
pub struct ShellExecutor {
    shell_path: PathBuf,  // detected via findSuitableShell()
    cwd: PathBuf,
}

pub struct ExecOptions {
    pub timeout: Duration,              // default: 120s (configurable via API_TIMEOUT_MS)
    pub prevent_cwd_changes: bool,
    pub should_use_sandbox: bool,
    pub should_auto_background: bool,
    pub extra_env: HashMap<String, String>,  // additional env vars for this command
    pub cwd_override: Option<PathBuf>,       // override working directory
}

pub struct ShellResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub new_cwd: Option<PathBuf>,
    pub timed_out: bool,
}

impl ShellExecutor {
    /// Detect suitable shell: CLAUDE_CODE_SHELL env > SHELL env > search /bin,/usr/bin
    pub async fn new(cwd: PathBuf) -> Result<Self, ShellError>;
    pub async fn exec(&self, command: &str, options: ExecOptions, cancel: CancellationToken) -> Result<ShellResult, ShellError>;
}
```
