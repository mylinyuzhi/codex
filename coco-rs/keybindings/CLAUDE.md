# coco-keybindings

Keyboard shortcut resolution: 18 contexts (global / input / conversation / permission / search / plan / diff / agent / worktree / ...), 73+ actions in `namespace:camelCase`, chord support (`ctrl+x ctrl+k`), platform defaults, context-first resolution with global fallback.

## TS Source
- `keybindings/schema.ts` — context + action enums (authoritative names)
- `keybindings/defaultBindings.ts` — platform-conditional defaults
- `keybindings/loadUserBindings.ts` — `~/.claude/keybindings.json` merge
- `keybindings/parser.ts`, `shortcutFormat.ts`, `template.ts` — chord parsing
- `keybindings/match.ts`, `resolver.ts`, `KeybindingContext.tsx`, `KeybindingProviderSetup.tsx` — runtime resolution
- `keybindings/reservedShortcuts.ts` — Ctrl+C/Ctrl+D double-press semantics
- `keybindings/validate.ts` — user-override validation
- `keybindings/useKeybinding.ts`, `useShortcutDisplay.ts` — React glue (not ported)

Paths relative to `/lyz/codespace/3rd/claude-code/src/`.

## Key Types
- `Keybinding` — `{key, action, context, when}`
- `KeybindingRegistry` — context-indexed `Vec<Keybinding>` with `resolve(key, context)` (context-first, falls back to context-less); `with_defaults()` constructor
- `KeyContext`, `KeybindingResolver` — from `context` module
- `KeyChord`, `KeyCombo`, `parse_chord`, `parse_combo` — from `parser` module
- `ChordResolver`, `ResolveOutcome` — from `resolver` module
- `ValidationIssue`, `validate()` — from `validator` module

## Key Functions
- `load_default_keybindings()` — minimal starter set (input: Ctrl+C interrupt, Ctrl+D quit, Enter submit, Tab autocomplete; dialog: Esc cancel; global: Ctrl+L clear, Ctrl+O compact)
- `get_all_defaults()` — full context/key/action tuples (global, input with kill-line/word-delete, conversation, permission y/n/a, search navigation)
- `ALL_CONTEXTS` — const array of known context names

## Modules
- `parser` — chord tokenization, combo parsing
- `resolver` — chord state machine for multi-key sequences
- `validator` — override validation + ambiguity warnings
- `context` — `KeyContext` enum + resolver wiring
