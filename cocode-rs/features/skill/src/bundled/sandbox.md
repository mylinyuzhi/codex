<command-name>sandbox</command-name>

# Sandbox Status

You are handling the `/sandbox` command. Gather the full sandbox state from the current session and present it in the structured format below.

## Information to Gather

1. **Enforcement level** — one of: Disabled, ReadOnly, WorkspaceWrite, Strict.
2. **Platform enforcement** — whether the kernel-level sandbox is active (Seatbelt on macOS, bubblewrap on Linux) and the platform implementation in use.
3. **Filesystem configuration**:
   - `allow_write` paths (writable roots, including their read-only subpath protections)
   - `deny_write` paths
   - `deny_read` paths
   - `allow_git_config` flag (whether `.git/config` and `~/.gitconfig` are writable)
4. **Network configuration**:
   - `allowed_domains` (if non-empty, only these domains are permitted)
   - `denied_domains` (always blocked, takes precedence over allow list)
   - `allow_unix_sockets` paths and `allow_all_unix_sockets` flag
   - `allow_local_binding` flag
   - HTTP proxy port and SOCKS proxy port (if network isolation is active)
   - MITM proxy config (if configured)
5. **Seccomp status** (Linux only) — whether a BPF filter path and apply binary are configured.
6. **Violation summary** — total violations since session start, non-benign count in ring buffer, and the 5 most recent non-benign violations (operation, path, timestamp).
7. **Platform dependencies** — for each required and optional binary, whether it was found and its path:
   - macOS: `sandbox-exec`
   - Linux: `bwrap` (required), `socat` (optional, needed for network bridge)
8. **Permission mode**:
   - `auto_allow_bash_if_sandboxed` — whether bash commands auto-approve when sandbox is active
   - `allow_unsandboxed_commands` — whether `dangerouslyDisableSandbox` bypass is permitted
9. **Excluded commands** — command patterns that skip sandbox wrapping.
10. **Advanced flags**:
    - `enable_weaker_nested_sandbox` (Docker/WSL environments)
    - `enable_weaker_network_isolation` (macOS Go TLS)
    - `allow_pty` (pseudo-terminal access, macOS)
    - `enabled_platforms` list

## Response Format

Present the information using this exact structure. Omit sections that do not apply to the current platform. Replace bracketed placeholders with actual values.

```
Sandbox Status
==============

Enforcement:  [Disabled | ReadOnly | WorkspaceWrite | Strict]
Platform:     [macOS Seatbelt | Linux bubblewrap | not active]
Active:       [yes | no]

Filesystem
----------
  Writable paths:     [list of allow_write paths, or "none"]
    Read-only within: [subpaths protected within each writable root, e.g. .git, .cocode]
  Deny write:         [list, or "none"]
  Deny read:          [list, or "none"]
  Git config writes:  [allowed | denied]

Network
-------
  Isolation active:   [yes | no]
  Allowed domains:    [list, or "all" if empty]
  Denied domains:     [list, or "none"]
  Unix sockets:       [specific paths, "all allowed", or "blocked"]
  Local binding:      [allowed | denied]
  HTTP proxy port:    [port or "not active"]
  SOCKS proxy port:   [port or "not active"]
  MITM proxy:         [socket path + intercepted domains, or "not configured"]

Seccomp (Linux)
---------------
  BPF filter:    [path or "not configured"]
  Apply binary:  [path or "not configured"]

Violations
----------
  Total (session):    [count]
  Non-benign (buffer): [count]
  Recent:
    - [timestamp] [operation] [path]
    ...

Dependencies
------------
  [binary name]: [found at /path | MISSING (required) | MISSING (optional)]
  ...

Permission Mode
---------------
  Auto-allow bash: [yes — commands auto-approve in sandbox | no — normal approval required]
  Bypass allowed:  [yes — dangerouslyDisableSandbox permitted | no — bypass blocked]

Excluded Commands
-----------------
  [list of patterns, or "none"]

Advanced
--------
  Weaker nested sandbox:     [enabled | disabled]
  Weaker network isolation:  [enabled | disabled]
  PTY access:                [allowed | denied]
  Enabled platforms:         [list]
```

If sandbox is disabled, still show the full status (so the user can see why) and append a note:

```
Note: Sandbox is currently disabled. To enable it, set "sandbox.enabled = true"
in your configuration. Required dependencies must also be available for your
platform.
```

## No Arguments

If the user runs `/sandbox` with no arguments, show the full status as described above.

## Configuration Guide

After showing the status, include a brief configuration guide at the bottom:

```
Configuration
=============
To change sandbox mode, update ~/.cocode/settings.local.json:

  Option 1 — Sandbox with auto-allow (recommended):
    { "sandbox": { "enabled": true, "autoAllowBashIfSandboxed": true } }

  Option 2 — Sandbox with manual approval:
    { "sandbox": { "enabled": true, "autoAllowBashIfSandboxed": false } }

  Option 3 — Disable sandbox:
    { "sandbox": { "enabled": false } }

To add writable paths:    "sandbox": { "filesystem": { "allowWrite": ["/path"] } }
To allow network domains: "sandbox": { "network": { "allowedDomains": ["*.example.com"] } }
To exclude commands:      "sandbox": { "excludedCommands": ["npm:*", "docker compose *"] }
```

## Important

- Report actual values from the running session state, not defaults or examples.
- When a list is empty, display "none" rather than an empty line.
- For violations, show only non-benign entries. If there are zero violations, say "No violations recorded."
- Keep the output compact; do not add explanatory prose between sections.
