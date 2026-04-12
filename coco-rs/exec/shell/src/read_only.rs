//! Read-only command validation.
//!
//! TS: readOnlyValidation.ts + readOnlyCommandValidation.ts — 40+ safe
//! commands, 200+ flag configs, docker/gh/kubectl read-only subcommands.
//! Aligned with cocode-rs utils/shell-parser/src/safety/is_safe_command.rs.
//!
//! Commands in the allowlist can be auto-approved without user permission.

/// Check if a command string is read-only (safe to auto-approve).
///
/// Splits the command into words and delegates to argv-based checking.
/// Returns false if uncertain (conservative).
pub fn is_read_only_command(command: &str) -> bool {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return false;
    }

    let argv = split_command_to_argv(trimmed);
    if argv.is_empty() {
        return false;
    }

    is_safe_to_call(&argv)
}

/// Check if an argv array represents a safe (read-only) command.
///
/// Matches the TS permission engine: always-safe commands, conditional
/// safety for find/rg/git/sed/curl/wget/docker/gh/kubectl.
fn is_safe_to_call(argv: &[&str]) -> bool {
    let Some(&cmd0) = argv.first() else {
        return false;
    };

    let cmd = executable_name(cmd0);

    match cmd {
        // Always-safe commands (no side effects)
        "cat" | "cd" | "cut" | "echo" | "expr" | "false" | "grep" | "egrep" | "fgrep" | "head"
        | "id" | "ls" | "nl" | "paste" | "pwd" | "rev" | "seq" | "stat" | "tail" | "tr"
        | "true" | "uname" | "uniq" | "wc" | "which" | "whoami" | "numfmt" | "tac" => true,

        // Additional read-only commands
        "less" | "more" | "file" | "tree" | "locate" | "ag" | "ack" | "diff" | "comm"
        | "hostname" | "date" | "uptime" | "df" | "du" | "free" | "top" | "ps" | "env"
        | "printenv" | "ping" | "dig" | "nslookup" | "host" | "whereis" | "type" | "readlink"
        | "basename" | "dirname" | "realpath" | "md5sum" | "sha256sum" | "sha1sum" | "sort"
        | "column" | "fmt" | "fold" | "expand" | "unexpand" | "strings" | "xxd" | "od"
        | "hexdump" => true,

        // base64: safe unless writing to file
        "base64" => !argv.iter().skip(1).any(|arg| {
            matches!(*arg, "-o" | "--output")
                || arg.starts_with("--output=")
                || (arg.starts_with("-o") && *arg != "-o")
        }),

        // find: safe unless executing or deleting
        "find" => {
            const UNSAFE_FIND: &[&str] = &[
                "-exec", "-execdir", "-ok", "-okdir", "-delete", "-fls", "-fprint", "-fprint0",
                "-fprintf",
            ];
            !argv.iter().any(|arg| UNSAFE_FIND.contains(arg))
        }

        // rg (ripgrep): safe unless using pre-processor or searching zips
        "rg" => !argv.iter().any(|arg| {
            matches!(*arg, "--search-zip" | "-z")
                || matches!(*arg, "--pre" | "--hostname-bin")
                || arg.starts_with("--pre=")
                || arg.starts_with("--hostname-bin=")
        }),

        // git: safe only for read-only subcommands with safe flags
        "git" => is_safe_git_command(argv),

        // sed: safe only for `sed -n {N|M,N}p`
        "sed" => {
            argv.len() <= 4
                && argv.get(1).copied() == Some("-n")
                && is_valid_sed_print(argv.get(2).copied())
        }

        // curl/wget: read-only network fetches (no upload flags)
        "curl" => is_safe_curl(argv),
        "wget" => is_safe_wget(argv),

        // Development tools (read-only operations)
        "cargo" => matches!(
            argv.get(1).copied(),
            Some("check" | "test" | "clippy" | "doc" | "build" | "bench" | "metadata")
        ),

        "npm" | "npx" | "yarn" | "pnpm" => matches!(
            argv.get(1).copied(),
            Some("test" | "run" | "list" | "ls" | "info" | "view" | "search" | "outdated")
        ),

        "python" | "python3" => matches!(argv.get(1).copied(), Some("-c" | "-m" | "--version")),

        "docker" => is_safe_docker_command(argv),
        "gh" => is_safe_gh_command(argv),
        "kubectl" => is_safe_kubectl_command(argv),

        // command -v is safe
        "command" => argv.get(1).copied() == Some("-v"),

        // jq: always safe (pure JSON processor, no side effects)
        "jq" | "yq" => true,

        // xargs with safe commands is handled elsewhere; bare xargs is not safe
        _ => false,
    }
}

// ── git validation ──

/// Check if a git command is read-only.
fn is_safe_git_command(argv: &[&str]) -> bool {
    // Reject git with config override global options
    if argv.iter().any(|arg| {
        matches!(*arg, "-c" | "--config-env" | "-C")
            || (arg.starts_with("-c") && arg.len() > 2)
            || arg.starts_with("--config-env=")
    }) {
        return false;
    }

    // Find the subcommand (skip global flags)
    let subcommand_idx = argv
        .iter()
        .skip(1)
        .position(|arg| !arg.starts_with('-'))
        .map(|i| i + 1);

    let Some(idx) = subcommand_idx else {
        return false;
    };
    let subcommand = argv[idx];
    let sub_args = &argv[idx + 1..];

    match subcommand {
        "status" | "log" | "diff" | "show" => git_args_are_read_only(sub_args),
        "branch" => git_args_are_read_only(sub_args) && git_branch_is_read_only(sub_args),
        "remote" => {
            if sub_args.is_empty() || matches!(sub_args.first(), Some(&"-v" | &"--verbose")) {
                return true;
            }
            // `git remote show <name>` is safe if name is alphanumeric
            if matches!(sub_args.first(), Some(&"show")) {
                let remaining = &sub_args[1..];
                let positional: Vec<&&str> = remaining.iter().filter(|a| **a != "-n").collect();
                return positional.len() == 1
                    && positional[0]
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
            }
            false
        }
        "tag" => is_safe_git_tag(sub_args),
        "describe" | "rev-parse" | "rev-list" | "ls-files" | "ls-tree" | "cat-file"
        | "name-rev" | "merge-base" | "for-each-ref" | "grep" | "shortlog" => true,
        "stash" => sub_args.is_empty() || matches!(sub_args.first(), Some(&"list" | &"show")),
        "worktree" => matches!(sub_args.first(), Some(&"list")),
        "blame" => git_args_are_read_only(sub_args),
        "reflog" => is_safe_git_reflog(sub_args),
        "config" => {
            // Only `git config --get` and read-only variants
            sub_args.iter().any(|a| {
                matches!(
                    *a,
                    "--get"
                        | "--get-all"
                        | "--get-regexp"
                        | "--list"
                        | "-l"
                        | "--show-origin"
                        | "--show-scope"
                )
            })
        }
        "ls-remote" => is_safe_git_ls_remote(sub_args),
        _ => false,
    }
}

/// Check git branch args are read-only (must have a read-only flag).
fn git_branch_is_read_only(args: &[&str]) -> bool {
    if args.is_empty() {
        return true; // `git branch` alone is safe
    }

    // Flags that take arguments
    const FLAGS_WITH_ARGS: &[&str] = &["--contains", "--no-contains", "--points-at", "--sort"];

    let mut saw_read_only = false;
    let mut saw_dash_dash = false;
    let mut i = 0;

    while i < args.len() {
        let arg = args[i];

        if arg == "--" {
            saw_dash_dash = true;
            i += 1;
            continue;
        }

        if saw_dash_dash {
            // Post-`--` positional: branch creation target
            return false;
        }

        if arg.starts_with('-') {
            match arg {
                "--list" | "-l" | "--show-current" | "-a" | "--all" | "-r" | "--remotes" | "-v"
                | "-vv" | "--verbose" | "--color" | "--no-color" | "--column" | "--no-column"
                | "--no-abbrev" | "-i" | "--ignore-case" => {
                    saw_read_only = true;
                    i += 1;
                }
                _ if arg.starts_with("--format=")
                    || arg.starts_with("--abbrev=")
                    || arg.starts_with("--sort=") =>
                {
                    saw_read_only = true;
                    i += 1;
                }
                _ if FLAGS_WITH_ARGS.contains(&arg) => {
                    saw_read_only = true;
                    i += 2; // skip flag + arg
                }
                _ if arg.contains('=') => {
                    i += 1; // unknown --flag=val, skip
                }
                _ => return false, // unknown flag like -d, -D
            }
        } else {
            // Positional argument — could be branch creation unless -l/--list seen
            if !saw_read_only {
                return false;
            }
            i += 1; // pattern after --list
        }
    }
    saw_read_only
}

/// Check git tag args are read-only.
fn is_safe_git_tag(args: &[&str]) -> bool {
    if args.is_empty() {
        return true; // bare `git tag` lists tags
    }

    const FLAGS_WITH_ARGS: &[&str] = &[
        "--contains",
        "--no-contains",
        "--merged",
        "--no-merged",
        "--points-at",
        "--sort",
        "--format",
        "-n",
    ];

    let mut seen_list_flag = false;
    let mut seen_dash_dash = false;
    let mut i = 0;

    while i < args.len() {
        let token = args[i];

        if token == "--" && !seen_dash_dash {
            seen_dash_dash = true;
            i += 1;
            continue;
        }

        if !seen_dash_dash && token.starts_with('-') {
            if matches!(token, "--list" | "-l") {
                seen_list_flag = true;
            } else if token.starts_with('-')
                && !token.starts_with("--")
                && token.len() > 2
                && !token.contains('=')
                && token[1..].contains('l')
            {
                // Short-flag bundle containing 'l'
                seen_list_flag = true;
            }

            if token.contains('=') {
                i += 1;
            } else if FLAGS_WITH_ARGS.contains(&token) {
                i += 2;
            } else {
                i += 1;
            }
        } else {
            // Positional arg — safe only with list flag
            if !seen_list_flag {
                return false;
            }
            i += 1;
        }
    }

    true
}

/// Check git reflog args are read-only.
fn is_safe_git_reflog(args: &[&str]) -> bool {
    const DANGEROUS_SUBCOMMANDS: &[&str] = &["expire", "delete", "exists"];

    for &token in args {
        if token.starts_with('-') {
            continue;
        }
        // First positional
        if DANGEROUS_SUBCOMMANDS.contains(&token) {
            return false;
        }
        return true; // show or ref name — safe
    }
    true // bare `git reflog` — safe
}

/// Check git ls-remote args don't contain exfiltration vectors.
fn is_safe_git_ls_remote(args: &[&str]) -> bool {
    // Block --server-option/-o (arbitrary data to remote)
    if args
        .iter()
        .any(|a| matches!(*a, "--server-option" | "-o") || a.starts_with("--server-option="))
    {
        return false;
    }
    // Block URL-like positional args
    for &arg in args {
        if arg.starts_with('-') {
            continue;
        }
        if arg.contains("://") || arg.contains('@') {
            return false;
        }
        // Allow simple remote names (alphanumeric, _, -, /)
        if arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '/' | '.'))
        {
            continue;
        }
        return false;
    }
    true
}

/// Check git subcommand args don't have unsafe output flags.
fn git_args_are_read_only(args: &[&str]) -> bool {
    const UNSAFE_GIT_FLAGS: &[&str] = &[
        "--output",
        "--ext-diff",
        "--textconv",
        "--exec",
        "--paginate",
    ];
    !args.iter().any(|arg| {
        UNSAFE_GIT_FLAGS.contains(arg) || arg.starts_with("--output=") || arg.starts_with("--exec=")
    })
}

// ── curl/wget validation ──

fn is_safe_curl(argv: &[&str]) -> bool {
    !argv.iter().any(|arg| {
        matches!(
            *arg,
            "-X" | "--request"
                | "-d"
                | "--data"
                | "--data-raw"
                | "--data-binary"
                | "-F"
                | "--form"
                | "-T"
                | "--upload-file"
                | "-o"
                | "--output"
        ) || arg.starts_with("--data=")
            || arg.starts_with("--output=")
    })
}

fn is_safe_wget(argv: &[&str]) -> bool {
    !argv.iter().any(|arg| {
        matches!(
            *arg,
            "--post-data"
                | "--post-file"
                | "--method"
                | "--body-data"
                | "--body-file"
                | "-O"
                | "--output-document"
        )
    })
}

// ── docker validation ──

/// Check if a docker command is read-only.
fn is_safe_docker_command(argv: &[&str]) -> bool {
    let sub = argv.get(1).copied();
    match sub {
        Some("ps" | "images" | "version" | "info" | "stats" | "top") => true,
        Some("logs") => is_safe_docker_logs(&argv[2..]),
        Some("inspect") => is_safe_docker_inspect(&argv[2..]),
        Some("network") => matches!(argv.get(2).copied(), Some("ls" | "inspect")),
        Some("volume") => matches!(argv.get(2).copied(), Some("ls" | "inspect")),
        Some("image") => matches!(argv.get(2).copied(), Some("ls" | "inspect" | "history")),
        Some("container") => matches!(
            argv.get(2).copied(),
            Some("ls" | "inspect" | "logs" | "top" | "stats")
        ),
        Some("compose") => matches!(argv.get(2).copied(), Some("ps" | "logs" | "config")),
        _ => false,
    }
}

fn is_safe_docker_logs(args: &[&str]) -> bool {
    const SAFE_FLAGS: &[&str] = &[
        "--follow",
        "-f",
        "--tail",
        "-n",
        "--timestamps",
        "-t",
        "--since",
        "--until",
        "--details",
    ];
    args.iter().all(|a| {
        !a.starts_with('-')
            || SAFE_FLAGS.contains(a)
            || a.starts_with("--tail=")
            || a.starts_with("--since=")
            || a.starts_with("--until=")
    })
}

fn is_safe_docker_inspect(args: &[&str]) -> bool {
    const SAFE_FLAGS: &[&str] = &["--format", "-f", "--type", "--size", "-s"];
    args.iter().all(|a| {
        !a.starts_with('-')
            || SAFE_FLAGS.contains(a)
            || a.starts_with("--format=")
            || a.starts_with("--type=")
    })
}

// ── gh validation ──

/// Check if a gh command is read-only.
fn is_safe_gh_command(argv: &[&str]) -> bool {
    let sub = argv.get(1).copied();
    let sub2 = argv.get(2).copied();
    let args = if argv.len() > 3 { &argv[3..] } else { &[] };

    // Block mutating flags globally
    if argv.iter().any(|arg| {
        matches!(
            *arg,
            "--edit" | "--close" | "--delete" | "--merge" | "--reopen" | "--approve"
        )
    }) {
        return false;
    }

    // Check for exfiltration: URLs, HOST/OWNER/REPO format
    if has_gh_exfil_risk(argv) {
        return false;
    }

    match (sub, sub2) {
        (Some("pr"), Some("view" | "list" | "diff" | "checks" | "status")) => true,
        (Some("issue"), Some("view" | "list" | "status")) => true,
        (Some("repo"), Some("view")) => true,
        (Some("run"), Some("list" | "view")) => {
            // Block --web/-w (opens browser — side effect)
            !argv.iter().any(|a| matches!(*a, "--web" | "-w"))
        }
        (Some("auth"), Some("status")) => {
            // Block --show-token/-t (leaks secrets)
            !args.iter().any(|a| matches!(*a, "--show-token" | "-t"))
        }
        (Some("release"), Some("list" | "view")) => true,
        (Some("workflow"), Some("list" | "view")) => {
            !argv.iter().any(|a| matches!(*a, "--web" | "-w"))
        }
        (Some("label"), Some("list")) => !argv.iter().any(|a| matches!(*a, "--web" | "-w")),
        (Some("search"), Some("repos" | "issues" | "prs" | "commits" | "code")) => {
            !argv.iter().any(|a| matches!(*a, "--web" | "-w"))
        }
        (Some("api"), _) => {
            // gh api is read-only only for GET requests (default)
            !argv.iter().any(|a| {
                matches!(
                    *a,
                    "-X" | "--method" | "-f" | "--raw-field" | "-F" | "--field"
                ) || a.starts_with("--method=")
            })
        }
        (Some("status"), _) => true,
        _ => false,
    }
}

/// Check for gh exfiltration risk: URLs or HOST/OWNER/REPO patterns.
fn has_gh_exfil_risk(argv: &[&str]) -> bool {
    for &token in argv {
        let value = if token.starts_with('-') {
            if let Some(eq_idx) = token.find('=') {
                &token[eq_idx + 1..]
            } else {
                continue;
            }
        } else {
            token
        };

        if value.is_empty()
            || (!value.contains('/') && !value.contains("://") && !value.contains('@'))
        {
            continue;
        }

        // URL schemes
        if value.contains("://") {
            return true;
        }
        // SSH-style
        if value.contains('@') {
            return true;
        }
        // 3+ segments = HOST/OWNER/REPO
        if value.matches('/').count() >= 2 {
            return true;
        }
    }
    false
}

// ── kubectl validation ──

/// Check if a kubectl command is read-only.
fn is_safe_kubectl_command(argv: &[&str]) -> bool {
    let sub = argv.get(1).copied();

    match sub {
        Some(
            "get" | "describe" | "logs" | "top" | "version" | "cluster-info" | "api-resources"
            | "api-versions" | "explain",
        ) => {
            // Block --output with templates that could execute code
            !argv.iter().any(|a| {
                a.starts_with("--output=go-template")
                    || a.starts_with("-o=go-template")
                    || matches!(*a, "--exec" | "--attach" | "-it")
            })
        }
        Some("config") => {
            // Only read-only config subcommands
            matches!(
                argv.get(2).copied(),
                Some("view" | "get-contexts" | "current-context" | "get-clusters" | "get-users")
            )
        }
        _ => false,
    }
}

// ── Utilities ──

/// Check if sed -n argument is a valid print pattern (digits or range + 'p').
fn is_valid_sed_print(arg: Option<&str>) -> bool {
    let s = match arg {
        Some(s) => s,
        None => return false,
    };
    let core = match s.strip_suffix('p') {
        Some(rest) => rest,
        None => return false,
    };
    let parts: Vec<&str> = core.split(',').collect();
    match parts.as_slice() {
        [num] => !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()),
        [a, b] => {
            !a.is_empty()
                && !b.is_empty()
                && a.chars().all(|c| c.is_ascii_digit())
                && b.chars().all(|c| c.is_ascii_digit())
        }
        _ => false,
    }
}

/// Extract the executable name from a path (e.g., "/usr/bin/git" -> "git").
fn executable_name(cmd: &str) -> &str {
    cmd.rsplit('/').next().unwrap_or(cmd)
}

/// Naive command splitting into argv (handles basic quoting).
fn split_command_to_argv(command: &str) -> Vec<&str> {
    // For simple commands, split on whitespace. This handles the common case.
    // Complex quoting (nested quotes, escapes) requires a full parser.
    command.split_whitespace().collect()
}

#[cfg(test)]
#[path = "read_only.test.rs"]
mod tests;
