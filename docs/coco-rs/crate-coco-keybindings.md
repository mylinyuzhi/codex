# coco-keybindings — Crate Plan

TS source: `src/keybindings/` (12 files, 2.6K LOC)

## Dependencies

```
coco-keybindings depends on:
  - coco-types, serde, serde_json

coco-keybindings does NOT depend on:
  - coco-tools, coco-query, coco-inference, any app/ crate
```

## Data Definitions

```rust
pub struct Keybinding {
    pub key: String,          // e.g. "ctrl+s", "ctrl+shift+p", "ctrl+x ctrl+k" (chord)
    pub action: KeybindingAction,
    pub context: KeybindingContext,
}

/// 18 keybinding contexts (from schema.ts).
/// Determines when a binding is active.
pub enum KeybindingContext {
    Global, Chat, Autocomplete, Confirmation, Help,
    Transcript, HistorySearch, Task, ThemePicker,
    Settings, Tabs, Attachments, Footer, MessageSelector,
    DiffDialog, ModelPicker, Select, Plugin,
}

/// 50+ keybinding actions (from schema.ts).
/// Grouped by namespace: app:*, chat:*, history:*, autocomplete:*, confirm:*, etc.
pub enum KeybindingAction {
    // app:*
    AppQuit, AppHelp, AppToggleVerbose, AppToggleTheme,
    AppCycleMode, AppShowCommandPalette, AppShowModelPicker,
    AppShowSessionBrowser, AppShowTasks, AppToggleThinking,
    // chat:*
    ChatSubmit, ChatNewline, ChatClear, ChatCompact,
    ChatInterrupt, ChatPaste, ChatImagePaste, ChatUp, ChatDown,
    ChatHistoryPrev, ChatHistoryNext, ChatUndo, ChatRedo,
    ChatSelectAll, ChatBackgroundAll,
    // history:*
    HistoryUp, HistoryDown, HistorySelect,
    // autocomplete:*
    AutocompleteAccept, AutocompleteNext, AutocompletePrev, AutocompleteDismiss,
    // confirm:*
    ConfirmYes, ConfirmNo, ConfirmAlways, ConfirmNever,
    ConfirmYesToAll, ConfirmExplain, ConfirmOpen, ConfirmDiff,
    // ... (tabs, transcript, footer, etc.)
}
```

## Core Logic

```rust
/// Loading order: platform defaults → user overrides (~/.claude/keybindings.json).
/// User bindings merged over defaults (override, not replace).
pub fn load_keybindings() -> Vec<Keybinding>;
pub fn save_keybindings(bindings: &[Keybinding]);

/// Chord support: "ctrl+x ctrl+k" notation.
/// Parser splits on space, each segment is a key combo.
/// Ambiguity detection: warns if chord prefix conflicts with single-key binding.
pub fn parse_shortcut(shortcut: &str) -> Vec<KeyCombo>;

/// Platform-specific defaults (from defaultBindings.ts 340 LOC):
/// IMAGE_PASTE_KEY: alt+v (Windows) | ctrl+v (other)
/// MODE_CYCLE_KEY: shift+tab (VT mode) | meta+m (Windows without VT)
pub fn get_platform_defaults() -> Vec<Keybinding>;

/// Reserved shortcuts: ctrl+c (interrupt) and ctrl+d (exit).
/// Double-press semantics: first press = soft action, second within 500ms = hard action.
pub fn is_reserved_shortcut(key: &KeyCombo) -> bool;

/// Key event matching: compare incoming key event against registered bindings
/// for the current active context.
pub fn match_keybinding(
    event: &KeyEvent,
    context: KeybindingContext,
    bindings: &[Keybinding],
) -> Option<KeybindingAction>;
```
