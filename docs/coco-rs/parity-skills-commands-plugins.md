# Parity Plan — Skills / Commands / Plugins (TS-mirrored)

Delta against `crate-coco-{skills,commands,plugins}.md`. Every item below has been verified against the TS source at `/lyz/codespace/3rd/claude-code/src/`. Each entry follows:

> **TS source** · **Define** (types) · **Behavior** (algorithm) · **UI** (what the user sees) · **Rust today** · **Mirror plan**

Status legend: ✅ aligned · ⚠️ partial / drift · ❌ missing.

---

## 0. Top-level seam (P1, blocks everything below)

The `CommandRegistry` constructor in TS resolves order: **bundled-skill commands → builtin-plugin skill commands → marketplace-plugin commands → on-disk skill dirs (managed → user → project → legacy `commands/`) → built-in slash commands** (`commands.ts` + `loadPluginCommands.ts:1-120`).

- **Rust today** (`commands/src/lib.rs:60`): `CommandRegistry::new() -> Default::default()`. The seam is empty — `register_builtins()` and `register_extended_builtins()` are called in isolation, with **no skill→command bridging and no plugin→command bridging**.
- **Mirror plan**: change to `CommandRegistry::build(cwd, &SkillManager, &PluginManager)`; emit registrations in the exact TS order (see §0.1). All P1 items below assume this seam exists.

### 0.1 Resolution order (verified)

```
1. bundled skills (skills/bundled/index.ts:24, registerBundledSkill)
2. builtin plugins (plugins/builtinPlugins.ts:108, getBuiltinPluginSkillCommands)
3. marketplace plugins (utils/plugins/loadPluginCommands.ts, getPluginCommands)
4. on-disk skill dirs in priority: policySettings → userSettings → projectSettings
   (skills/loadSkillsDir.ts:78, getSkillsPath)
5. legacy commands/ flat .md (loadedFrom='commands_DEPRECATED')
6. hardcoded slash commands (/help, /clear, /compact, ...)
```

Last-wins for name collisions, with one exception: bundled commands cannot be overridden by user/project (`source: 'bundled'` is sticky in `loadSkillsDir.ts`).

---

## 1. Skills

### 1.1 Bundled-skill file extraction & `skillRoot` ❌ P1

- **TS source**: `skills/bundledSkills.ts:53-220`.
- **Define**:
  ```ts
  type BundledSkillDefinition = {
    name; description; aliases?; whenToUse?; argumentHint?;
    allowedTools?; model?; disableModelInvocation?; userInvocable?;
    isEnabled?: () => boolean; hooks?; context?: 'inline'|'fork'; agent?;
    files?: Record<string, string>;          // ← lazy extraction
    getPromptForCommand: (args, ctx) => Promise<ContentBlockParam[]>;
  }
  ```
- **Behavior**:
  1. `registerBundledSkill(def)` checks `def.files`. If present, computes `skillRoot = getBundledSkillExtractDir(name)` (= `~/.claude/bundled-skills/<nonce>/<name>/`).
  2. Wraps `getPromptForCommand` with a closure that **memoizes one extraction promise per process** (concurrent callers await the same promise — no write race).
  3. On first call: `extractBundledSkillFiles()` → groups files by parent dir, `mkdir({recursive: true, mode: 0o700})` once per parent, then writes each file via `open(path, O_WRONLY|O_CREAT|O_EXCL|O_NOFOLLOW, 0o600)`. On Windows: `'wx'` flag (libuv numeric flags throw EINVAL).
  4. Path validation in `resolveSkillFilePath`: rejects `isAbsolute`, rejects components matching `..` against **both** `path.sep` and literal `/`. No unlink-on-EEXIST (`unlink()` follows symlinks too).
  5. After extraction succeeds, `prependBaseDir(blocks, dir)` injects `Base directory for this skill: <dir>\n\n` into the first text block (or as a new leading block).
  6. On extract failure → log, return `null`, prompt still works without the prefix.
- **UI**: invisible to user. The model sees `Base directory for this skill: /…/<nonce>/<name>` and uses it for Read/Bash/Grep against bundled reference files.
- **Rust today** (`skills/src/bundled.rs`): `prompt: String` is `include_str!`'d at compile time. No `files`, no `skillRoot`, no extraction, no per-process nonce.
- **Mirror plan**:
  - Add `files: Option<HashMap<String, &'static str>>` (compile-time map) on `BundledSkillSpec`.
  - At first invocation, extract to `~/.coco/bundled-skills/<process-nonce>/<name>/` using `nix`/`rustix` for `O_NOFOLLOW|O_EXCL`. Use a `tokio::sync::OnceCell<Result<PathBuf>>` per skill for the memoized promise.
  - Pre-flight path validation with the same two-pass `..` check.
  - On success, set `SkillDefinition.skill_root = Some(dir)` and have the runtime prepend `Base directory for this skill: <dir>\n\n` to the prompt at injection time (`coco-context::skill_listing`).
  - Test case: concurrent `tokio::spawn` × 10 must produce exactly one extraction and same dir.

### 1.2 Lazy `getPromptForCommand(args, ctx)` ❌ P1

- **TS source**: `skills/bundled/*.ts` (10 unconditional skills + 7 feature-gated). Each `register*Skill()` provides an async closure.
- **Behavior**: TS resolves prompts at invocation time, allowing arg substitution (`$ARGUMENTS`, `$1`…), shell expansion (`$(date)` → result string via `executeShellCommandsInPrompt`), env inspection, and conditional content per `ToolUseContext`.
- **UI**: invisible — user types `/skill arg1 arg2`, model gets fully-rendered `ContentBlockParam[]`.
- **Rust today**: `SkillDefinition.prompt: String` is static. `shell_exec.rs` runs at *load* time, not invocation time. Argument substitution exists for disk skills (`argument_substitution.rs` parity unverified) but not for bundled.
- **Mirror plan**:
  - Replace `SkillDefinition.prompt: String` for bundled skills with a function pointer `render: fn(args: &str, ctx: &SkillRenderContext) -> Vec<PromptPart>`.
  - For TOML/MD-disk skills, keep `prompt: String` but route through `expand_args(prompt, args)` + `execute_shell_in_prompt(prompt, ctx)` at invocation, not load.
  - `PromptPart` mirrors `ContentBlockParam`: `Text { text } | Image { … } | Document { … }`.

### 1.3 Bundled skill inventory drift ⚠️ P1

Verified TS unconditional registrations (`skills/bundled/index.ts:24-34`): `update-config, keybindings, verify, debug, lorem-ipsum, skillify, remember, simplify, batch, stuck` (10).

Feature-gated (`index.ts:35-78`): `dream` (KAIROS|KAIROS_DREAM), `hunter` (REVIEW_ARTIFACT), `loop` (AGENT_TRIGGERS), `schedule` (AGENT_TRIGGERS_REMOTE), `claude-api` (BUILDING_CLAUDE_APPS), `claude-in-chrome` (auto-detect), `run-skill-generator` (RUN_SKILL_GENERATOR).

Coco-rs (`skills/src/bundled.rs:47-264`): `commit, review-pr, pdf, simplify, verify, update-config, keybindings-help, remember, stuck, batch, loop, debug, skillify, lorem-ipsum, claude-api, schedule` (16).

| Skill | TS registration | Rust | Action |
|---|---|---|---|
| update-config | unconditional | ✅ | — |
| keybindings(-help) | unconditional, `userInvocable:false` | ✅ (matches) | — |
| verify | unconditional | ✅ | — |
| debug | unconditional, `disableModelInvocation:true` | ✅ (matches) | — |
| lorem-ipsum | unconditional | ✅ | — |
| skillify | unconditional | ✅ | — |
| remember | unconditional | ✅ | — |
| simplify | unconditional | ✅ | — |
| batch | unconditional, `disableModelInvocation:true` | ✅ (matches) | — |
| stuck | unconditional | ✅ | — |
| loop | gated `AGENT_TRIGGERS` | unconditional | gate via `Feature::AgentTriggers` |
| schedule | gated `AGENT_TRIGGERS_REMOTE` | unconditional | gate via `Feature::AgentTriggersRemote` |
| claude-api | gated `BUILDING_CLAUDE_APPS` | unconditional | gate via `Feature::BuildingClaudeApps` |
| claude-in-chrome | auto-detect (`shouldAutoEnableClaudeInChrome`) | ❌ | port detection helper |
| dream | gated `KAIROS\|KAIROS_DREAM` | ❌ | port (KAIROS feature flag) |
| hunter | gated `REVIEW_ARTIFACT` | ❌ | port |
| run-skill-generator | gated `RUN_SKILL_GENERATOR` | ❌ | port |
| commit | not in TS bundled | extra | **delete** — TS ships `/commit` as a top-level *command*, not a bundled skill (`commands/commit.ts`) |
| review-pr | not in TS bundled | extra | **delete** — `commands/review.ts` covers this |
| pdf | not in TS bundled | extra | **delete** — Read tool already supports PDF (`crate-coco-tools.md`) |

- **Mirror plan**: drop `commit`, `review-pr`, `pdf` from bundled; add the 4 missing gated skills with `is_enabled: Option<fn() -> bool>` + a `Feature` lookup; verify model/tool/disable-flags match TS per-skill files.

### 1.4 `isEnabled()` callback per-skill ❌ P1

- **TS**: `Command.isEnabled?: () => boolean` at `types/command.ts`. Used by every gated skill plus the loop skill (`registerLoopSkill` delegates to `isKairosCronEnabled()` per-invocation, so even if AGENT_TRIGGERS is on, the skill hides if cron is off).
- **Behavior**: command typeahead, `/help`, and Skill-tool listing all check `isEnabled()` per-keystroke. Bundled skills register unconditionally; visibility flips at runtime.
- **UI**: skills appear/disappear in the `/`-typeahead and `Skill` tool's "available skills" panel **without a session reload**.
- **Rust today** (`commands/src/lib.rs:35`): `IsEnabledFn = fn() -> bool` exists on `RegisteredCommand` but **`SkillDefinition` has no equivalent** — bundled skills are registered at startup via `register_bundled()` and stay in the registry forever.
- **Mirror plan**: add `pub is_enabled: Option<fn(&Features) -> bool>` to `SkillDefinition`. Filter at every read site: `SkillManager::visible_skills(features)`, `inject_skill_listing()`, `to_commands()`. Keep `register()` insertion stable so toggling feature flags re-shows the skill.

### 1.5 `paths` glob conditional activation ⚠️ P2

- **TS source**: `skills/loadSkillsDir.ts:159-178` — `paths` parsed via `splitPathInFrontmatter`, `**` and trailing `/**` stripped, `**`-only patterns normalized to "no paths".
- **Behavior**: a skill with `paths: ["src/**/*.ts"]` only appears in the listing when the current edit/read target matches one of the patterns. Hot-evaluated as the agent touches files (`discoverSkillDirsForPaths`).
- **Rust today** (`skills/src/lib.rs:741-800`): `expand_braces` exists; the matcher exists. **But `coco-tools` doesn't call `discover_skill_dirs_for_paths()` from Read/Write/Edit handlers.** Activation is silent.
- **Mirror plan**: add a `system-reminder` generator hook in `coco-system-reminder` that subscribes to `FileTouchedEvent` and calls `SkillManager::activate_for_paths()`; on change, re-run `inject_skill_listing()`.

### 1.6 Skill-change detector behavioral details ⚠️ P2

- **TS source**: `utils/skills/skillChangeDetector.ts:1-311`.
- **Define / Behavior** (verified constants):
  - `FILE_STABILITY_THRESHOLD_MS = 1000`, `FILE_STABILITY_POLL_INTERVAL_MS = 500`
  - `RELOAD_DEBOUNCE_MS = 300`, `POLLING_INTERVAL_MS = 2000` (Bun-only polling fallback)
  - chokidar `depth: 2`, `ignoreInitial: true`, `atomic: true`
  - Watched paths: `~/.claude/skills`, `~/.claude/commands`, `.claude/skills`, `.claude/commands`, plus every `--add-dir` entry's `.claude/skills`
  - Ignored: `.git/` segments, non-file/non-dir entries (sockets/FIFOs, EOPNOTSUPP on macOS)
  - On batch fire: run `executeConfigChangeHooks('skills', firstPath)`. If `hasBlockingResult` → log, abort reload. Else: `clearSkillCaches()` → `clearCommandsCache()` → `resetSentSkillNames()` → `skillsChanged.emit()`.
- **UI**: silent unless a hook blocks it; on success, the next `/`-typeahead reflects new skills, and the next user message picks up new system-reminder content.
- **Rust today** (`skills/src/watcher.rs`): notify-based, 300ms debounce. **Missing**: 1s stability threshold, ConfigChange hook integration, `.git/` ignore, `--add-dir` watched paths, `resetSentSkillNames()` analogue.
- **Mirror plan**:
  - Add `WatcherConfig { stability_threshold: 1s, poll_interval: 2s, debounce: 300ms }`.
  - Filter `.git/` from watch handler.
  - Subscribe to `additional_dirs` from `RuntimeConfig`.
  - Call `coco_hooks::execute_config_change_hooks("skills", path)` before clearing caches; honor blocking result.
  - Reset sent-skill tracking in attachments crate.

### 1.7 MCP-sourced skills ❌ P2

- **TS source**: `skills/mcpSkillBuilders.ts` + `services/mcp/mcpSkills.ts` (uses `getMCPSkillBuilders()` registry).
- **Define**: builder is registered at `loadSkillsDir.ts` module init via `registerMCPSkillBuilders({createSkillCommand, parseSkillFrontmatterFields})`. `mcpSkills.ts` then consumes it whenever an MCP server publishes a skill list.
- **Behavior**: cycle-break — `loadSkillsDir → mcpSkillBuilders → mcp client` would otherwise cycle, so the registry is a write-once leaf module.
- **Rust today**: `SkillSource::Mcp { server_name }` enum variant exists; **no builder registration, no consumer**.
- **Mirror plan**: add `coco-skills::mcp_builders::register(...)` (one-time `OnceLock`); wire `coco-mcp::client::on_skill_list` to call `SkillManager::register_mcp_skill(server_name, frontmatter, body)`.

---

## 2. Slash Commands

### 2.1 `/rewind` ❌ P1

- **TS source**: `commands/rewind/rewind.ts:1-13` (call) + `Tool.ts` (`openMessageSelector` callback) + `utils/fileHistory.ts` (~1110 LOC).
- **Define**:
  ```ts
  call: async (_args, context) => {
    if (context.openMessageSelector) context.openMessageSelector();
    return { type: 'skip' };
  }
  ```
- **Behavior** (full chain):
  1. Command body just opens an overlay; everything else is in the TUI message-selector + fileHistory.
  2. `openMessageSelector` shows `selectableUserMessagesFilter()` results — user messages only, walks backward from latest.
  3. **Compact-boundary respected**: cannot rewind past a `compact-summary` message.
  4. On selection: `fileHistoryMakeSnapshot()` snapshots current files, then truncates messages array, then **replays inverted edit log** to restore file state at that point.
  5. `removeLastFromHistory()` so a subsequent ESC (auto-restore-on-interrupt) doesn't double-undo.
- **UI**:
  - Overlay (~ratatui equivalent) titled "Rewind to a previous turn".
  - Up/Down to navigate, ENTER selects, ESC cancels.
  - Each row: timestamp + first-line preview of the user message (240-char trunc).
  - Compact boundaries shown as a horizontal dim rule line; entries above are non-selectable.
  - On confirm: a system-line "Rewound to <preview>. Restored N file(s)." then control returns to prompt with the conversation truncated.
- **Rust today** (`commands/src/handlers/`): no rewind handler.
- **Mirror plan** (mirroring TS exactly):
  - **types**: `coco-types::CoreEvent::Tui::OpenMessageSelector` (new variant); `coco-tui::overlays::message_selector` widget.
  - **fileHistory port** (new crate `coco-context::file_history`): content-addressed store at `~/.coco/file-history/<sha>/`. On every Edit/Write tool result, append `{message_uuid, path, before_sha, after_sha}` to in-memory ordered Vec (NOT HashMap — order matters for replay).
  - **handler**: `RewindHandler::execute(args, ctx)` emits `CoreEvent::Tui::OpenMessageSelector` and returns `Skip`.
  - **selector widget**: ratatui list, filter callback, compact-boundary rendering as a `ratatui::widgets::Block::default().borders(Borders::TOP)` with dim title "compact boundary".
  - **truncate + restore**: `MessageHistory::truncate_after(uuid)` then `FileHistory::restore_to(uuid)` (replay before→after deltas in reverse).

### 2.2 `/compact` ❌ P1

- **TS source**: `commands/compact/compact.ts:1-287`.
- **Define**: `LocalCommandCall(args, context) -> CompactionResult | error`.
- **Behavior** (verified flow):
  1. `messages = getMessagesAfterCompactBoundary(context.messages)` — REPL keeps snipped messages for scrollback; project so the model doesn't summarize stripped content.
  2. If empty: `throw new Error('No messages to compact')`.
  3. `customInstructions = args.trim()`.
  4. **If no customInstructions**: try `trySessionMemoryCompaction(messages, agentId)`. If it returns a result:
     - Clear `getUserContext.cache`, run `runPostCompactCleanup()`.
     - If `PROMPT_CACHE_BREAK_DETECTION`: `notifyCompaction(querySource, agentId)`.
     - `markPostCompaction()` and `suppressCompactWarning()`.
     - Return `{type:'compact', compactionResult, displayText}`.
  5. **Else if reactive-only mode** (`reactiveCompact?.isReactiveOnlyMode()`): route through `compactViaReactive` — runs `executePreCompactHooks` and `getCacheSharingParams` in `Promise.all`, merges hook instructions, calls `reactiveCompactOnPromptTooLong`, returns the same `{type:'compact'}` shape.
  6. **Else (legacy)**: `microcompactMessages` first → `compactConversation(messagesForCompact, ctx, cacheParams, false, customInstructions, false)` → `setLastSummarizedMessageId(undefined)` → `suppressCompactWarning` → cleanup.
  7. Error mapping: `aborted` → "Compaction canceled."; `NOT_ENOUGH_MESSAGES` re-throw verbatim; `INCOMPLETE_RESPONSE` re-throw verbatim; else `Error during compaction: ${error}`.
- **UI** (verified `buildDisplayText`):
  ```
  Compacted (${shortcut} to see full summary)
  ${userDisplayMessage}            // optional from compact result
  ${upgradeMessage}                // optional from getUpgradeMessage('tip')
  ```
  Rendered via `chalk.dim(...)`. The `(ctrl+o to see full summary)` line is **hidden when `verbose=true`**. On reactive: emits `setSDKStatus('compacting')` and progress events `{type:'hooks_start'|'compact_start'|'compact_end'}`.
- **Rust today** (`commands/src/handlers/compact.rs`, ~72 LOC): prints "Compacting…" string. No wiring to `coco-compact`.
- **Mirror plan**:
  - Wire to `coco-compact::{compact_conversation, microcompact_messages, try_session_memory_compaction, run_post_compact_cleanup, mark_post_compaction, suppress_compact_warning}`. These all exist in plan; verify implementation in `compact` crate.
  - Reactive path gated by `Feature::ReactiveCompact` (already in `Feature` enum per `feature-gates-and-tool-filtering.md`).
  - **CoreEvent emissions** (UI parity): `Tui::CompactStart`, `Tui::HooksStart {hook_type: PreCompact}`, `Tui::CompactEnd`, `Tui::SetStatus("compacting")`.
  - **Return shape**: `CommandResult::Compact(CompactionResult)` (new variant) — currently Rust has only `Text`/`InjectPrompt`/`Skip`.
  - **Display string**: build via `coco-tui::format::dim_lines` with the `(<shortcut> to see full summary)` line conditional on `!verbose`.

### 2.3 `/init` ❌ P1

- **TS source**: `commands/init.ts:1-256`. Type: `'prompt'`. Two prompts gated by `feature('NEW_INIT')` AND `(USER_TYPE='ant' || CLAUDE_CODE_NEW_INIT truthy)`.
- **Behavior**:
  1. `maybeMarkProjectOnboardingComplete()` flips a state flag (one-shot suppression of onboarding banner).
  2. Returns `[{type:'text', text: NEW_INIT_PROMPT | OLD_INIT_PROMPT}]`.
  3. The agent then runs an 8-phase guided flow (Phase 1 ask via `AskUserQuestion`, Phase 2 codebase survey via subagent, Phase 3 fill gaps with `preview` markdown panel, Phases 4-7 write CLAUDE.md / CLAUDE.local.md / `.claude/skills/` / hooks / Phase 8 summary).
- **UI**: this command's UI is ENTIRELY agent-driven via `AskUserQuestion` overlays. The `progressMessage: 'analyzing your codebase'` shows in the streaming status line.
- **Rust today** (`commands/src/handlers/`): partial `init_handler_async` checks file existence only.
- **Mirror plan**:
  - Mark as `CommandType::Prompt` (not `Local`). Builder returns the verbatim NEW_INIT_PROMPT or OLD_INIT_PROMPT string based on `Feature::NewInit` AND (`UserType::Ant` || env `COCO_NEW_INIT`).
  - Inline the **full 256-line prompt** in `commands/src/prompts/init_new.txt` and `init_old.txt` via `include_str!`. Do NOT paraphrase — the prompt is the contract.
  - Hook `maybe_mark_project_onboarding_complete()` into `coco-state::project_onboarding`.
  - Verify `Feature::NewInit` exists; if not add to `coco-types::Feature`.
  - `progressMessage: "analyzing your codebase"` → `CommandBase.progress_message`.

### 2.4 `/memory` ❌ P1

- **TS source**: `commands/memory/memory.tsx:1-89` — type: `'local-jsx'`.
- **Define**: `LocalJSXCommandCall = async (onDone) => ReactNode`.
- **Behavior**:
  1. Pre-flight: `clearMemoryFileCaches() + await getMemoryFiles()` (avoids Suspense flash).
  2. Render `<MemoryFileSelector onSelect onCancel>` inside a `Dialog title="Memory" color="remember"`.
  3. On select: `mkdir($CLAUDE_HOME)`, `writeFile(path, '', {flag:'wx'})` (catches EEXIST). Open in `$VISUAL || $EDITOR`. Report `Opened memory file at <relpath>` + an editor-source hint (`Using $VISUAL="vim". To change editor, set $EDITOR or $VISUAL.`).
  4. On cancel: `'Cancelled memory editing'` with `display:'system'`.
- **UI**:
  - Dialog with `color: 'remember'` (palette key).
  - File selector lists: enterprise/managed → user-global → project → CLAUDE.local.md → subdir CLAUDE.md files.
  - Bottom margin: dim text "Learn more: https://code.claude.com/docs/en/memory".
- **Rust today** (`commands/src/handlers/memory.rs`, ~170 LOC): list-only.
- **Mirror plan**:
  - Add `CommandResult::OpenDialog(DialogSpec)` variant; `DialogSpec::MemoryFileSelector { entries: Vec<MemoryFileEntry> }`.
  - TUI overlay `coco-tui::overlays::memory_dialog` mirrors the TS Ink dialog (title "Memory", color = palette `remember`).
  - File ops: tokio::fs `create_dir_all`, then `OpenOptions::new().write(true).create_new(true).open(...)` (≡ `flag:'wx'` — error::AlreadyExists is the EEXIST analogue, swallow it).
  - Editor open via `coco-utils::prompt_editor::open_in_editor` (port `utils/promptEditor.ts`).
  - System message format string MUST match TS verbatim — UI testing keys on it.

### 2.5 `/security-review`, `/insights`, `/brief`, `/advisor` ❌ P1

All four are top-level `Prompt` commands (not directory commands). Verified files: `commands/security-review.ts:243`, `insights.ts:3200` (large, generates an analytics dashboard prompt), `brief.ts:130`, `advisor.ts:109`.

- **Define**: `{ type: 'prompt', name, description, getPromptForCommand: async (args, ctx) => ContentBlockParam[] }`.
- **Behavior**: the command body returns a static prompt; the model does the work via tool calls in subsequent turns.
- **UI**: progress message in the status line; everything else is normal stream output.
- **Rust today**: declared in `implementations.rs` as stubs; **`Prompt`-type execution path is not wired into `coco-query`**. Even if the handler returns the right text, nothing runs the model loop on it.
- **Mirror plan**:
  - Add `CommandResult::Prompt(Vec<PromptPart>)`. `coco-query::execute_command` routes this back into the agent loop as a synthesized user message (TS `processSlashCommand` does this).
  - Port the four prompt bodies verbatim from TS to `commands/src/prompts/*.txt`. Wire each into `register_extended_builtins`.

### 2.6 `/commit-push-pr` ❌ P2

- **TS source**: `commands/commit-push-pr.ts:158`. Single command that orchestrates `git add → git commit → git push → gh pr create` via a guided agent prompt.
- **Behavior**: prompt asks the agent to (1) inspect diff, (2) draft message + PR body, (3) execute the chain with confirmations between steps.
- **UI**: same as any prompt command — visible in `/`-typeahead under "git workflow" group.
- **Rust today**: `/commit` and `/pr` exist separately; no orchestrator.
- **Mirror plan**: port verbatim prompt; register as `Prompt` command.

### 2.7 `createMovedToPluginCommand` migration helper ❌ P2

- **TS source**: `commands/createMovedToPluginCommand.ts:22-65`.
- **Define**:
  ```ts
  function createMovedToPluginCommand({name, description, progressMessage,
    pluginName, pluginCommand, getPromptWhileMarketplaceIsPrivate}): Command
  ```
- **Behavior**:
  - If `process.env.USER_TYPE === 'ant'`: return a **fixed prompt** that instructs the model to tell the user how to install the plugin (`claude plugin install <pluginName>@claude-code-marketplace`) and use `/<pluginName>:<pluginCommand>` afterwards.
  - Else: fall back to `getPromptWhileMarketplaceIsPrivate(args, context)` (the original prompt body, until the marketplace is public).
- **UI**: a system-line in the chat tells the user to install + how to invoke post-install.
- **Rust today**: not used anywhere.
- **Mirror plan**:
  - Port helper to `commands/src/migration.rs::create_moved_to_plugin_command`.
  - Use it for any command that's currently bundled but planned to move to a plugin (e.g. once `/insights` migrates to a plugin).
  - Gate via `UserType::Ant` (already in `coco-types`).

### 2.8 Stub commands (P3 batch)

These need real handlers but UI is straightforward; one-line table for tracking.

| Command | TS file | Behavior summary | UI |
|---|---|---|---|
| `/theme` | `commands/theme/index.ts` | Cycle theme; persist to settings | overlay color picker |
| `/color` | `commands/color/` | Show palette grid | inline | 
| `/output-style` | `commands/output-style/` | Set response style preset | overlay select |
| `/sandbox-toggle` | `commands/sandbox-toggle/` | Toggle sandbox + persist | confirm dialog |
| `/vim` | `commands/vim/` | Enter vim mode | status line |
| `/keybindings` | `commands/keybindings/` | Edit keybindings.json | external editor |
| `/privacy-settings` | `commands/privacy-settings/` | Toggle telemetry | overlay |
| `/branch` | `commands/branch/` | Fork session into a new branch | confirm |
| `/tag` `/share` `/export` `/rename` | `commands/*/` | Session metadata | dialogs |
| `/env` `/heap-dump` `/ant-trace` `/debug-tool-call` `/ctx_viz` `/perf-issue` `/bughunter` | `commands/*/` | Diagnostics | system messages |

For each: port the TS prompt or local action verbatim; reuse Rust handler scaffolding.

---

## 3. Plugins

### 3.1 Three-layer refresh ❌ P1

- **TS source**: `utils/plugins/refresh.ts:1-216` (Layer 3) + `utils/plugins/reconciler.ts:1-265` (Layer 2). `installedPluginsManager.ts:1268` writes Layer-1 intent.
- **Define**:
  ```ts
  type RefreshActivePluginsResult = {
    enabled_count, disabled_count, command_count, agent_count,
    hook_count, mcp_count, lsp_count, error_count,
    agentDefinitions, pluginCommands  // local refs for callers
  }
  ```
- **Behavior — Layer 2 (`reconcileMarketplaces`)**:
  1. `declared = getDeclaredMarketplaces()` (from settings).
  2. `materialized = await loadKnownMarketplacesConfig()` (try/catch → `{}`).
  3. `diff = diffMarketplaces(declared, materialized, {projectRoot})`. Three buckets: `missing`, `sourceChanged`, `upToDate`. **Fallback marker**: if `intent.sourceIsFallback`, presence suffices — never compare sources, never re-clone (would stomp seed/prior-install/mirror).
  4. Skip via opts.skip (zip-cache mode unsupported types) and skip `update`-action local-path entries whose path doesn't exist.
  5. For each remaining: `await addMarketplaceSource(source)`. Source-idempotent — same source returns `alreadyMaterialized:true`.
  6. Emits `onProgress` events: `installing | installed | failed`.
- **Behavior — Layer 3 (`refreshActivePlugins`)**:
  1. `clearAllCaches()` then `clearPluginCacheExclusions()`.
  2. **Sequence not race**: `await loadAllPlugins()` THEN `Promise.all([getPluginCommands, getAgentDefinitionsWithOverrides])`. (Pre-fix #23693, racing them caused plugin-cache-miss.)
  3. Populate `mcpServers` and `lspServers` lazily on each enabled plugin in parallel; aggregate counts.
  4. `setAppState({plugins: {...}, agentDefinitions, mcp.pluginReconnectKey++})`.
  5. `reinitializeLspServerManager()` unconditionally (even if no plugins ship LSP — clears stale config).
  6. `await loadPluginHooks()` in try/catch (failure goes to `error_count`, doesn't lose plugin/command/agent data).
  7. Compute `hook_count` from `enabled[].hooksConfig` event-matchers.
  8. Return result with local `agentDefinitions` and `pluginCommands` refs (for callers maintaining outside AppState, e.g. `print.ts`).
- **UI**:
  - Interactive: `useManagePlugins` sets `needsRefresh` notification ("Plugins changed. Run /reload-plugins to apply."). Layer-3 refresh runs only on explicit `/reload-plugins` (PR 5b/5c — never on auto-effect to avoid thrashing).
  - Headless: `print.ts → refreshPluginState()` runs Layer-3 once before first query under `SYNC_PLUGIN_INSTALL`.
  - Background: `performBackgroundPluginInstallations()` after a new marketplace install (Layer-2 result triggers Layer-3).
- **Rust today** (`plugins/src/lib.rs`, `loader.rs`): only stubs.
- **Mirror plan** (verified call sites):
  - **Layer 2** in `coco-plugins::reconciler::reconcile_marketplaces(opts) -> ReconcileResult`. Mirror the diff buckets + fallback flag exactly. Source-idempotency via marketplace name + content hash.
  - **Layer 3** in `coco-plugins::refresh::refresh_active_plugins(set_app_state) -> RefreshActivePluginsResult`. Sequence as TS — `load_all_plugins().await` THEN parallel command/agent loads.
  - Bump `AppState.mcp.plugin_reconnect_key`; downstream MCP connection manager picks up new servers on next tick.
  - `coco-lsp::reinitialize_server_manager()` called unconditionally.
  - Hook-load failure isolated: `try { load_plugin_hooks().await } catch { error_count++ }`.
  - **Notification** in TUI: `CoreEvent::Tui::PluginsNeedRefresh { count }`. `useManagePlugins` analogue lives in `coco-state::plugins::needs_refresh: AtomicBool`.

### 3.2 Dependency resolver ❌ P1

- **TS source**: `utils/plugins/dependencyResolver.ts:1-305` (verified in full above).
- **Define**:
  ```ts
  type ResolutionResult =
    | {ok:true, closure: PluginId[]}
    | {ok:false, reason:'cycle', chain: PluginId[]}
    | {ok:false, reason:'not-found', missing, requiredBy}
    | {ok:false, reason:'cross-marketplace', dependency, requiredBy}
  ```
- **Behavior — `resolveDependencyClosure`**:
  1. `INLINE_MARKETPLACE = 'inline'` sentinel for `--plugin-dir` plugins.
  2. `qualifyDependency(dep, declaringId)`: bare names inherit declarer's marketplace UNLESS declarer is `@inline` (sentinel can't qualify).
  3. DFS walk from rootId. **Skip already-enabled deps** (avoids surprise settings writes), **but never skip root** (re-install must re-cache even if settings claim it's enabled but disk is empty).
  4. **Cross-marketplace**: blocked by default unless ROOT marketplace's `allowCrossMarketplaceDependenciesOn` includes target. **No transitive trust** — A allowing B does not mean B's deps inherit the trust.
  5. Cycle detection via `stack.includes(id)`.
- **Behavior — `verifyAndDemote`**:
  - Fixed-point iteration. Demoting A may break B; loop until no change.
  - Two reasons: `'not-enabled'` (in plugin set but disabled) vs `'not-found'` (entirely absent).
  - **Bare deps from `@inline` plugins** match by name only, against a `enabledByName` multiset (so demoting one of two same-named plugins doesn't make the name disappear from the index).
  - Does NOT mutate input. Returns demoted Set + errors for `/doctor`.
- **Behavior — `findReverseDependents`**:
  - For uninstall/disable warnings.
  - Bare deps match by name only, against `pluginId`'s name component.
- **UI**:
  - `formatDependencyCountSuffix([dep1,dep2])` → `" (+ 2 dependencies)"` (singular/plural).
  - `formatReverseDependentsSuffix(['A','B'])` → `" — warning: required by A, B"`.
  - `'cross-marketplace'` errors shown in `/plugin install` flow with "—why blocked + how to override (install dep yourself first)".
- **Rust today**: signatures only.
- **Mirror plan**: port verbatim — pure functions, no I/O, deterministic test surface. Use `coco-types::PluginId` (= `name@marketplace` newtype).

### 3.3 MCPB (`.mcpb`/`.dxt`) bundles ❌ P1

- **TS source**: `utils/plugins/mcpbHandler.ts:968` + `zipCache.ts:406` + `zipCacheAdapters.ts:164` (= 1538 LOC total).
- **Define / Behavior**:
  1. ZIP container; extensions `.mcpb` (Anthropic format) or `.dxt` (legacy DX-ext).
  2. Contains `manifest.json` + extracted server binaries + optional `configSchema`.
  3. Load pipeline: download → extract to cache → parse manifest → validate `configSchema` → generate MCP server config.
  4. **Cache**: content-addressed (SHA-256 of archive bytes). Path: `~/.coco/plugins/mcpb-cache/<sha>/`. Metadata sidecar tracks source URL + extracted path + timestamps.
  5. `McpbLoadStatus::NeedsConfig{schema, existing, errors}` returned when configSchema requires user input.
- **UI**:
  - First-time install: `/plugin install <mcpb-source>` shows a "Configure MCPB" overlay with form fields per `configSchema` property.
  - Validation errors shown inline.
  - Successful install: shows server name + count of tools exposed.
- **Rust today**: zero.
- **Mirror plan**:
  - New module `coco-plugins::mcpb` (~300 LOC):
    - `parse_mcpb_archive(bytes) -> (Manifest, Vec<File>)` via `zip` crate.
    - `validate_config_schema(schema, user_config) -> Result<ResolvedConfig, Vec<Error>>` — JSONSchema subset matching TS shape.
    - `extract_to_cache(sha, files) -> PathBuf` — content-addressed.
    - `cache_metadata.json` sidecar with `{source_url, sha, extracted_at, last_used}`.
  - TUI overlay: `coco-tui::overlays::mcpb_config` — form widget driven by JSONSchema properties.

### 3.4 Validation & security ❌ P1

- **TS source**: `utils/plugins/validatePlugin.ts:903`.
- **Behavior — three pillars**:
  1. **Path traversal**: reject `..` segments (path.sep AND literal `/`), absolute paths, and symlinks escaping plugin root (resolve+startsWith check).
  2. **Official-name impersonation**:
     - Regex `^claude-(plugins?-)?official(-|$)` and similar — block third-party plugins matching.
     - Non-ASCII homograph detection: NFKD-normalize, reject if normalized name matches an official pattern (catches Cyrillic 'а' / Greek 'ο' tricks).
  3. **Enterprise policy** (`pluginPolicy.ts:20`):
     - `strict_known_marketplaces: bool` — only allow plugins from approved marketplaces.
     - `blocked_marketplaces: string[]` — explicit blocklist.
     - `strict_plugin_only_customization: bool` — users can't install plugins outside `Managed` scope.
- **UI**: install flow rejects with clear reason ("This plugin name impersonates an official plugin." / "Marketplace 'X' is blocked by policy."). `/doctor` shows policy state.
- **Rust today**: warn-only manifest validation.
- **Mirror plan**:
  - `coco-plugins::security::{validate_paths, check_impersonation, is_blocked_by_policy}`. Use `unicode-normalization` crate for NFKD.
  - `EnterprisePluginPolicy` already in `crate-coco-plugins.md`; wire into `PluginManager::load()`.

### 3.5 Builtin plugin registry ❌ P1

- **TS source**: `plugins/builtinPlugins.ts:1-159`.
- **Define**:
  ```ts
  // Plugin ID format: `{name}@builtin`
  type BuiltinPluginDefinition = {
    name; description; version?;
    skills?: BundledSkillDefinition[];
    hooks?: HooksSettings;
    mcpServers?: McpServerConfig;
    defaultEnabled?: bool;
    isAvailable?: () => bool;  // hide when unavailable
  }
  ```
- **Behavior**:
  - Registry: `BUILTIN_PLUGINS: Map<name, BuiltinPluginDefinition>`.
  - `getBuiltinPlugins()` reads `settings.enabledPlugins[<name>@builtin]` and resolves to enabled vs disabled lists. Effective state: user setting > `defaultEnabled` > `true`.
  - `isAvailable() === false` → omit entirely (not even shown as disabled in the UI).
  - Skills from enabled builtins are merged into the command registry via `getBuiltinPluginSkillCommands()` with `source:'bundled'` (so analytics + truncation behavior matches bundled skills).
- **UI**:
  - `/plugin` overlay shows a "Built-in" section with each builtin plugin, toggleable.
  - The plugin card lists contributed skills/hooks/MCP servers.
  - Toggling persists to user settings (`settings.json:enabledPlugins`).
- **Rust today**: marketplace constants exist (`OFFICIAL_MARKETPLACE_NAME`); **no builtin plugin registry**.
- **Mirror plan**:
  - `coco-plugins::builtins::{register_builtin_plugin, get_builtin_plugins, get_builtin_plugin_skill_commands, is_builtin_plugin_id}`.
  - Plugin ID format `{name}@builtin` matched via `BUILTIN_MARKETPLACE_NAME = "builtin"`.
  - Wire into Layer-3 refresh (§3.1) so builtins always appear before marketplace plugins.
  - TUI section in plugin overlay (`coco-tui::overlays::plugin_picker`).

### 3.6 Headless / CCR mode ❌ P2

- **TS source**: `utils/plugins/headlessPluginInstall.ts:174` + `pluginAutoupdate.ts:284`.
- **Behavior**:
  - No interactive prompts (auto-approve managed/policy plugins; never prompt for user-scope).
  - **Zip cache**: pre-archived `.zip` in shared dir; `reconcileMarketplaces({skip})` skips unsupported source types and falls back to zip cache before network.
  - Stricter timeouts: 30s (vs 120s interactive).
- **UI**: no overlays. Progress emitted as plain log lines on stderr (`[reconcile] N marketplaces: ...`).
- **Rust today**: synchronous loader, no headless variant.
- **Mirror plan**: `PluginManager::install_headless(settings, cache_dir, timeout)`. Same code path as interactive, with `opts.skip` and `opts.auto_approve = true`.

### 3.7 Hot reload ❌ P2

- **TS source**: `utils/plugins/loadPluginHooks.ts:287` + the `/reload-plugins` command.
- **Behavior**: file watcher on `~/.coco/plugins/*/PLUGIN.toml` (manifest changes) and `installed_plugins.json`. On change → emit `plugins.needsRefresh = true`. **Does NOT auto-reload** — user must run `/reload-plugins` (PR 5b/5c rationale: avoid mid-turn surprise).
- **UI**: a notification line "Plugins changed on disk. Run /reload-plugins to apply." appears above the prompt.
- **Rust today**: `hot_reload.rs` (73 LOC) is atomic-flag scaffolding only.
- **Mirror plan**: notify-based watcher; on debounced change → set `AppState.plugins.needs_refresh`. Add `/reload-plugins` command that calls `refresh_active_plugins()` (§3.1).

### 3.8 Other plugin gaps ⚠️ P2/P3

| Item | TS | Rust | Action |
|---|---|---|---|
| `installed_plugins.json` V1→V2 migration | `installedPluginsManager.ts:1268` | none | port migration code (~80 LOC); read V1 list, infer scope, write V2 |
| Versioned cache paths `<name>/<version>/` | `pluginVersioning.ts:157` | flat | add per-source version calc (git→short-SHA, npm→pkg-version, local→content-hash) |
| Official marketplace auto-install | `officialMarketplaceStartupCheck.ts:439` | constants only | startup hook in `coco-cli` bootstrap; subscribe `anthropics/claude-plugins-official` on first run unless policy blocks |
| Contribution conflict warnings | `loadPluginCommands.ts:946` (dedup with log) | silent override | track seen names in each bridge; emit `CoreEvent::Tui::Warning` on collision |
| Marketplace search / hint recommendation | `marketplaceManager.ts:2643`, `hintRecommendation.ts:164` | none | P3 |
| Error taxonomy (20+ variants) | `types/plugin.ts` | one struct | refactor to enum (`PluginError::{GitAuthFailed, ManifestParseError, …}`) |

---

## 4. Sequencing (verified, mirrors TS load order)

**Round A — unblocks user-visible flows** (1 sprint):
1. Implement seam §0 (CommandRegistry takes SkillManager + PluginManager).
2. Skill `is_enabled` (§1.4), file extraction (§1.1), bundled inventory cleanup (§1.3), lazy prompts (§1.2).
3. Commands: `/compact` (§2.2), `/rewind` (§2.1), `/init` (§2.3), `/memory` (§2.4), prompt-type seam for security-review/insights/brief/advisor (§2.5).
4. Plugins: dependency resolver (§3.2) + Layer-2/3 refresh (§3.1) + builtin registry (§3.5).

**Round B — security + correctness** (1 sprint):
5. Plugin validation (§3.4) — path traversal, impersonation, policy.
6. MCPB (§3.3).
7. Skill watcher full parity (§1.6).
8. Skill paths-based activation (§1.5).

**Round C — parity tail** (ongoing):
9. Headless install (§3.6), hot reload (§3.7), V1→V2 migration, versioned cache.
10. Stub command fill-in (§2.8) and `createMovedToPluginCommand` (§2.7).
11. MCP-sourced skills (§1.7).
12. Marketplace search / hint recommendation, error taxonomy.

---

## 5. Cross-cutting deltas (TS-mirroring)

- **`Command.source` field**: TS distinguishes `'bundled' | 'builtin' | 'plugin' | 'managed' | 'mcp' | 'projectSettings' | 'userSettings' | 'commands_DEPRECATED'`. Rust `CommandSource` should match exactly — currently lossy. Add the missing variants to `coco-types::CommandSource`.
- **`CommandResult` variants**: Rust has `Text | InjectPrompt | Skip`. Add `Compact(CompactionResult)`, `Prompt(Vec<PromptPart>)`, `OpenDialog(DialogSpec)` to mirror TS `'compact' | 'prompt' | 'local-jsx'`.
- **Manifest format**: TS uses `plugin.json` (Zod-validated). Rust accepts both `plugin.json` and `PLUGIN.toml`. Decision: keep TOML for hand-authored Rust-native plugins, but make `plugin.json` the parity-preferred format and **strict-validate** unknown fields (currently warn-only).
- **Frontmatter parser**: TS's `parseSkillFrontmatterFields` (`loadSkillsDir.ts:185-265`) handles `'inherit'` model, `EFFORT_LEVELS` parser, and `parseShellFrontmatter`. Confirm Rust `frontmatter` util matches all three.
- **Telemetry events**: TS emits `tengu_skill_file_changed`, `plugin_install_started`, `plugin_install_failed`, etc. via `logEvent`. Rust should emit equivalents through `coco-otel` so dashboards line up.

---

## 6. Verification checklist (per merge)

For every PR landing pieces of this plan:

- [ ] TS source citation in commit message (`file.ts:Line` or `file.ts:func`).
- [ ] Rust types map 1:1 to TS types — no field drops, no behavior shortcuts.
- [ ] UI strings match TS verbatim (test via insta snapshots in `coco-tui`).
- [ ] Feature gate present in `coco-types::Feature` if TS gates the behavior.
- [ ] Telemetry event matches TS `tengu_*` / `plugin_*` event name.
- [ ] Test: covers (a) happy path, (b) error path, (c) concurrent invocation if applicable.
