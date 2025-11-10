# Sandbox & Approval Best Practices

**Last Updated:** 2025-11-10
**Audience:** Codex users and developers

## Overview

This guide provides practical recommendations for using Codex's sandbox and approval systems effectively. For architectural details, see [`docs/architecture/sandbox-approval-integration.md`](../architecture/sandbox-approval-integration.md).

---

## Quick Start Recommendations

### For New Users

**Start conservative:**
```bash
# First time in a new codebase
codex --sandbox read-only --ask-for-approval untrusted
```

**Benefits:**
- Every action requires explicit approval
- Known-safe commands (ls, cat, git status) auto-approved
- Learn what Codex wants to do before trusting it
- Zero risk of unintended modifications

**When to trust:**
- After reviewing several approval requests
- When you understand the task Codex is performing
- In version-controlled repositories (easy to revert)
- Never in production directories

### For Experienced Users

**Trusted repositories:**
```bash
# Mark directory as trusted (one-time)
# Codex will prompt on first run in new directory

# Or manually configure
mkdir -p .codex
cat >> .codex/config.toml << 'EOF'
[projects."."]
trust_level = "trusted"
EOF
```

**Effect:**
- Switches to "auto" preset
- Workspace writes allowed without approval
- Model only asks to leave workspace or access network
- Faster iteration

### For CI/CD Pipelines

**Read-only analysis:**
```bash
codex exec --sandbox read-only --ask-for-approval never "Analyze code quality"
```

**Benefits:**
- No user interaction required
- Safe for automated environments
- Failures returned to model
- Predictable behavior

**Full automation (use with caution):**
```bash
codex exec --yolo "Run tests and fix failures"
```

**Risks:**
- No sandbox protection
- No approval gates
- Codex can modify anything
- Only use in isolated containers

---

## Configuration Strategies

### By Use Case

#### Exploring Unfamiliar Code

**Goal:** Maximum safety while browsing

```toml
[profiles.explore]
approval_policy = "untrusted"
sandbox_mode = "read-only"

[profiles.explore.shell_environment_policy]
inherit = "core"  # Minimal environment variables
exclude = ["*TOKEN*", "*KEY*", "*SECRET*"]
```

**Usage:**
```bash
codex --profile explore
```

**What this does:**
- Read-only filesystem access
- Every command requires approval (except known-safe)
- Secrets excluded from environment
- Codex can answer questions but not modify code

---

#### Active Development (Trusted Repo)

**Goal:** Fast iteration with safety net

```toml
[profiles.dev]
approval_policy = "on-request"
sandbox_mode = "workspace-write"

[sandbox_workspace_write]
network_access = false
exclude_tmpdir_env_var = false
exclude_slash_tmp = false

# Allow writes to common cache directories
writable_roots = [
    "~/.cache",
    "~/.local/share"
]
```

**Usage:**
```bash
codex --profile dev
```

**What this does:**
- Codex can edit files in workspace freely
- Cannot access network (prevents accidental API calls, git push)
- Cannot leave workspace without approval
- .git/ is always read-only (prevents accidental commits)

---

#### Testing & Verification

**Goal:** Allow network for package downloads, still ask for risky operations

```toml
[profiles.test]
approval_policy = "on-failure"
sandbox_mode = "workspace-write"

[sandbox_workspace_write]
network_access = true  # Allow npm install, cargo build, etc.
```

**Usage:**
```bash
codex --profile test "Run tests and fix failures"
```

**What this does:**
- First attempt always runs in sandbox
- Network access for package managers
- Only prompts if sandbox denies a command
- Good for semi-automated workflows

---

#### Code Review Assistant

**Goal:** Analyze without modifying

```toml
[profiles.review]
approval_policy = "never"
sandbox_mode = "read-only"
```

**Usage:**
```bash
codex --profile review "Review PR #123 for security issues"
```

**What this does:**
- Codex reads code but cannot modify
- No approval prompts (non-interactive)
- Perfect for generating review comments
- Safe for production code

---

#### Maximum Autonomy (Trusted Environment Only)

**Goal:** Hands-off development in containerized/VM environment

```toml
[profiles.yolo]
approval_policy = "never"
sandbox_mode = "danger-full-access"
```

**Usage:**
```bash
# Inside Docker container or VM
codex --profile yolo "Implement feature X, run tests, commit changes"
```

**Risks:**
- No safeguards whatsoever
- Codex can do anything
- Only use in:
  - Disposable containers
  - Virtual machines
  - Isolated development environments
- **NEVER** in production or on host machine with important data

---

## Trust Management

### When to Trust a Directory

**Good candidates:**
✅ Git repositories (easy to revert)
✅ Project directories you actively develop
✅ Code you understand and review regularly
✅ Sandboxed environments (containers, VMs)

**Bad candidates:**
❌ Your home directory
❌ System directories (/usr, /etc, /var)
❌ Production code directories
❌ Directories with untracked secrets
❌ Third-party code you haven't reviewed

### Trust Resolution Behavior

**Git repositories:**
```bash
cd ~/projects/myapp/src/components
codex
# Trust prompt will offer to trust: ~/projects/myapp (repo root)
# NOT ~/projects/myapp/src/components
```

**Non-git directories:**
```bash
cd ~/scripts
codex
# Trust prompt will offer to trust: ~/scripts (exact directory)
```

**Implication:**
- Trusting a git repo trusts the entire repo
- Trusting a non-git dir only trusts that specific directory
- Subdirectories of non-git directories are NOT trusted

### Reviewing Trust Settings

**Check current trust:**
```bash
# Global config
cat ~/.codex/config.toml | grep -A 2 '\[projects\.'

# Project config
cat .codex/config.toml | grep -A 2 '\[projects\.'
```

**Remove trust:**
```toml
# Edit config and remove the projects section
# or set:
[projects."/path/to/repo"]
trust_level = "untrusted"
```

---

## Approval Workflow Tips

### Interpreting Approval Prompts

**Exec approval shows:**
```
Command: git push origin feature-branch
Working directory: /home/user/myproject
Reason: Write to remote repository
Risk: HIGH - Exposes code to external network
```

**Questions to ask yourself:**
- Is this command expected for the task?
- Does the working directory make sense?
- Do I understand the risk?
- Would I run this command myself?

### When to Use "Approve for Session"

**Good use cases:**
```
✅ npm install (during development session)
✅ cargo test (running tests repeatedly)
✅ git status (checking repo state)
✅ ls, cat, grep (browsing code)
```

**Bad use cases:**
```
❌ git push (should confirm each time)
❌ rm -rf (destructive operations)
❌ curl to untrusted URLs (security risk)
❌ Commands with dynamic arguments
```

**Why:**
- "For session" caches the approval for all similar commands
- Safe for read-only or idempotent operations
- Risky for destructive or sensitive operations
- Cache persists until session restart

### Handling Sandbox Denials

**Typical denial scenario:**
```
Command blocked: git push origin main
Sandbox: workspace-write (no network)
stderr: Permission denied

Retry without sandbox? [y/n/a]
```

**Decision tree:**
1. **Is this expected?**
   - git push needs network → Expected denial
   - ls getting blocked → Unexpected, possible bug

2. **Is it safe to retry without sandbox?**
   - git push to trusted remote → Usually safe
   - curl to random URL → Review carefully

3. **Should I change config instead?**
   - If this happens often → Enable network in workspace-write
   - If one-time operation → Approve retry

**Changing config instead:**
```toml
# Enable network for this session
[sandbox_workspace_write]
network_access = true
```

Then restart Codex. Better than repeatedly approving retries.

---

## Security Best Practices

### Environment Variable Hygiene

**Principle:** Only expose what's necessary

**Recommended config:**
```toml
[shell_environment_policy]
inherit = "core"  # PATH, HOME, USER, SHELL, TERM, TMPDIR, PWD

# Explicitly exclude sensitive variables
exclude = [
    "AWS_*",
    "AZURE_*",
    "GCP_*",
    "*TOKEN*",
    "*SECRET*",
    "*KEY*",
    "*PASSWORD*",
    "NPM_TOKEN",
    "GITHUB_TOKEN",
]

# Only include what's needed for your workflow
include_only = [
    "PATH",
    "HOME",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "NODE_ENV",
]

# Set non-sensitive context
[shell_environment_policy.set]
CI = "0"
CODEX_SESSION = "true"
```

**Why:**
- Reduces risk of credential leakage
- Model can't accidentally use sensitive tokens
- Easier to audit environment in logs

### Writable Roots Management

**Principle:** Minimal write access

**Good configuration:**
```toml
[sandbox_workspace_write]
# Only project directory writable
writable_roots = []  # Default: just cwd, /tmp, $TMPDIR

# Exclude temp dirs if not needed
exclude_slash_tmp = true
exclude_tmpdir_env_var = true
```

**When to add writable roots:**
```toml
# Package manager caches (safe, improves performance)
writable_roots = [
    "~/.cache",
    "~/.cargo",
    "~/.npm",
]

# Build output directories (safe, necessary for builds)
writable_roots = [
    "./target",
    "./build",
    "./dist",
]
```

**Never add:**
❌ Home directory (`~`)
❌ System directories (`/usr`, `/etc`)
❌ Parent directories of workspace
❌ Directories with secrets

### Git Directory Protection

**Built-in protection:**
- `.git/` is always read-only in workspace-write mode
- Prevents accidental:
  - `git push`
  - `git commit --amend`
  - `git rebase`
  - `.git/config` modification

**Manual git operations:**
If Codex needs to commit:
```bash
# Codex will request escalated permission
# You'll see approval prompt:
# "Request write access to .git directory"

# Review the git operation carefully before approving
```

**Recommendation:**
- Let Codex prepare code
- Manually review and commit
- Maintain control over git history

### Monitoring Codex Activity

**Check history:**
```bash
# All executed commands logged
cat ~/.codex/history.jsonl | jq -r '.command' | tail -20
```

**Watch for:**
- Unexpected network access
- Writes outside workspace
- Access to sensitive files
- Unusual command patterns

**Set up alerts (advanced):**
```bash
# Monitor history file for sensitive patterns
tail -f ~/.codex/history.jsonl | grep -E '(aws|secret|token|password)' --color
```

---

## Performance Optimization

### Approval Caching

**Use "Approve for Session" strategically:**

**For repetitive read operations:**
```bash
# Codex is browsing code
codex: "Can I run: cat src/main.rs"
you: [Approve for Session]

# Next time Codex wants to cat any file:
# → Auto-approved, no prompt
# → Faster workflow
```

**Implementation:**
- Approval cache is in-memory only
- Keyed by command pattern
- Cleared on session restart

**Cache key examples:**
```
exec:cat src/main.rs           → Caches "cat" commands
exec:git status                → Caches "git status"
patch:/path/to/file            → Caches patches to that file
```

### Risk Assessment Overhead

**Feature:** `experimental_sandbox_command_assessment = true`

**Trade-off:**
- Adds ~5 seconds per sandbox denial
- Provides AI-powered risk analysis
- Helps make informed decisions

**When to enable:**
```toml
# Enable for learning/exploration
[features]
experimental_sandbox_command_assessment = true
```

**When to disable:**
```toml
# Disable for fast iteration
[features]
experimental_sandbox_command_assessment = false
```

**Alternative:** Use `on-failure` approval policy to skip initial prompts entirely

### Parallel Tool Execution

**How it works:**
- Read-only tools execute concurrently
- Write tools execute serially
- Mixed → Serial fallback

**User impact:**
- Faster for multi-tool queries
- "Search X, read Y, analyze Z" → All parallel
- "Edit X, edit Y, run tests" → Serial (safe)

**No configuration needed** - automatic optimization

---

## Troubleshooting

### Common Issues

#### Issue: Codex keeps asking for approval

**Symptoms:**
- Every command prompts for approval
- "Approve for Session" doesn't stick

**Causes:**
1. Approval policy is `untrusted`
2. Commands have dynamic arguments
3. Cache key mismatch

**Solutions:**
```bash
# Check current policy
codex /status

# Switch to on-request
codex --ask-for-approval on-request

# Or trust the directory
# (TUI will prompt, or edit .codex/config.toml)
```

**For dynamic commands:**
- Cache keys are command-specific
- `git status` is cached separately from `git diff`
- "Approve for Session" only applies to exact command pattern

---

#### Issue: Sandbox blocks legitimate operations

**Symptoms:**
- `git push` fails with "Permission denied"
- `npm install` fails with network error
- Build tools can't download dependencies

**Cause:**
- Workspace-write sandbox blocks network by default

**Solutions:**

**Option 1: Enable network in config**
```toml
[sandbox_workspace_write]
network_access = true
```

**Option 2: Approve retry when prompted**
```
Command blocked: git push origin main
Retry without sandbox? [y] Yes
```

**Option 3: Use different sandbox mode**
```bash
codex --sandbox danger-full-access
# or
codex --yolo  # (alias)
```

---

#### Issue: Sandbox unavailable on platform

**Symptoms:**
- Warning: "Sandbox not available, using danger-full-access"
- Commands run without sandbox protection

**Causes:**
- Linux: Kernel doesn't support Landlock (< 5.13)
- Windows: Experimental feature not enabled
- macOS: sandbox-exec not found (rare)
- Running inside container without proper capabilities

**Solutions:**

**Check sandbox support:**
```bash
# macOS
which sandbox-exec

# Linux
codex sandbox linux ls
# If fails: kernel too old or Landlock not compiled

# Windows
# Check features in config.toml
```

**Workarounds:**

**Linux (old kernel):**
```bash
# Run Codex inside Docker with newer kernel
docker run -it -v $(pwd):/workspace ubuntu:22.04
# Install Codex in container
```

**Windows:**
```toml
[features]
enable_experimental_windows_sandbox = true
```
⚠️ Experimental, may not work reliably

**Container environments:**
```bash
# Use container isolation instead
# Run with --sandbox danger-full-access inside container
docker run --rm -it \
  -v $(pwd):/workspace \
  codex-container \
  codex --yolo "task"
```

---

#### Issue: .git directory is read-only

**Symptoms:**
- Codex can't run `git commit`
- `git push` blocked even with network access
- Manual git operations inside Codex session fail

**Cause:**
- Built-in protection: .git/ is always read-only in workspace-write mode

**This is by design:**
- Prevents accidental history modification
- Forces explicit approval for git operations

**Solutions:**

**Option 1: Request escalated permission**
- Codex will ask to retry without sandbox
- Approve when safe (trusted repo, reviewed changes)

**Option 2: Manual git workflow**
```bash
# Let Codex prepare code
codex "Implement feature X"

# Exit Codex, manually commit
git add .
git commit -m "Implement feature X"
git push
```

**Option 3: Dangerous full access (not recommended)**
```bash
codex --yolo
# .git/ is writable, no protection
```

---

### Debugging Sandbox Issues

**Check if sandbox is active:**
```bash
# Inside a Codex tool execution
echo $CODEX_SANDBOX
# macOS: "seatbelt"
# Linux: "landlock"
# None: (empty)

echo $CODEX_SANDBOX_NETWORK_DISABLED
# "1" if network blocked
# (empty) if network allowed
```

**Test sandbox manually:**
```bash
# macOS
codex sandbox macos --full-auto ls -la

# Linux
codex sandbox linux --full-auto ls -la

# Run arbitrary command in sandbox
codex sandbox macos --full-auto /bin/bash
# Now you're in a sandboxed shell
# Try operations to test restrictions
```

**Check sandbox logs:**
```bash
# macOS seatbelt logs
log show --predicate 'process == "sandbox-exec"' --last 1h

# Linux (check dmesg for Landlock denials)
sudo dmesg | grep -i landlock
```

---

## Advanced Patterns

### Project-Specific Presets

**Scenario:** Different projects need different configurations

**Project A (web app):**
```bash
# mywebapp/.codex/config.toml
[projects."."]
trust_level = "trusted"

approval_policy = "on-request"
sandbox_mode = "workspace-write"

[sandbox_workspace_write]
network_access = true  # npm install, API testing
writable_roots = [
    "./node_modules",
    "./dist",
    "~/.npm",
]
```

**Project B (security-critical):**
```bash
# security-tool/.codex/config.toml
[projects."."]
trust_level = "untrusted"  # Force explicit approval

approval_policy = "untrusted"
sandbox_mode = "read-only"

[shell_environment_policy]
inherit = "none"  # Minimal environment
include_only = ["PATH", "HOME", "CARGO_HOME"]
```

**Effect:**
- Codex automatically adapts to project context
- No need to remember flags per project
- Consistent team configuration (commit .codex/config.toml)

### Multi-Stage Workflows

**Pattern:** Different phases have different risk profiles

**Stage 1: Research & Planning (safe)**
```bash
codex --profile readonly "Analyze codebase and plan refactoring"
```

**Stage 2: Implementation (controlled)**
```bash
codex --profile dev "Implement planned refactoring"
```

**Stage 3: Testing (network needed)**
```bash
codex --profile test "Run tests and fix failures"
```

**Stage 4: Review (safe)**
```bash
codex --profile readonly "Review changes and suggest improvements"
```

**Stage 5: Commit (manual)**
```bash
git add .
git commit -m "Refactoring based on Codex analysis"
git push
```

### Subagent Delegation

**Pattern:** Nested approval workflows

**Scenario:** Code review with auto-fix

```bash
codex "Review PR #123 and fix any issues found"
```

**What happens:**
1. Parent session (user-facing)
2. Spawns Review subagent
3. Review subagent finds issues
4. Review subagent wants to edit files
5. Approval request **forwarded to parent**
6. User sees approval in parent session UI
7. User approves
8. Review subagent continues with edits

**Benefit:**
- Single approval UI (parent session)
- No nested prompts
- Clear delegation hierarchy
- User always in control

**Cancellation:**
- Aborting parent cancels all subagents
- Clean cleanup of entire task tree

---

## Migration Guide

### From "YOLO" to Safer Configuration

**Current state:**
```bash
codex --yolo  # No sandbox, no approvals
```

**Problems:**
- High risk of unintended changes
- No audit trail of approved actions
- Can't review before execution

**Migration path:**

**Step 1: Add sandbox**
```bash
codex --sandbox workspace-write --ask-for-approval never
```
- Still no prompts
- But sandboxed (can't leave workspace)
- Network blocked by default

**Step 2: Add selective approval**
```bash
codex --sandbox workspace-write --ask-for-approval on-failure
```
- Auto-approve first attempt
- Only prompt if sandbox blocks
- Good for iterative development

**Step 3: Full control (final state)**
```bash
codex --sandbox workspace-write --ask-for-approval on-request
```
- Model decides when to ask
- You review risky operations
- Fast iteration with safety net

### From "Untrusted" to Trusted Workflow

**Current state:**
```bash
codex --ask-for-approval untrusted
```

**Too many prompts for trusted repos**

**Migration:**

**Option 1: Trust directory**
```bash
# TUI will prompt, or manually:
mkdir -p .codex
cat >> .codex/config.toml << 'EOF'
[projects."."]
trust_level = "trusted"
EOF
```

**Option 2: Use "on-request" policy**
```bash
codex --ask-for-approval on-request
```

**Effect:**
- Fewer prompts
- Known-safe commands auto-approved
- Model asks when genuinely risky

---

## Testing Configurations

### Safe Testing Environment

**Before changing config:**

**Create test workspace:**
```bash
mkdir -p /tmp/codex-test
cd /tmp/codex-test
git init
echo "# Test" > README.md
git add README.md
git commit -m "Initial commit"
```

**Test new configuration:**
```bash
# Copy your config
cp ~/.codex/config.toml .codex/config.toml

# Edit .codex/config.toml with new settings

# Test
codex "Make a small change to README.md"
```

**Verify:**
```bash
git diff  # Check what changed
git log   # Check if Codex committed (shouldn't in default config)
```

**If satisfied:**
```bash
cp .codex/config.toml ~/.codex/config.toml
```

### Configuration Validation

**Check current effective config:**
```bash
codex /status
```

**Shows:**
- Active approval policy
- Active sandbox mode
- Writable roots
- Network access status
- Trust level of current directory

**Validate config file syntax:**
```bash
# Codex will report errors on startup
codex
# Look for warnings:
# "Warning: Invalid config.toml: ..."
```

**Common syntax errors:**
```toml
# ❌ Wrong quotes
writable_roots = ['~/.cache']  # Single quotes don't expand ~

# ✅ Correct
writable_roots = ["~/.cache"]

# ❌ Missing section header
approval_policy = "on-request"
sandbox_mode = "workspace-write"
network_access = true  # Error: network_access needs [sandbox_workspace_write]

# ✅ Correct
approval_policy = "on-request"
sandbox_mode = "workspace-write"

[sandbox_workspace_write]
network_access = true
```

---

## Quick Reference

### Configuration Matrix

| Use Case | Approval | Sandbox | Network | Writable Roots |
|----------|----------|---------|---------|----------------|
| Exploring code | `untrusted` | `read-only` | ❌ | None |
| Dev (trusted) | `on-request` | `workspace-write` | ❌ | cwd, /tmp |
| Testing | `on-failure` | `workspace-write` | ✅ | cwd, /tmp, ~/.cache |
| CI/CD read | `never` | `read-only` | ❌ | None |
| CI/CD write | `never` | `workspace-write` | ✅ | cwd, /tmp |
| YOLO | `never` | `danger-full-access` | ✅ | Everything |

### Command Cheat Sheet

```bash
# Safe browsing
codex --sandbox read-only --ask-for-approval on-request

# Trusted development
codex --full-auto
# Equivalent to:
codex --sandbox workspace-write --ask-for-approval on-request

# Test sandbox manually
codex sandbox macos ls -la    # macOS
codex sandbox linux ls -la    # Linux

# Check current config
codex /status

# Use named profile
codex --profile paranoid

# CI/CD mode
codex exec --sandbox read-only --ask-for-approval never "task"

# Maximum autonomy (dangerous)
codex --yolo
```

### Approval Policy Decision Tree

```
What level of control do you want?

├─ Trust Codex completely in this environment?
│  └─ YES → approval_policy = "never"
│
├─ Review every command before execution?
│  └─ YES → approval_policy = "untrusted"
│
├─ Only intervene when sandbox blocks?
│  └─ YES → approval_policy = "on-failure"
│
└─ Let model decide when to ask?
   └─ YES → approval_policy = "on-request" (recommended)
```

### Sandbox Mode Decision Tree

```
What can Codex modify?

├─ Nothing (read-only analysis)?
│  └─ sandbox_mode = "read-only"
│
├─ Only the workspace?
│  └─ sandbox_mode = "workspace-write"
│     └─ Need network?
│        ├─ YES → network_access = true
│        └─ NO → (default, network blocked)
│
└─ Everything (trusted environment only)?
   └─ sandbox_mode = "danger-full-access"
```

---

## Related Documentation

- **Architecture:** [`../architecture/sandbox-approval-integration.md`](../architecture/sandbox-approval-integration.md)
- **Flow details:** [`../architecture/tool-execution-orchestration.md`](../architecture/tool-execution-orchestration.md)
- **User guide:** [`../sandbox.md`](../sandbox.md)
- **Config reference:** [`../config.md`](../config.md)

---

## Changelog

| Date | Author | Changes |
|------|--------|---------|
| 2025-11-10 | Initial | Created best practices guide |
