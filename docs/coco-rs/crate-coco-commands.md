# coco-commands — Crate Plan

TS source: `src/commands.ts`, `src/commands/` (86 command dirs + 15 top-level command files = ~100 total)

## Dependencies

```
coco-commands depends on:
  - coco-types    (Message, SessionId)
  - coco-tool     (ToolRegistry — for command context)
  - coco-config   (Settings, EffortLevel, FastModeState)
  - coco-error

coco-commands does NOT depend on:
  - coco-tools    (no concrete tool implementations)
  - coco-inference (commands that need LLM receive ApiClient via CommandContext)
  - coco-query    (no query engine knowledge)
  - any app/ crate
```

## Data Definitions

### Command Trait

```rust
#[async_trait]
pub trait Command: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn aliases(&self) -> &[&str] { &[] }
    fn argument_hint(&self) -> Option<&str> { None }
    fn is_hidden(&self) -> bool { false }
    fn user_invocable(&self) -> bool { true }
    fn is_enabled(&self) -> bool { true }

    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> CommandResult;
}

pub enum CommandResult {
    Text(String),              // display message
    InjectPrompt(String),      // inject as user input
    Compact(CompactionResult), // compaction result
    Skip,                      // no output
}
```

### Command Registry

```rust
pub struct CommandRegistry {
    commands: Vec<Arc<dyn Command>>,
}

impl CommandRegistry {
    /// Load from: bundled skills -> plugin skills -> skill dirs -> builtins
    pub fn new(cwd: &Path, plugins: &[LoadedPlugin]) -> Self;
    pub fn find(&self, name: &str) -> Option<&Arc<dyn Command>>;
    pub fn all_names(&self) -> HashSet<String>;
}
```

## Complete Command Inventory

### v1 Core Commands (34) — essential for basic agent functionality

| Command | Purpose | Category |
|---------|---------|----------|
| `add-dir` | Add directory to permission scope | Session |
| `clear` | Clear/compact conversation | Session |
| `compact` | Force conversation compaction | Session |
| `config` | Edit settings.json | Config |
| `context` | View session context/CLAUDE.md | Session |
| `cost` | Show token/cost tracking | Info |
| `diff` | Show file diffs since session start | File |
| `doctor` | Run diagnostic checks | Diagnostic |
| `effort` | Set effort level (low/medium/high/max) | Config |
| `exit` | Exit session | Session |
| `fast` | Toggle fast mode | Config |
| `feedback` | Send feedback | Info |
| `files` | Manage watched files | File |
| `help` | Show command help | Info |
| `hooks` | Configure lifecycle hooks | Config |
| `ide` | Configure IDE integration | Integration |
| `init` | Initialize .claude/ directory | Setup |
| `keybindings` | Configure keyboard shortcuts | Config |
| `login` | Authenticate | Auth |
| `logout` | Sign out | Auth |
| `mcp` | MCP server management | Integration |
| `memory` | View/manage memory | Memory |
| `model` | Set LLM model | Config |
| `permissions` | View/edit permission rules | Config |
| `plan` | Toggle plan mode | Session |
| `plugin` | Plugin management | Integration |
| `resume` | Resume previous session | Session |
| `review` | AI code review | Agent |
| `session` | Session management | Session |
| `skills` | Manage skills | Integration |
| `status` | Show session status | Info |
| `tasks` | View background tasks | Agent |
| `theme` | Theme customization | Config |
| `usage` | Show usage stats | Info |
| `version` | Show version | Info |

### v1 Extended Commands (20) — secondary but needed for completeness

| Command | Purpose | Category |
|---------|---------|----------|
| `branch` | Git branch management | File |
| `color` | Color palette display | Config |
| `copy` | Copy last response to clipboard | Info |
| `env` | Show environment variables | Diagnostic |
| `export` | Export session transcript | Session |
| `install` | Install CLI/desktop app | Setup |
| `oauth-refresh` | Refresh OAuth tokens | Auth |
| `onboarding` | Run onboarding flow | Setup |
| `output-style` | Set output style | Config |
| `privacy-settings` | Configure privacy | Config |
| `rate-limit-options` | Rate limit settings | Config |
| `release-notes` | Show release notes | Info |
| `reload-plugins` | Hot-reload plugins | Integration |
| `rename` | Rename session | Session |
| `reset-limits` | Reset rate limits | Config |
| `rewind` | Rewind to earlier turn | Session |
| `sandbox-toggle` | Toggle sandbox mode | Config |
| `share` | Share session | Session |
| `stats` | Show detailed statistics | Info |
| `statusline` | Configure status line | Config |

### v1 Top-Level File Commands (10) — defined as .ts files, not directories

| Command | Purpose | Category |
|---------|---------|----------|
| `advisor` | Advisor tool configuration | Agent |
| `brief` | Generate session brief | Agent |
| `commit` | Git commit helper | File |
| `commit-push-pr` | Commit + push + PR workflow | File |
| `init-verifiers` | Initialize verification hooks | Setup |
| `bridge-kick` | Restart IDE bridge | Integration |
| `createMovedToPluginCommand` | Migration helper for commands moved to plugins | Migration |
| `security-review` | Security-focused code review | Agent |
| `insights` | Usage insights dashboard | Info |
| `ultraplan` | Ultra-plan mode (advanced planning) | Agent |

### v2 Commands (15) — advanced/platform features

| Command | Purpose | Category |
|---------|---------|----------|
| `agents` | Multi-agent management | Agent |
| `autofix-pr` | Auto-fix PR issues | Agent |
| `chrome` | Chrome extension integration | Platform |
| `desktop` | Desktop app features | Platform |
| `mobile` | Mobile integration | Platform |
| `voice` | Voice mode toggle | Platform |
| `vim` | Vim mode configuration | Config |
| `remote-env` | Remote environment config | Integration |
| `remote-setup` | Remote setup wizard | Integration |
| `teleport` | Cross-machine session transfer | Session |
| `tag` | Tag sessions/messages | Session |
| `summary` | Generate session summary | Agent |
| `install-github-app` | Install GitHub App | Integration |
| `install-slack-app` | Install Slack App | Integration |
| `pr_comments` | PR comment management | Integration |

### v3/Internal Commands (7) — diagnostic/experimental

| Command | Purpose | Category |
|---------|---------|----------|
| `ant-trace` | Internal tracing | Diagnostic |
| `bughunter` | Bug hunting mode | Diagnostic |
| `ctx_viz` | Context visualization | Diagnostic |
| `debug-tool-call` | Debug tool calls | Diagnostic |
| `heapdump` | Memory heap dump | Diagnostic |
| `perf-issue` | Performance issue reporter | Diagnostic |
| `mock-limits` | Mock rate limits (testing) | Testing |

### Skipped (3) — trivial or cosmetic

| Command | Purpose |
|---------|---------|
| `break-cache` | Dev-only cache invalidation |
| `btw` | Easter egg |
| `good-claude` | Easter egg (companion) |
| `extra-usage` | Internal usage tracking |
| `passes` | Internal pass tracking |
| `stickers` | Cosmetic sticker system |
| `thinkback` / `thinkback-play` | Thinking playback (experimental) |
| `backfill-sessions` | Internal migration tool |
| `terminalSetup` | Terminal setup wizard |
| `issue` | Bug report helper |

### Count Summary

| Version | Commands | Status |
|---------|----------|--------|
| v1 core | 34 | Must implement |
| v1 extended | 20 | Implement for feature parity |
| v1 file commands | 10 | Implement |
| v2 | 15 | Deferred |
| v3/internal | 7 | Deferred |
| Skipped | ~10 | Not porting |
| **Total** | **~96** | |
