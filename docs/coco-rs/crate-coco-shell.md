# coco-shell — Crate Plan (REWRITE from TS)

TS source: `src/utils/bash/` (12K LOC), `src/utils/Shell.ts`, `src/utils/shell/`, `src/tools/BashTool/bashPermissions.ts`, `src/tools/BashTool/bashSecurity.ts`, `src/tools/BashTool/readOnlyValidation.ts`, `src/tools/BashTool/commandSemantics.ts`

**Strategy**: REWRITE from TS. TS has 23K LOC of battle-tested validation; cocode-rs only 6.2K.

## Dependencies

```
coco-shell depends on:
  - coco-types       (SandboxMode)
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
    fn pipe_segments(&self) -> Vec<String>;
    fn without_output_redirections(&self) -> String;
    fn output_redirections(&self) -> Vec<OutputRedirection>;
}

pub struct OutputRedirection {
    pub target: String,
    pub operator: RedirectOp,  // Append, Overwrite
}

// Two implementations (from TS):
// 1. TreeSitterParsedCommand — accurate, quote-aware
// 2. RegexParsedCommand — fallback when tree-sitter unavailable
```

### Bash Security (from `bashSecurity.ts` — 20+ check categories)

```rust
pub enum SecurityCheckId {
    IncompleteCommands = 1,
    JqSystemFunction = 2,
    ObfuscatedFlags = 4,
    ShellMetacharacters = 5,
    DangerousVariables = 6,
    CommandSubstitution = 8,
    InputRedirection = 9,
    OutputRedirection = 10,
    BraceExpansion = 16,
    ControlCharacters = 17,
    UnicodeWhitespace = 18,
    ZshDangerousCommands = 20,
    // ... more
}

pub static ZSH_DANGEROUS_COMMANDS: LazyLock<HashSet<&str>> = ...; // zmodload, emulate, sysopen, sysread, syswrite, zpty, ztcp, zsocket, mapfile, zf_rm, zf_mv, zf_ln, zf_chmod

pub fn check_security(command: &str) -> Vec<SecurityCheck>;
pub fn strip_safe_redirections(command: &str) -> String;  // remove 2>&1, >/dev/null
pub fn has_unescaped_char(content: &str, ch: char) -> bool;
pub fn extract_quoted_content(command: &str) -> QuotedContent;
```

### Bash Permissions (from `bashPermissions.ts`)

```rust
pub fn get_simple_command_prefix(command: &str) -> Option<String>;
// "git commit -m 'fix'" -> "git commit"
// Skips safe env vars (NODE_ENV, RUST_LOG, etc.)

pub fn get_first_word_prefix(command: &str) -> Option<String>;
// UI fallback, excludes BARE_SHELL_PREFIXES

pub static SAFE_ENV_VARS: LazyLock<HashSet<&str>> = ...; // NODE_ENV, RUST_LOG, PYTHON_ENV, GO_ENV...
pub static BARE_SHELL_PREFIXES: LazyLock<HashSet<&str>> = ...; // sh, bash, zsh, env, xargs, sudo, nohup...

pub const MAX_SUBCOMMANDS_FOR_SECURITY_CHECK: usize = 50;
```

### Read-Only Validation (from `readOnlyValidation.ts`)

```rust
pub struct CommandConfig {
    pub safe_flags: HashMap<String, FlagArgType>,
    pub regex: Option<Regex>,
    pub is_dangerous_callback: Option<fn(&str, &[String]) -> bool>,
    pub respects_double_dash: bool,
}

pub enum FlagArgType { None, StringArg, NumberArg, CharArg }

/// Allowlisted safe commands with flag validation:
/// - git (show, log, diff, status, branch, blame, help)
/// - file, sort, man, netstat, ps, xargs, head, tail, wc
/// - ripgrep, docker (read-only ops), pyright
pub static COMMAND_ALLOWLIST: LazyLock<HashMap<&str, CommandConfig>> = ...;

pub fn is_read_only_command(command: &str) -> bool;
```

## Core Logic

### Shell Execution (from `utils/Shell.ts`)

```rust
pub struct ShellExecutor {
    shell_path: PathBuf,  // detected via findSuitableShell()
    cwd: PathBuf,
}

pub struct ExecOptions {
    pub timeout: Duration,      // default: 30 min
    pub prevent_cwd_changes: bool,
    pub should_use_sandbox: bool,
    pub should_auto_background: bool,
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
