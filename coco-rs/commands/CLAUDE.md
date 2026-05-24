# coco-commands

Slash command registry and built-in implementations (help, config, clear, compact, model, session, mcp, plugin, diff, commit, pr, review, doctor, ...). ~96 commands across v1/v2/v3 in TS.

## TS Source
- `commands.ts` — top-level registry (loads + registers builtins)
- `commands/` — 88+ subdirs, one per command (each with its own `.ts` or `.tsx`)
- Top-level file commands: `commands/commit.ts`, `commit-push-pr.ts`, `bridge-kick.ts`, `brief.ts`, `advisor.ts`, `createMovedToPluginCommand.ts`


## Key Types
- `CommandHandler` trait — `execute(args: &str) -> Result<String>`
- `RegisteredCommand` — metadata (`CommandBase` from coco-types) + optional handler + `is_enabled` feature-flag gate
- `CommandRegistry` — name-keyed map with alias lookup; filter views: `visible()`, `sdk_safe()` (strips `is_sensitive`), `safe_for(CommandSafety)`
- `BuiltinCommand` / `AsyncBuiltinCommand` — sync and async built-in handler wrappers
- `builtin_base()`, `builtin_base_ext()` — construct default `CommandBase` with safety + argument-hint options
- `register_builtins()` — registers the starter ~25; `register_extended_builtins` in `implementations::`

## Modules
- `handlers/` — richer command handlers that need app state
- `implementations/` — extended builtin registrations and shared `names` constants

## Deliberately Not Ported

**Audits and parity reviews should skip the commands listed below — these are
conscious omissions, not gaps.** Do NOT flag them as missing during a TS↔Rust
diff. If a future change re-introduces one of these, remove the corresponding
row from this table and add it to the registry.

### Group A — Provider / account-specific (Anthropic-only flows)

Skipped because the multi-provider scope means no single sign-in / billing /
account-management surface applies across providers.

| Command | TS file | Reason |
|---|---|---|
| `/login` | `commands/login/index.ts` | OAuth flow tied to Anthropic accounts; multi-provider auth lives in `vercel-ai-*` per-provider crates. |
| `/logout` | `commands/logout/index.ts` | Same as `/login`. |
| `/feedback` (and its alias `/bug`) | `commands/feedback/index.ts` | Posts to Anthropic Statsig endpoint; gated on `DISABLE_FEEDBACK_COMMAND` + Bedrock/Vertex/Foundry env. The `/bug` alias is also not registered standalone — pointing users at the upstream Anthropic GitHub repo would mislead them about where to file coco-rs issues. |
| `/fast` | `commands/fast/index.ts` | Claude.ai/console-only fast-mode picker; coco-rs exposes fast-mode via `FastModeState` + Ctrl+Shift+F keybind only. |
| `/release-notes` | `commands/release-notes/index.ts` | Fetches Anthropic-hosted changelog; not slash-invoked in coco-rs (CLI subcommand only). |
| `/privacy-settings` | `commands/privacy-settings/index.ts` | `isConsumerSubscriber()`-gated; calls Anthropic Grove API. |
| `/rate-limit-options` | `commands/rate-limit-options/index.ts` | Claude.ai-only, hidden internal. |
| `/reset-limits` (+ non-interactive) | `commands/reset-limits/index.ts` | TS file is a literal `isEnabled: () => false` stub. |
| `/install-github-app` | `commands/install-github-app/index.ts` | `claude-ai`/`console` availability + Anthropic GitHub App OAuth. |
| `/install-slack-app` | `commands/install-slack-app/index.ts` | `claude-ai` availability + Anthropic Slack App marketplace. |
| `/chrome` | `commands/chrome/index.ts` | `claude-ai` availability; Chrome-extension-only settings UI. |
| `/mobile` (aliases `/ios`, `/android`) | `commands/mobile/index.ts` | claude.ai mobile-app QR flow. |
| `/desktop` (alias `/app`) | `commands/desktop/index.ts` | `claude-ai` + macOS/win32 only; Anthropic desktop client install. |
| `/passes` | `commands/passes/index.ts` | claude.ai referral / Passes program. |
| `/terminal-setup` | `commands/terminalSetup/index.ts` | Anthropic-specific `claude` CLI binding installer. |
| `/extra-usage` (+ non-interactive) | `commands/extra-usage/index.ts` | Anthropic admin-overage request flow. |
| `/think-back` / `/thinkback-play` | `commands/thinkback/`, `commands/thinkback-play/` | Statsig-gated experimental Anthropic feature. |

### Group B — Anthropic-internal stubs / first-party-only

Skipped because the public TS source is already a literal `{ isEnabled: () =>
false, isHidden: true, name: 'stub' }` placeholder, or the feature depends on
Anthropic-internal infrastructure (KAIROS, CCR, advisor API beta) that
coco-rs does not ship.

| Command | TS file | Reason |
|---|---|---|
| `/voice` | `commands/voice/index.ts` | Anthropic `voiceStreamSTT` + GrowthBook `isVoiceModeEnabled`; needs SoX + microphone probes. |
| `/advisor` | `commands/advisor.ts` | Server-side Anthropic API beta `advisor-tool-2026-03-01`, first-party-only. |
| `/ultraplan` | `commands/ultraplan.tsx` | `feature('ULTRAPLAN')`; depends on Claude-Code-on-Web ("CCR") session backend. |
| `/ultrareview` | `commands/review.ts` (named export) | CCR-backed multi-agent review with no local execution path. |
| `/bughunter` | `commands/bughunter/` | TS source is a literal `isEnabled: () => false` stub. |
| `/autofix-pr` | `commands/autofix-pr/` | TS source is a literal stub. |
| `/issue` | `commands/issue/` | TS source is a literal stub. |
| `/onboarding` | `commands/onboarding/` | TS source is a literal stub; in `INTERNAL_ONLY_COMMANDS`. |
| `/share` | `commands/share/` | TS source is a literal stub; in `INTERNAL_ONLY_COMMANDS`. |
| `/teleport` | `commands/teleport/` | TS source is a literal stub; in `INTERNAL_ONLY_COMMANDS`. |
| `/heapdump` | `commands/heapdump/` | Node.js V8 heap snapshot; no Rust runtime equivalent. |
| `/ctx_viz` | `commands/ctx_viz/` | Anthropic-internal context probe; in `INTERNAL_ONLY_COMMANDS`. |
| `/ant-trace` | `commands/ant-trace/index.js` | TS source is a literal stub; original feature was an Anthropic-only OTel trace toggle. |
| `/brief` | `commands/brief.ts` | KAIROS-only (`feature('KAIROS_BRIEF')`); depends on Anthropic-internal `BriefTool`. |

### Re-introducing one of these

If a downstream consumer needs a skipped command, treat it as a feature add,
not a bug fix:
1. Remove the row from the table above.
2. Implement the command in `implementations.rs` or `handlers/`.
3. If the command depends on Anthropic-only infrastructure, hide it behind a
   `Feature` gate so non-Anthropic providers stay clean.

## Deferred (registered but thinned vs TS)

These commands ARE registered and respond, but the body is intentionally
simpler than TS pending follow-up work. Don't flag them as missing — they
are stubs by design — but DO update this table when the gap closes.

| Command | TS reference | Rust state | Gap |
|---|---|---|---|
| `/insights` | `commands/insights.ts` (~3.2K LOC) | `register_static_prompt` with 12-line body in `prompts/insights.txt` | TS uses Opus-driven facet extraction + SCP-from-Coder for remote sessions + JSONL log parsing. Rust delegates the work to the agent via prompt. P3 — port only if non-TS users actually need the local aggregator. |
| `/ide` | `commands/ide/ide.tsx` (645 LOC, IDE bridge) | Static text stub in `ide_handler` | TS has `detectRunningIDEs`, JetBrains/VS Code auto-connect dialogs, MCP cache invalidation. Rust ships the `coco-bridge` crate but the slash command is not wired to it. P2 — wire when bridge UX is finalized. |
| `/help` | `commands/help/help.tsx` (dynamic from registry) | Hardcoded `CATEGORIES` in `handlers/help.rs` | User-installed skills, plugin contributions, and MCP-bridged tools won't appear in `/help` output. P1 — refactor to iterate the live `CommandRegistry`; needs handler-side registry access (currently `CommandHandler::execute_command(&self, args: &str)` doesn't carry one). |
| `/color` | `commands/color/color.ts` (transcript-persistent) | `dispatch_color` writes only to live `app_state.agent_color` | TS persists the choice in the session transcript so it survives restarts. Rust's choice is ephemeral. P3 — wire to settings.json or session metadata. |

## Always-Enabled General-Purpose Commands

These commands are plain Rust features with no gating in coco-rs. The TS
sources happen to wrap them in `isEnabled: () => USER_TYPE === 'ant'`,
but that is an Anthropic-internal visibility convention unrelated to the
command's behavior. **Do not introduce `is_enabled` for these in coco-rs**
— they are intentionally available to every user.

| Command | What it does in coco-rs |
|---|---|
| `/version` | Prints `cocode v{CARGO_PKG_VERSION}`. |
| `/tag` | Toggles a searchable tag on the current session via `SessionManager::toggle_tag` (sentinel-based dispatch). |
| `/files` | Lists `git ls-files` grouped by top-level directory with rough context-size estimate. (Description: "List git-tracked files in this repository".) |

## Rewind / Resume Naming

Two distinct features:

- **`/rewind`** — in-session TUI checkpoint picker (TS
  `commands/rewind/rewind.ts` → `openMessageSelector`). Operates on file-
  history snapshots; touches no transcript-on-disk.
- **`/resume`** — load a prior transcript and continue. CLI form: `--resume`
  / `-r`. Reads JSONL; rebuilds chain via `coco_session::recovery`.

**Canonical names only.** TS ships aliases (`/rewind` → `[checkpoint]`,
`/resume` → `[continue]`); coco-rs intentionally drops them. Single dispatch
arm per command — no `matches!(name, "rewind" | "checkpoint" | "undo")`
fan-out, no alias entries in `RegisteredCommand.base.aliases`. Audits that
reintroduce an alias must first justify why the divergence from this rule is
worth carrying. The historical `/restore` and `--restore` names from an
earlier coco-rs draft are likewise off the table.

## Permission/persistence gaps below the slash-command layer

These items are NOT command-handler bugs but show up in audits because
they manifest as "the command doesn't seem to do anything". They're
tracked here so audits can cross-reference.

- `DialogSpec::PluginPicker`, `DialogSpec::McpbConfig`, `DialogSpec::Confirm`:
  registered but `tui_runner::dispatch_slash_command` emits
  `SlashCommandStatusKind::DialogPending` instead of opening a real
  overlay. The dialog data is plumbed; the TUI consumer is not. Track
  in `coco-tui::overlays`, not here.
- `/permissions allow|deny|reset`: mutates `engine_config` for the
  session but does not write to settings.json. Match TS behavior
  (session-only `PermissionUpdateDestination::Session`). No fix needed.
