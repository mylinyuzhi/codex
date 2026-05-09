# coco-keybindings

Keyboard shortcut resolution. TS port of `keybindings/`. Closed enums for
contexts (20: 18 user-rebindable + 2 internal) and actions (~105 variants (86 schema + 18 internal + Command escape hatch)
mirroring TS `KEYBINDING_ACTIONS`), chord support (`ctrl+x ctrl+k`,
whitespace-separated), JSON config wrapper, validator with severity,
hot-reloading user-config loader, platform-aware display formatting,
crossterm `KeyEvent` adapter.

## TS Source
- `keybindings/schema.ts` — context + action enums (authoritative names)
- `keybindings/defaultBindings.ts` — platform-conditional defaults → [`defaults`]
- `keybindings/loadUserBindings.ts` — `~/.claude/keybindings.json` merge → [`loader`] (feature-gated)
- `keybindings/parser.ts`, `shortcutFormat.ts`, `template.ts` — chord parsing → [`parser`]; display → [`display`]; template → [`template`]
- `keybindings/match.ts`, `resolver.ts` — runtime resolution → [`resolver`] (chord state machine + 1s timeout); crossterm bridge → [`adapter`] (feature-gated)
- `keybindings/reservedShortcuts.ts` — Ctrl+C/Ctrl+D + macOS-reserved → [`reserved`]
- `keybindings/validate.ts` — user-override validation → [`validator`] (5 issue kinds × 2 severities)
- `keybindings/KeybindingContext.tsx`, `KeybindingProviderSetup.tsx`,
  `useKeybinding.ts`, `useShortcutDisplay.ts` — React glue (intentionally
  not ported; replaced by the TUI's TEA dispatch + `keybinding_bridge.rs`)

Paths relative to `/lyz/codespace/3rd/claude-code/src/`.

## Key Types

| Type | Purpose |
|---|---|
| `KeybindingAction` | Closed enum, ~105 variants (86 schema + 18 internal + Command escape hatch) in `namespace:camelCase`. `Command(String)` escape hatch for user `command:foo`. Custom serde via `try_from = "String", into = "String"`. |
| `KeybindingContext` | Closed enum, 20 variants. `ALL_USER` → 18 user-rebindable; the validator rejects user bindings into `Scroll` / `MessageActions`. |
| `Keybinding` | Parsed binding: `(KeyChord, Option<KeybindingAction>, KeybindingContext)`. `action: None` is a TS-style null unbind. |
| `KeybindingBlock` | One block from JSON: `(KeybindingContext, BTreeMap<String, Option<KeybindingAction>>)`. |
| `KeybindingsConfig` | Top-level JSON shape: `{ $schema, $docs, bindings: Vec<KeybindingBlock> }`. `from_json`, `to_json_pretty`, `parse_bindings`. |
| `KeyChord`, `KeyCombo`, `parse_chord`, `parse_combo`, `ParseError` | Chord parser. Whitespace separates combo steps; `" "` is the space key. |
| `ChordResolver`, `ResolveOutcome` | Chord state machine. Outcomes: `NoMatch`, `Fire(action)`, `Pending`, `Unbound` (null-bound), `ChordCancelled`. 1s timeout via `tick(now)`. Esc cancels pending. |
| `ValidationIssue`, `Severity`, `ValidationKind`, `validate`, `format_issue` | Typed warnings mirroring TS `KeybindingWarning`. Five kinds × two severities. |
| `ReservedShortcut`, `NON_REBINDABLE`, `TERMINAL_RESERVED`, `MACOS_RESERVED`, `get_reserved_shortcuts`, `lookup_reserved`, `normalize_key_for_comparison` | Reserved-shortcut detection. |
| `DisplayPlatform`, `keystroke_to_string`, `keystroke_to_display_string`, `chord_to_string`, `chord_to_display_string` | Canonical + platform-aware display rendering for status bar / help. |
| `default_blocks`, `default_config` | Full TS-mirrored default bindings table. |
| `generate_template` | User-config template derived from defaults, NON_REBINDABLE filtered. |
| **(feature: `crossterm`)** `from_crossterm` | `KeyEvent → KeyCombo` adapter, including the escape+meta quirk fix. |
| **(feature: `loader`)** `load_keybindings`, `KeybindingsLoadResult`, `KeybindingsWatcher`, `default_keybindings_path` | Async loader with hot-reload via `coco-file-watch`. |

## Modules

- `action` — KeybindingAction enum + `FromStr` / `Display` / serde
- `context` — KeybindingContext enum + `ALL` / `ALL_USER` / `description()`
- `parser` — chord tokenization, combo parsing, `ParseError` (thiserror)
- `resolver` — chord state machine, last-wins ordering, prefix-preference, 1s timeout, Esc cancel
- `validator` — config validation: parse errors, duplicates, internal-context-in-user-config, command-outside-chat, voice-on-bare-letter, reserved shortcuts
- `reserved` — non-rebindable + terminal + macOS reserved shortcut tables; canonical-form normalization
- `display` — canonical/platform-aware keystroke + chord rendering
- `defaults` — TS-mirrored default bindings table with platform conditionals
- `template` — user-config template generator (filters NON_REBINDABLE)
- `adapter` (feature: `crossterm`) — crossterm `KeyEvent` → `KeyCombo`
- `loader` (feature: `loader`) — async user-config loader + file watcher

## Cargo features

- `crossterm` — enables `adapter` module + `crossterm` dep. Required by TUI consumers.
- `loader` — enables `loader` module + `tokio` / `tracing` / `coco-file-watch` / `dirs` deps.

Default features: none. Library callers without a TUI/runtime stay lean.

## Deliberately Not Ported

| TS file | Why |
|---|---|
| `useKeybinding.ts`, `useShortcutDisplay.ts` | React hooks; TEA architecture replaces with direct dispatch in TUI. |
| `KeybindingContext.tsx`, `KeybindingProviderSetup.tsx` | React context provider + chord interceptor; equivalent state lives in `app/tui/src/keybinding_resolver.rs` (chord state) + `keybinding_dispatch.rs` (action → TuiCommand). |
| `loadUserBindings.ts:isKeybindingCustomizationEnabled` (USER_TYPE === 'ant' / GrowthBook gate) | Per project rule: no ant-only gates in coco-rs (`feedback_no_ant_gates` memory). User customization is always available. |
| Feature-gated TS default-bindings blocks (`KAIROS`, `QUICK_SEARCH`, `TERMINAL_PANEL`, `MESSAGE_ACTIONS`, `VOICE_MODE`) | Skipped pending the underlying capability. Re-add behind a Cargo feature when relevant. |

## TUI integration

The TUI consumes `coco-keybindings` via three modules in `app/tui/src/`:

- `keybinding_resolver.rs` — defines the `KeybindingHandle` (cheap-clone
  `Arc<RwLock<HandleInner>>`) wrapping `ChordResolver` + warnings + display
  platform. Lives in `AppState.ui.kb_handle`; tests get their own per
  `AppState::new()`. No process-wide global — `cargo test --lib` runs
  without `serial_test` guards.
- `keybinding_dispatch.rs` — `dispatch_action(&action, &state) -> Option<TuiCommand>`.
  Match is exhaustive over all 105 schema variants (no wildcard arm).
  Real implementations for the user-facing chat / overlay / app actions
  (including `app:toggleTodos` → cycle expanded view, `app:toggleTranscript`
  → transcript overlay, `app:toggleTeammatePreview` → toggle preview).
  `Command(name)` → `ExecuteSlashCommand(name)` is the user-binding
  escape hatch. Feature-gated TS actions whose surface coco-rs hasn't
  built (Plugin*, Voice*, MessageActions:*, etc.) silently return None
  — matches TS behaviour where `useKeybinding` is never registered for
  unported features.
- `keybinding_setup.rs` — `install_keybindings()` returns a
  `KeybindingSetup { watcher, warnings_rx, initial, handle }`. Caller
  installs `handle` into `app.state.ui.kb_handle`, plumbs `warnings_rx`
  into the App's tokio::select! loop for toast surfacing, and holds
  `watcher` alive for the TUI's lifetime.

`keybinding_bridge::map_key(state, key)` runs the resolver via
`state.ui.kb_handle.resolve_key(...)` first; if the resolver fires an
action with a TUI handler it wins. If the resolver consumed the
keystroke (chord cancelled, null unbind, pending chord), the keystroke
is swallowed — preventing the legacy cascade from doing something a
user-customized binding wouldn't do. Otherwise the legacy hardcoded
cascade runs for TUI-only shortcuts (F1 help, Ctrl+, settings, …) that
aren't in the TS default schema.

The help overlay (`render_overlays/help.rs`) renders shortcuts dynamically
via `state.ui.kb_handle.display_for(action, ctx)` so user re-bindings
reflect immediately. i18n keys carry only the description text.

The chat widget (`widgets/chat/render_system.rs`) takes
`Option<&'a KeybindingHandle>` via builder so the truncation hint
(`…(<chord> to see full summary)`) reflects the user's actual binding.

## Conventions

- **Wire format** for actions and contexts is exactly TS — `app:exit`,
  `Global`, etc. Round-trip through serde is lossless.
- **Chord syntax**: combos joined by `+`, chord steps separated by
  whitespace. `" "` (a single space) is the space-key binding.
- **Canonical key names** match TS: `escape`, `enter`, `delete`,
  `backspace`, `pageup`, `pagedown`, `space`. Aliases (`esc`, `return`,
  `del`, `bs`, `pgup`, `pgdn`) normalize to the canonical form at parse
  time.
- **Last-wins** within a context: registering the same chord twice with
  different actions, the later registration wins (mirrors TS `findLast`).
- **Context priority**: callers pass an ordered context stack to
  `ChordResolver::feed`; the most-specific context's bindings are
  searched first.
- **Chord timeout**: 1 second between combos in a multi-combo chord
  (mirrors TS `CHORD_TIMEOUT_MS = 1000`). Drive via `ChordResolver::tick(now)`.

## Tests

Per-module `*.test.rs` companion files. `cargo test -p coco-keybindings
--all-features --lib` runs all 96.

## Known Follow-ups

(none — all G3 / A2 / N4 follow-ups landed; see the ResolverResult test
suite + `keybinding_dispatch.test.rs` for coverage)

## Future TS porting

TS feature-gated blocks deferred until a Cargo feature wires them up:

- `KAIROS` / `KAIROS_BRIEF` → `app:toggleBrief` (Anthropic-internal feature)
- `QUICK_SEARCH` → `app:globalSearch`, `app:quickOpen`
- `TERMINAL_PANEL` → `app:toggleTerminal`
- `MESSAGE_ACTIONS` → `chat:messageActions` + 11 actions in MessageActions context
- `VOICE_MODE` → `voice:pushToTalk` (needs SoX + microphone probes)

Action variants for these are present in the enum (so user configs can
parse) but no defaults are emitted.
