# TUI Overall Design

Status: design target for the next `coco-rs/app/tui` UI consolidation.

Baseline: `crate-coco-tui.md` documents the existing TEA architecture, state
model (`SessionState` + `UiState`), `Overlay` enum and 9-tier priority queue,
streaming pacing, notification backends, theme palette, and keybinding-context
routing. This document is the consolidation delta on top of that baseline — it
does not replace it. When this doc and `crate-coco-tui.md` disagree, the
consolidation direction here is the target; cite the baseline for current-state
semantics.

Primary Rust scope:

- `coco-rs/app/tui`
- `coco-rs/app/cli/src/tui_runner.rs`
- `coco-rs/commands`
- `coco-rs/common/types/src/event.rs`

Primary TS reference scope:

- `/lyz/codespace/3rd/claude-code/src/components`
- `/lyz/codespace/3rd/claude-code/src/screens/REPL.tsx`
- `/lyz/codespace/3rd/claude-code/src/commands`
- `/lyz/codespace/3rd/claude-code/src/components/memory`

Reference Rust scope:

- `codex-rs/tui`

This document describes the target UI shape, why the shape fits
`coco-rs`, and how to implement it without copying TS React/Ink
components into ratatui one-for-one.

## What

The TUI should converge on a single Conversation Workspace made of five
visible regions:

1. Header and status surface.
2. Conversation transcript.
3. Turn activity surface.
4. Input surface.
5. Overlay layer.

The current TEA ownership stays intact:

```text
CoreEvent / terminal input
        -> update
        -> AppState
        -> presentation view models
        -> ratatui widgets
```

`AppState` remains the source of facts. View models convert facts into
renderable rows, labels, badges, scroll windows, and semantic styles. Widgets
render those view models. Effects such as opening editors, changing models,
or sending approval responses stay in `update` / CLI runner code.

## Why

The TS UI is React/Ink. `coco-rs` is ratatui with explicit state and update
functions. A direct component port would make the Rust UI harder to reason
about because React component composition, hooks, and suspense-like data
loading do not map cleanly to ratatui's frame-by-frame render model.

The design uses TS as the behavioral reference, not as the structural
reference:

- Keep the Rust TEA loop, overlay queue, keybinding bridge, theme system, and
  event dispatch.
- Port TS interaction semantics such as memory picker rows, model picker
  axes, thinking collapse, tool lifecycle display, and teammate/subagent
  activity.
- Avoid UI-local provider inference and LLM-name hardcoding. Provider/model
  display must come from runtime config and session-frozen catalogs.
- Avoid input-driven side effects. The input bar submits intent; overlays and
  update handlers own file editing, picker selection, and configuration.

This also fixes the main UI consistency issue in the current code: complex
overlays are split between typed/styled renderers and simple
`(title, body, color)` string bodies. The model picker already requires a
custom renderer. New complex overlays, especially `/memory`, should use the
same typed-rendering direction instead of adding another string renderer.

## TS Interaction Mirror Contract

For supported `coco-rs` capabilities, the TUI should mirror the TS user
interaction as closely as ratatui and the `coco-rs` runtime model allow. The
mirror target is observable behavior: key flow, default focus, cancel/rollback,
loading and empty states, transcript messages, footer hints, row descriptions,
disabled state, preview behavior, and side effects. It is not a requirement to
copy React component structure, TS feature flags, Anthropic-only backend paths,
or Claude-specific naming.

Parity rules:

- Start every interactive surface from the TS component or command that owns
  the same user workflow. If `coco-rs` intentionally diverges, write the
  reason in this document or the crate-local design note.
- Route all transient UI through a typed surface stack. The focused surface
  handles local keys first, then the composer, then global interrupt/quit.
- Preserve TS cancellation semantics: preview flows roll back, form flows close
  without side effects, and user-visible command results are written to the
  transcript instead of disappearing as toast-only feedback.
- Preserve TS default focus and current-item visibility. A currently active
  value that is absent from the normal option list must be rendered as a
  synthetic current row instead of hidden.
- Preserve TS hint truthfulness. Footer, picker, permission, and help hints
  must be generated from resolved keybindings and current focus state.
- Keep terminal-specific adaptations centralized. Use alternate keybindings or
  compact hints where needed, but do not let terminal capability checks leak
  into business state.
- Translate unsupported TS features into explicit non-goals. For example,
  skip Ultraplan/CCR-only behavior, but keep plan approval and feedback dialog
  mechanics for the `coco-rs` plan workflow.

## Rust TUI Reference Patterns

`codex-rs/tui` is a stronger structural reference than TS for terminal
mechanics because it solves similar ratatui problems with explicit state,
measurement, key routing, and snapshot-tested layouts. Absorb these patterns,
but do not copy the large `ChatWidget` / `BottomPane` ownership shape
wholesale. Translate the ideas into the `coco-rs` TEA boundary:
`AppState -> view models -> widgets`, with effects outside render.

Patterns to carry forward:

- Interactive surface stack: the composer/input state survives while transient
  views are active, and key routing gives the focused local surface first
  chance before global interrupt/quit handling.
- Renderable measurement contract: complex surfaces expose render plus
  `desired_height(width)` and optional cursor metadata, so layout can measure
  before drawing instead of guessing from strings.
- Transcript cells: committed transcript entries are separate from the
  in-flight streaming cell. Transcript overlays can include a render-only live
  tail cached by width, active-cell revision, continuation state, and animation
  tick.
- Pager overlay: long read-only surfaces use a consistent pager with
  line/page/half-page/top/bottom navigation, follow-bottom behavior, cached
  item heights, and a compact percent footer.
- Selection list mechanics: selection state tracks actual item indices, not
  filtered visible row numbers; disabled rows are skipped; search, tabs,
  toggles, side content, live preview callbacks, and cancel rollback are part
  of the shared picker contract.
- Footer rendering: footer/status hints are pure props. Width collapse follows
  an explicit priority order so action-required hints remain visible and text
  never overlaps.
- Composer edge cases: history search, queued input, attachment placeholders,
  non-bracketed paste bursts, IME input, remote image rows, and kill-buffer
  preservation are explicit state-machine behavior, not incidental rendering.
- Terminal/layout defenses: terminal-aware keybinding fallbacks, positive-width
  guards, Unicode-width-aware truncation, resize debounce, and conservative
  reflow caps prevent narrow-terminal corruption.
- Status adapters: protocol/runtime snapshots are converted into stable display
  structs that classify available, stale, missing, and unavailable state before
  render.

## Module Design

Introduce a pure presentation layer under `app/tui/src/presentation/`.

```text
app/tui/src/presentation/
  mod.rs
  styles.rs
  layout.rs
  renderable.rs
  surface_stack.rs
  conversation.rs
  streaming.rs
  input.rs
  suggestions.rs
  footer.rs
  activity.rs
  pager.rs
  picker.rs
  dialogs.rs
  permissions.rs
  command_surfaces.rs
  notifications.rs
  model_picker.rs
  memory_picker.rs
  diff.rs
  status.rs
  terminal.rs
```

### `styles.rs`

`Theme` remains the palette contract. Add a thin semantic facade that maps
existing theme fields into UI intents.

```rust
pub struct UiStyles<'a> {
    pub theme: &'a Theme,
}

impl UiStyles<'_> {
    // Picker / list
    pub fn picker_selected(&self) -> Style;
    pub fn picker_current(&self) -> Style;
    pub fn picker_unavailable(&self) -> Style;
    pub fn picker_disabled(&self) -> Style;
    pub fn picker_header(&self) -> Style;

    // Chrome
    pub fn border(&self) -> Style;
    pub fn border_focused(&self) -> Style;
    pub fn scrollbar(&self) -> Style;
    pub fn hint(&self) -> Style;
    pub fn hint_emphasis(&self) -> Style;

    // Intent
    pub fn success(&self) -> Style;
    pub fn warning(&self) -> Style;
    pub fn error(&self) -> Style;

    // Transcript semantics
    pub fn user_message(&self) -> Style;
    pub fn assistant_message(&self) -> Style;
    pub fn thinking(&self) -> Style;
    pub fn system_message(&self) -> Style;

    // Tool lifecycle
    pub fn activity_running(&self) -> Style;
    pub fn activity_completed(&self) -> Style;
    pub fn activity_failed(&self) -> Style;

    // Mode banners
    pub fn plan_mode(&self) -> Style;
    pub fn permission_mode(&self) -> Style;

    // Diff
    pub fn diff_added(&self) -> Style;
    pub fn diff_removed(&self) -> Style;
    pub fn diff_added_word(&self) -> Style;
    pub fn diff_removed_word(&self) -> Style;

    // Input syntax
    pub fn mention(&self) -> Style;          // @path / @agent / @#symbol
    pub fn slash_command(&self) -> Style;    // /cmd
    pub fn pasted_pill(&self) -> Style;      // [Pasted text #N]
}
```

Rules:

- `UiStyles` is the only coupling point between presentation code and the
  theme palette. Widgets must not read `theme.X` directly. If a widget needs
  a style that is not in `UiStyles`, add a method here — the facade grows
  with real demand, not by speculation.
- Do not hardcode widget colors.
- Do not add new `Theme` fields until an existing field cannot express the
  semantic intent.
- If `Theme` is extended, update builtin themes, custom theme parsing,
  ANSI/daltonized variants, and snapshots together.
- `.white()` remains forbidden. Use reset/default foreground unless a theme
  semantic says otherwise.
- The method list above is seed coverage, not a freeze. Phase 4 onward may
  add methods as widgets migrate; each addition must justify why an existing
  semantic does not fit.

This keeps custom themes, auto mode, hot reload, light/dark modes, and
daltonized themes working across new UI.

### `layout.rs`

Shared terminal layout helpers live here, not inside individual renderers.

Rules:

- Guard fixed-prefix layouts with helpers that return `None` when no positive
  content width remains. Do not pass zero-width content into wrapping code.
- Use Unicode-width-aware truncation with styled ellipsis when a single-line row
  must fit.
- Measure multi-line text through ratatui primitives such as
  `Paragraph::line_count` with the same wrap settings used for rendering.
- Keep resize and reflow policy explicit. If terminal scrollback or replay is
  repaired after resize, debounce the repair and keep source-backed transcript
  cells as the source of truth.

### `renderable.rs`

Use a small presentation-only measurement trait for complex composed surfaces.
It should not become a business abstraction and it should not replace simple
widgets.

Suggested shape:

```rust
pub trait UiRenderable {
    /// Lines visible in the inline transcript or surface body.
    fn display_lines(&self, width: u16) -> Vec<Line<'static>>;

    /// Lines visible in the full transcript pager. Defaults to display_lines.
    /// Override when the inline view truncates or hides content the pager
    /// should expose (system reminders, large tool output previews).
    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.display_lines(width)
    }

    /// Wrapped height at this width. The default uses `Paragraph::line_count`
    /// with the same wrap settings as the default render impl so measure and
    /// draw stay in sync. Override for surfaces that compose lines from
    /// multiple regions (pagers, multi-column dialogs).
    fn desired_height(&self, width: u16) -> u16 {
        Paragraph::new(Text::from(self.display_lines(width)))
            .wrap(Wrap { trim: false })
            .line_count(width) as u16
    }

    /// Default render draws display_lines as a wrapped Paragraph. Complex
    /// surfaces (pickers, multi-region dialogs, the input bar) override.
    fn render(&self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(Text::from(self.display_lines(area.width)))
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }

    /// Cursor metadata for surfaces with an editable caret.
    fn cursor(&self, area: Rect) -> Option<UiCursor> { None }
}
```

The trait is one, not two: simple transcript cells implement only
`display_lines` and inherit `desired_height` / `render`; complex surfaces
override `render` and `desired_height` directly. The pager and the inline
transcript share `display_lines` / `transcript_lines` as the line accessors —
this is the codex-rs `HistoryCell` pattern unified into one entry point.

Good targets are pager rows, transcript cells, picker side content, footer
variants, and rich input surfaces. Simple one-off rows can remain plain
`Line` / `Paragraph` values.

### `surface_stack.rs`

Own transient interactive surfaces independently from the composer. This is
the Rust TEA equivalent of TS mounting dialogs over `PromptInput` while
preserving the prompt state underneath.

**Relationship to the existing `Overlay` enum.** The surface stack is the
focus router, not a replacement for `state/overlay.rs`. The `Overlay` enum
and its 9-tier `priority()` queue (documented in `crate-coco-tui.md`) stay
as the typed domain state — they decide *which* overlay is active and how
agent-driven overlays queue behind security-critical ones. The surface stack
decides *who handles a key* once an overlay is mounted, and what survives
underneath. Concretely:

- `state/overlay.rs::Overlay` — domain state, priority tiers, queue overflow
  policy. Owned by `update` code.
- `presentation/surface_stack.rs` — focus stack item per mounted surface,
  routes keys, holds focus-local state, preserves composer underneath.
  Owned by `update` plus the focused surface.

Migrating to the surface stack must preserve every `Overlay::priority` tier
and the `MAX_OVERLAY_QUEUE` overflow rule. The stack adds focus routing on
top; it does not change priority semantics.

Targets:

- picker overlays
- permission prompts
- request-user-input / AskUserQuestion-style forms
- MCP elicitation forms
- history search, quick open, and global search
- prompt suggestion lists and footer selection
- help, status, transcript, and diff pagers

Rules:

- The stack item owns focus-local state and key handling. It emits typed
  actions to update code; it does not mutate runtime config or files directly.
- Composer text, cursor, pasted content ids, images, stash state, and queued
  command state survive while a stack item is focused.
- Key routing order is explicit and matches the order validated in
  `codex-rs/tui/src/bottom_pane`:
  1. Focused stack item handles local keys, including Esc and Ctrl-C when
     the surface is a confirmation, permission, or active picker. Ctrl-C
     on confirmation surfaces cancels the request rather than killing the
     TUI.
  2. The existing `KeybindingContext` resolution (Confirmation → Picker →
     Scrollable → Autocomplete) routes the key against the focused surface
     kind.
  3. Composer state machine (history search, paste bursts, IME, kill
     buffer) gets the key if the surface declined.
  4. Global keybindings (interrupt, quit, help) only fire after the
     composer also declined.
- Opening a second surface should either replace the current surface or push a
  child explicitly. Implicit nested overlays are forbidden because they make
  cancellation and footer hints ambiguous.
- The stack must not bypass `Overlay` priority. A higher-priority overlay
  arriving while a lower-priority surface is focused displaces the focused
  surface back into the queue (or to a tier-appropriate slot), exactly as
  `UiState::set_overlay` already does today.

### `picker.rs`

Add shared picker primitives, not a generic business abstraction.

```rust
pub struct OverlayFrame<'a> {
    pub title: Line<'a>,
    pub border_style: Style,
    pub footer: Option<Line<'a>>,
}

pub enum PickerRow<'a> {
    Header(Line<'a>),
    Item(PickerItem<'a>),
    Blank,
}

pub struct PickerItem<'a> {
    pub id: PickerItemId,
    pub label: Line<'a>,
    pub description: Option<Line<'a>>,
    pub selected: bool,
    pub current: bool,
    pub disabled: bool,
}
```

`PickerScaffold` owns the common terminal mechanics:

- centered sizing with min/max bounds
- `Clear`
- border and title
- filter line
- viewport calculation
- selected row highlight
- disabled/current row styling
- footer hints
- optional tabs
- stable actual-item selection across filtering
- disabled-row skipping
- optional side content with side-by-side layout only when minimum widths fit,
  otherwise stacked fallback
- live selection callback for preview flows
- cancel callback for rollback flows
- number shortcuts only when search is not capturing typed digits
- page up/down movement by visible count
- optional edge callbacks such as up-from-first and down-from-last
- selectable input rows for permission feedback and editable prefixes
- multi-select rows with checkbox state and optional submit row
- inline descriptions and compact / expanded / compact-vertical layouts
- hide-index mode for dense dialogs such as MCP server selection
- full-width digit and space-key normalization where TS accepts it
- input-mode Tab toggling, with Enter returning to text input when the row is
  actively capturing text
- optional editor/image-paste hooks only as typed intents, never direct side
  effects from the widget

It does not own domain selection behavior. Update handlers still decide what
Enter, Tab, arrows, and typed characters mean for each overlay.

Also add a `FuzzyPicker` variant for TS-style search surfaces. It differs from
`PickerScaffold` in two ways: caller-owned filtering and an always-visible
search box.

Fuzzy picker behavior:

- caller receives every query change and passes filtered items back in
- `on_focus` fires when the focused item changes so preview loading stays out
  of render
- Enter is the primary action; Tab may either alias Enter or run a separate
  action; Shift-Tab is optional
- direction can be down or up, with up-mode placing newest/best rows next to
  the input while arrow keys still match visual direction
- preview can render to the right or below, but the layout structure stays
  stable even when there is no focused preview
- visible count is capped by terminal rows with a small minimum, so cursor
  positioning and stale rows do not break on short terminals
- compact hint mode drops secondary hints on narrow terminals
- backspace on an empty search field should not cancel the picker
- empty and match-count labels are caller-provided so loading, searching, and
  result-limit states remain truthful

Use this for quick open, global search, history search, log/session resume
search, and any future searchable picker with preview.

### `model_picker.rs`

The model picker is the reference for complex pickers. It is not a simple
list.

It has three axes:

1. Role axis: `Main`, `Fast`, `Plan`, `Explore`, `Review`, `HookAgent`,
   `Memory`, `Subagent`.
2. Model axis: provider-grouped model entries.
3. Effort axis: supported thinking efforts for the focused model.

Data source rules:

- Use `SessionState.model_catalog` for rows.
- Use `SessionState.provider_statuses` for unavailable state.
- Use `SessionState.model_by_role` for the current binding.
- If the current role binding is not present in the catalog or filtered
  allowlist, append a synthetic current row with the structured
  `(provider, api, model_id)` binding instead of hiding the active model.
  This mirrors TS `ModelPicker` behavior for current models that are absent
  from the base option set.
- Do not infer provider from model id prefixes in production paths.
- The fallback that infers provider from builtin model ids should be removed
  or constrained to tests where no session catalog exists.
- Keep unavailable reasons typed through the view model. Do not collapse them
  to a boolean before rendering the footer or row badge.

Rendered row data:

```rust
pub struct ModelPickerView {
    pub role_tabs: Vec<RoleTab>,
    pub filter: String,
    pub rows: Vec<ModelPickerRow>,
    pub effort_tabs: Vec<EffortTab>,
    pub unavailable_summary: Option<String>,
    pub hints: Line<'static>,
}

pub enum ModelPickerRow {
    ProviderHeader {
        provider: String,
        provider_display: String,
    },
    Model {
        provider: String,
        provider_display: String,
        model_id: String,
        display_name: String,
        context_window: Option<i64>,
        current: bool,
        synthetic_current: bool,
        unavailable_reasons: Vec<ProviderUnavailableReason>,
        supported_efforts: Vec<ReasoningEffort>,
        default_effort: Option<ReasoningEffort>,
    },
    Blank,
}
```

The selected index should continue to point at filtered model entries, not
headers. The view model resolves that into visible rows for rendering.

Interaction rules to mirror TS:

- Include a default/no-preference sentinel where a role can inherit the global
  default instead of pinning a model.
- Cap the visible row count and show a hidden-count footer such as "and N
  more" when the catalog is larger than the viewport.
- Left/right change effort for the focused model. If the user has not toggled
  effort explicitly, focus movement updates the preview/default effort only;
  persistence happens on confirmation.
- If a selected effort is unsupported by the focused model, downgrade to the
  nearest supported value in the view model and show a typed hint.
- If plan mode or another session override temporarily changes the active
  model, render that state explicitly and make model selection clear the
  temporary override through an update action.
- Fast-mode state belongs in the picker footer: show when it is on, when the
  focused model would turn it off, and when the user can toggle it from the
  command surface. Do not hide this behind provider-specific labels.
- Standalone command mode keeps an explicit cancel/exit hint and reports
  selection, cancellation, and validation failures as command results.
- Custom model ids follow the same validation route as config/model aliases.
  Known aliases may bypass remote validation; arbitrary ids should not mutate
  settings until validation succeeds or the user explicitly confirms the risk.

### `memory_picker.rs`

`/memory` becomes the only memory-management entrypoint. The previous
`#` prompt mode should be deleted rather than carried as a compatibility
path.

Delete or retire these surfaces:

- `PromptMode::Memory`
- `UserCommand::SubmitMemory`
- `TuiOnlyEvent::MemorySaved`
- `MessageContent::MemoryInput`
- `#note` help entry
- prompt-mode direct append to `CLAUDE.md`

Memory picker rows are typed:

```rust
pub enum MemoryPickerRow {
    File {
        scope: MemoryScope,
        path: PathBuf,
        label: String,
        description: String,
        exists: bool,
        depth: usize,
        imported_from: Option<PathBuf>,
    },
    Folder {
        kind: MemoryFolderKind,
        path: PathBuf,
        label: String,
        description: String,
    },
    Toggle {
        kind: MemoryToggleKind,
        label: String,
        description: String,
        enabled: bool,
        editable: bool,
    },
}
```

Minimum row set:

- existing memory files discovered for the session
- missing user memory entry
- missing project memory entry
- imported child files when discovery exposes them
- auto-memory folder row when the feature exists
- team memory folder row when team memory is enabled
- agent memory rows when active agents expose memory
- auto-memory and auto-dream toggles when those settings are available

TS mirror details:

- Remember the last selected memory path across opens and restore focus when
  that row is still present.
- Prime or clear the memory-file cache before opening `/memory` so the picker
  does not flash fallback rows and then replace them.
- Filter out auto-memory and team-memory entrypoint files from ordinary file
  rows; expose those directories through explicit folder/toggle rows instead.
- Render labels and descriptions by row kind: user memory, project memory,
  imported child rows with indentation, dynamic/agent memory rows, and missing
  rows with a clear new-file marker.
- Toggle rows are part of the same surface but can have a separate focus band
  above the file selector. Moving up from the first file can focus the last
  toggle; when a toggle is focused, file selection is disabled.
- Auto-dream rows are snapshot-stable for the lifetime of the picker so a live
  setting/task update does not remove the focused row mid-navigation.
- Dream status should distinguish running, never run, and last consolidated
  timestamp. If a manual command exists, the enabled idle state may hint it.
- Folder open should create the directory if missing before delegating to the
  opener/editor service, and errors should return as typed command results.
- File creation uses create-exclusive semantics so a concurrently-created
  memory file is not overwritten.

The TUI-side event payload must become row-kind aware before the renderer is
migrated. `DialogSpec::MemoryFileSelector` / `TuiOnlyEvent::OpenMemoryDialog`
should carry enough structured data to build `MemoryPickerRow` without
reconstructing memory semantics inside widgets. If this is staged, introduce
the new row enum first and populate only file rows initially; widgets and
renderers should consume `MemoryPickerRow`, not the old path/label/scope-only
shape.

Selection behavior:

- Enter on file: create parent directory, create missing file with
  create-exclusive semantics, then open through the external editor service.
- Enter on folder: open the folder with the platform opener or editor.
- Enter on toggle: dispatch a settings update.
- Esc: close and add a transcript-visible system message for cancellation.

Toast-only feedback is not enough for memory editing because it is a
user-visible command result. Success, cancel, and failure should also be
represented in the transcript as system messages.

### `conversation.rs`

`ConversationView` is the transcript projection.

It should group and render:

- user messages
- assistant text
- assistant thinking and redacted thinking
- tool calls
- tool results
- local bash input/output
- system messages
- slash command results
- teammate/user-agent messages

Rules:

- Transcript is history, not transient activity.
- Running state may be echoed compactly, but the detailed live view belongs in
  the activity surface.
- Hidden/meta messages remain controlled by transcript/debug settings.
- Thinking defaults should follow display settings and user toggles, not be
  hardcoded per renderer.
- Keep committed transcript entries separate from the active streaming entry.
  The active entry may mutate in place while a turn is running; committed
  entries should be stable and source-backed for reflow and transcript overlay
  rendering.
- Streaming markdown should accumulate source text and commit at safe
  boundaries. Width changes should re-render from source instead of preserving
  already-wrapped lines as the canonical state.
- Live-tail pacing, the `ActiveCellTranscriptKey` cache, and the streaming
  view model live in `streaming.rs`. `conversation.rs` projects the
  committed history only; it borrows the live tail as a renderable.

### `streaming.rs`

Presentation adapter for the in-flight streaming cell. Committed transcript
entries (`conversation.rs`) are stable and source-backed; the active streaming
cell mutates in place and needs cache-key discipline so consumers (inline
view, pager overlay, snapshot tests) do not re-render every frame.

Source state stays in `app/tui/src/streaming/` as today — this module is the
view-model + cache key, not a duplicate of the streaming pipeline.

Suggested shape:

```rust
pub struct StreamingView<'a> {
    pub source: &'a StreamingSource,    // borrowed from state/streaming
    pub mode: StreamMode,               // Text | Thinking | ToolUse
    pub spinner_tick: Option<u64>,
}

pub enum StreamMode { Text, Thinking, ToolUse }

/// Cache key for the live tail. Consumers memoize rendered lines by this
/// key. Width changes invalidate by `width`; source mutations bump
/// `revision`; spinner-driven animation invalidates by `animation_tick`.
/// Never include the rendered `Vec<Line>` in the key.
pub struct ActiveCellTranscriptKey {
    pub width: u16,
    pub revision: u64,
    pub is_stream_continuation: bool,
    pub animation_tick: Option<u64>,
}
```

Rules:

- Pacing policy (Smooth / CatchUp hysteresis, severe-backlog bypass) is
  defined in the baseline `crate-coco-tui.md` and implemented in
  `app/tui/src/streaming/`. This module exposes the policy as a parameter
  and never redefines it.
- Bump `revision` only on source mutation, not on layout changes.
- Width changes re-render from source. Already-wrapped lines are never the
  canonical state.
- Spinner ticks must not invalidate the source-derived line cache. They
  invalidate the live tail's overlay cache only (`animation_tick` field).
- Snapshot tests must exercise: narrow-then-wide width change mid-stream,
  spinner-tick advancing without source change, and source append without
  width change. Each must hit or miss the cache as expected.

### `pager.rs`

Large read-only surfaces use a shared pager instead of bespoke scroll code.

Targets:

- full transcript overlay
- help / shortcuts overlay
- status detail overlay
- large command output or diagnostic views
- future diff or file-preview overlays

Pager behavior:

- up/down, page up/down, half-page, top, bottom
- follow-bottom while new content streams, unless the user scrolls away
- cached desired height per row and width
- render-only live tail for in-flight transcript content
- compact footer with position or percent
- clear unused rows so stale glyphs never remain after shrink
- sticky prompt/footer support for long permission and plan-approval surfaces,
  so response options stay visible while the user scrolls through context
- selected-message cursor navigation for message actions and search matches

### `activity.rs`

`TurnActivityView` is the live "what is happening now" surface.

It should merge:

- active thinking state
- queued/running tool executions
- tool progress
- subagent tree
- background tasks
- plan/todo progress
- stream stall or interrupt state

Suggested model:

```rust
pub struct TurnActivityView {
    pub sections: Vec<ActivitySection>,
}

pub enum ActivitySection {
    Thinking(ThinkingActivity),
    Tools(Vec<ToolActivityRow>),
    Subagents(SubagentTree),
    Plan(Vec<PlanActivityRow>),
    Background(Vec<BackgroundTaskRow>),
}
```

The side panel should render this unified activity view instead of independently
deciding between tool, subagent, coordinator, and task panels in the top-level
`render.rs`.

Any display mode derived from settings, feature gates, or environment variables
must be folded into `AppState` before render. The view layer should not call
config/env helpers to decide between coordinator, subagent, tool, or task
layouts.

### `input.rs`

The input surface should be a single widget and view model. The current split
between `render.rs::render_input` and `widgets/input.rs` should be collapsed.
The unified view model owns placeholder priority, prefix stripping, visual
cursor math, syntax-highlight spans, and title/hint selection.

The input/composer state should survive transient bottom-pane style views such
as pickers, approval prompts, request-user-input forms, and history search.
Key routing should be explicit:

1. focused transient surface
2. composer/input state machine
3. parent/global interrupt and quit handling

Supported input modes:

- normal chat input
- `!` bash prefix mode
- slash command suggestions
- attachments and pasted content
- queued command preview
- plan/permission context hint
- reverse history search
- bracketed and non-bracketed paste handling
- attachment placeholders and remote attachment rows

TS mirror details:

- Placeholder priority should match TS: no placeholder when input is non-empty;
  teammate/agent message targets override generic hints; queued editable
  command hints appear before example-command suggestions; proactive hints must
  not flicker over active command flows.
- Input mode detection supports normal prompt and `!` bash prefix mode. `#`
  has no special meaning.
- Highlight slash command triggers, file/directory suggestions, team/user
  mentions, thinking/plan/token-budget markers, and pasted-content references
  as spans derived from the same parser used by update logic.
- Large text paste becomes a collapsed pasted-content reference with a stable
  id. Resume/continue sessions must seed the next pasted-content id from
  existing transcript references to avoid collisions.
- Pasted text is ANSI-stripped and line-ending normalized before it becomes a
  reference. Pasting `!cmd` into an empty prompt should enter bash mode when
  the TS workflow would.
- Image and file attachment placeholders are cursor-addressable chips. Delete,
  backspace, and cursor movement must avoid orphaning attachment state.
- Prompt suggestions block submit while an actionable suggestion is focused,
  except for directory completion where Tab completes the path.
- Up/Down dispatch depends on cursor position, suggestion visibility, footer
  focus, and queued-command state. Up at the top may open history or pull a
  queued command into the editor; Down at the bottom may move into the footer.
- Reverse history search preserves the current prompt as the initial query and
  restores selected pasted-content references before submit.
- Stash behavior mirrors TS: save prompt/cursor/pasted content when non-empty,
  restore when empty and a stash exists.
- External editor opens through the shared editor service and expands pasted
  references into editable text. On return, re-collapse references only through
  the typed pasted-content parser.
- Quick open, global search, and history search are stack surfaces using
  `FuzzyPicker`; insertion uses spacing rules from the composer, not raw string
  concatenation.
- Vim mode, if supported, is a composer state variant with matching footer
  hints. It must not fork the rest of the input renderer.
- Escape/backspace/delete/Ctrl-U at cursor zero leave special modes according
  to the focused mode. Escape double-press suppression while suggestions are
  open should be explicit, not emergent.
- Inline overlays such as model picker, fast-mode picker, and thinking toggles
  should be memoized/stabilized so unrelated notifications do not move the
  prompt or change focus.

Unsupported:

- `#` memory prompt mode
- input-bar file writes
- input-bar editor launching

### `suggestions.rs`

Own slash command, file path, directory, mention, and prompt-suggestion view
models. This keeps `input.rs` focused on composer mechanics.

Rules:

- The parser that detects trigger spans should also produce the suggestion
  query. Rendering and submit handling must not rediscover trigger positions
  independently.
- Suggestion rows carry kind, replacement range, display label, description,
  disabled reason, and whether selecting the row submits or only edits input.
- Directory completion via Tab is distinct from Enter selection.
- Accepting an empty-input prompt suggestion is allowed only when no pasted
  content/image is attached and the current input still matches the suggestion
  prefix.
- Suggestion surfaces can move into fullscreen/overlay presentation when the
  prompt is in a constrained layout, but they must use the same row model and
  key hints as the inline footer.
- All four suggestion kinds (slash command, file, agent, symbol) share a
  single focused-trigger state machine in `state/ui.rs`
  (`UiState::active_suggestions: Option<ActiveSuggestions>`). Do not
  introduce parallel per-kind state. Async kinds (file, symbol) install the
  trigger with empty items and let the app loop dispatch the search; stale
  results are discarded when the trigger query no longer matches. This is
  the existing baseline — preserve it.

`#foo` is ordinary chat text. Memory management enters through `/memory`.

### `footer.rs`

The footer is a pure renderer over footer props. It should not inspect config,
terminal state, or input state directly.

Suggested inputs:

- resolved keybinding hints
- queue / pending input hint
- active mode hint
- contextual overlay hint
- error or action-required hint

Width handling must be priority-based: preserve action-required or queued-input
hints first, then collapse lower-priority shortcut/context hints. Do not let
footer spans overlap or wrap into unrelated UI.

TS mirror details:

- Prompt suggestions replace the ordinary footer while active, except in
  fullscreen/constrained layouts where they move to the overlay layer.
- Help menu, footer navigation, and selected footer actions are focused
  surfaces. Typing a normal character should return focus to the composer.
- Status line rendering is conditional on settings, prompt mode, terminal
  height, active paste state, and custom status-line configuration.
- Narrow terminals use a column layout and hide optional shortcut hints before
  hiding action-required information.
- Bridge/IDE/reconnect state is a status indicator only when actionable or
  explicitly requested; failure states should surface as notifications.
- `? for shortcuts` and similar hints are suppressed when a custom status line
  or history-search hint already owns that space.

### `dialogs.rs`

Provide shared frame primitives for TS-style dialogs that are not generic
pickers.

Targets:

- theme picker and onboarding theme picker
- output-style picker inside config
- trust, managed-settings, invalid-settings, auto-mode opt-in, bypass
  permissions, cost threshold, and press-enter-to-continue dialogs
- MCP server approval/import dialogs
- settings/config submenus

Rules:

- Dialog view models carry title, optional subtitle, color intent, body
  renderable, footer hints, and cancel policy.
- Standalone command dialogs may show a border/input guide; embedded config
  submenus may hide them. This is a surface property, not a separate renderer.
- Theme picker must support live preview on focus and rollback on cancel.
  Saving happens only on confirmation. Syntax-highlight toggling is a typed
  action with a truthful hint.
- Output-style picker loads project/user/builtin styles, shows a loading row,
  falls back to builtin styles on load error, and keeps initial style focused.

### `permissions.rs`

Tool permission UI should be a typed surface family, not a string dialog. TS
has one dispatcher that chooses specialized permission views per tool while
sharing common prompt mechanics; mirror that shape in Rust.

Core types:

```rust
pub enum PermissionSurface {
    Bash(BashPermissionView),
    FileEdit(FileEditPermissionView),
    FileWrite(FileWritePermissionView),
    WebFetch(WebFetchPermissionView),
    NotebookEdit(NotebookEditPermissionView),
    EnterPlanMode(PlanModePermissionView),
    ExitPlanMode(PlanApprovalView),
    AskUserQuestion(UserQuestionView),
    Skill(SkillPermissionView),
    Mcp(McpPermissionView),
    Fallback(FallbackPermissionView),
}
```

Rules:

- Dispatch by typed tool identity, not display strings.
- Shared permission prompts support plain options, keybound options, inline
  descriptions, and optional feedback input for accept/reject choices.
- Tab toggles feedback input only for the focused feedback-enabled option.
  Empty feedback collapses when focus moves away; submitted feedback is trimmed
  and omitted when empty.
- Escape cancels the permission request and records cancellation state through
  update logic. Ctrl-C under confirmation context should reject/cancel the
  permission rather than killing the whole TUI when the TS workflow would.
- Permission frames support worker badges, title/subtitle color intents,
  classifier-in-progress/auto-approved state, and `on_user_interaction` so
  async auto-approval cannot dismiss a prompt after the user starts editing.
- A timeout can produce a notification that permission is needed, but the
  prompt remains the source of truth.
- Long plan approvals and large diffs use pager/sticky-footer support so
  response options remain visible while context is scrollable.
- Bash permission options mirror TS: approve once, reject, optional
  "do not ask again" suggestions, editable safe prefix when applicable, and an
  optional classifier-reviewed option. Command labels should strip output
  redirections when TS does.
- File edit/write permission views render structured diffs and path context,
  not raw JSON. Sed-style edits may use a specialized diff path when the tool
  input implies one.
- Ask-user-question/request-user-input supports multiple questions,
  single-select, multi-select, free-text "other" rows, preview markdown side
  panels, navigation between questions, and a final review/submit state.
- MCP elicitation forms validate fields synchronously or asynchronously through
  typed validators. URL/waiting flows are distinct surfaces, not ad hoc text.

### `command_surfaces.rs`

Interactive slash commands should resolve to typed command surfaces and command
result transcript messages.

Mirror these TS command groups when the corresponding `coco-rs` feature
exists:

- `/model`, `/fast`, `/effort`, `/plan`
- `/memory`
- `/theme`, `/config`, `/output-style` compatibility messaging
- `/permissions`, `/sandbox`
- `/mcp`
- `/agents`, `/tasks`, workflow/team selectors where supported
- `/session`, `/resume`, `/history`-style search
- `/context`, `/status`, `/diff`, `/export`, `/help`
- `/hooks`, `/plugins`, `/skills`, `/keybindings`
- `/add-dir`, `/ide`, `/login`, `/logout`, terminal setup and privacy/security
  settings
- low-friction utility commands such as clear/copy/rename/share/export,
  cost/usage/stats/rate-limit detail, diagnostics/doctor/env, reload/reset,
  rewind, and review/debug helpers when `coco-rs` exposes equivalents

Rules:

- Commands that open UI return a typed surface plus an eventual typed result.
  They should not render their own one-off string picker.
- Success, cancellation, validation failure, and side-effect failure are
  transcript-visible command results. Toasts/notifications may duplicate urgent
  information but must not be the only durable result.
- Deprecated TS commands may be mirrored as compatibility messages if
  `coco-rs` intentionally exposes the same command name. Do not add deprecated
  Rust behavior just because TS has a deprecated path.
- Configuration commands mutate settings through the existing config/update
  path, not directly from widgets.
- Maintain an audit table for TS-only commands. Each row should be mapped to
  `implemented`, `alias/compat-message`, `backend unsupported`, or
  `intentionally omitted`. This prevents silent parity gaps.

### `notifications.rs`

Notifications are transient UI, not command history. This module owns the
**display** of notifications (in-terminal toast rows, priority, expiry).
Notification **delivery** (OSC sequences, terminal detection, BEL handling,
tmux/screen DCS wrapping) stays in `widgets/notification.rs::NotificationBackend`
as today — that is a terminal-IO service, not presentation, and it must not
move into `presentation/`.

Toast severity tiers (carry over from the baseline):

```rust
pub enum ToastSeverity {
    Info,      // 3s, dim border
    Success,   // 3s, success border
    Warning,   // 5s, warning border
    Error,     // 8s, error border
}
```

Rules:

- Keep at most the highest-priority visible notification in the prompt/footer
  area when vertical space is constrained; clip to one row if needed.
- Cap the active toast queue at 5; drop the oldest on overflow. Auto-expire
  is checked on `Tick` (every 250ms).
- Keep the notification surface mounted/stable while suggestions or modal
  overlays are active so effects do not re-fire due only to layout changes.
- Bridge, clipboard image failure, editor failure, permission-needed timeout,
  and background task updates may produce notifications, but user-command
  results also need transcript entries.
- Notification priority and expiry live in update state. Renderers only
  receive display-ready rows.
- Severity styles come from `UiStyles` (`success` / `warning` / `error` /
  `hint`), never from `theme.X` directly.

### `status.rs`

The status surface should render provider/model state from structured model
bindings:

- provider id
- provider display label
- model id
- display name when known
- role and effort when relevant
- permission mode
- plan mode
- fast mode
- token/context state

Do not add a `title_model: String` or similar display-only shortcuts. Use
`ModelRole` and `(provider, api, model_id)` state.

Status view models should adapt protocol/runtime snapshots into display
structures before render. Classify data as available, stale, missing, or
unavailable in the view model so widgets only style known states.

TS mirror details:

- Status and context visualization should expose token/context usage,
  active model role, permission mode, plan mode, fast mode, connected IDE/bridge
  state, and memory/config warnings as separate typed fields.
- Custom status-line configuration can replace the default status line, but
  it must not erase action-required footer hints.
- Context visualization rows should be navigable when they imply an action
  such as opening `/memory`, `/config`, or status details.
- Provider/model unavailable state should appear as typed badges/hints rather
  than collapsing to a single generic warning.
- Status detail views use the shared pager when they exceed the available
  height.

### `diff.rs`

Render file edits, command diffs, and permission diffs through a shared
structured diff view model.

Rules:

- Keep path, operation type, old/new hunks, truncation state, and syntax-color
  availability typed.
- Fall back cleanly when color/syntax highlighting is unavailable; the fallback
  reason belongs in the view model, not a global renderer check.
- Permission dialogs, transcript tool output, and `/diff` command views reuse
  the same diff row model so narrow-width wrapping and snapshots stay aligned.
- Very large diffs move into `pager.rs` with sticky permission/footer support
  instead of overflowing the active prompt.

### `terminal.rs`

Centralize terminal capability and keybinding fallbacks.

Examples:

- alternate keybindings for terminals or multiplexers that intercept a shortcut
- whether inline images or remote image rows can be represented
- conservative row caps for resize/reflow work
- terminal family names used only for capability selection, never for business
  behavior

Resolved keybindings should be shown through the same data used by input,
footer, help, and picker hints.

Deliberate-off capabilities (these must stay off — codify here so a future
contributor does not flip them):

- **Mouse capture is intentionally disabled.** Never call
  `EnableMouseCapture`. The terminal keeps ownership of mouse events so
  native drag-to-select and Cmd/Ctrl-C work exactly as in `vim` and `less`.
  `TuiEvent` has no `Mouse` variant; `Event::Mouse` is dropped defensively
  in `app.rs`. Same choice as `codex-rs/tui`.
- **OSC 8 hyperlinks remain deferred** until ratatui ships a native
  `Span::hyperlink` primitive. The current `Span` buffer strips raw escape
  sequences, so embedding them produces stale glyphs after resize. Track
  upstream; do not work around with manual escape sequences in render code.

## How

Implement in small, testable steps.

### Phase 0: Build the TS interaction inventory

Before rewriting a surface, create a small parity note or fixture that records
the TS behavior being mirrored.

Minimum inventory:

- keybindings and focus order
- default focus/current-row behavior
- Enter, Tab, Shift-Tab, Escape, Ctrl-C, Up/Down, PageUp/PageDown behavior
- loading, empty, disabled, cancel, success, and failure text
- transcript result versus notification-only feedback
- preview/rollback behavior
- narrow-terminal behavior

Outcome:

- Every migrated interactive command has a short behavior target.
- Intentional divergences are visible before implementation.
- Snapshot tests can be named after behavior, not implementation details.

### Phase 1: Remove `#` memory prompt mode

Touch points:

- `app/tui/src/state/ui.rs`
- `app/tui/src/update/edit.rs`
- `app/tui/src/command.rs`
- `app/cli/src/tui_runner.rs`
- `common/types/src/event.rs`
- `app/tui/src/state/session.rs`
- `app/tui/src/widgets/chat/render_user.rs`
- `app/tui/src/render_overlays/help.rs`
- locale strings and tests

Outcome:

- `#foo` is normal input.
- No prompt-mode append to `CLAUDE.md`.
- `/memory` is the only memory editing path.

### Phase 2: Add presentation styles

Add `presentation/styles.rs` and route new widgets through it.

Outcome:

- New UI uses semantic style methods.
- Existing `Theme` remains the palette source.
- Custom theme and hot reload continue to work.

### Phase 3: Shared presentation primitives

Add the small shared primitives before migrating feature surfaces. Split
into four substeps so a regression can be bisected by primitive instead of
by phase. Each substep is committable on its own with snapshot coverage.

**Phase 3a — Layout and terminal helpers (pure refactor, zero behavior change):**

- positive-width and Unicode-width helpers under `layout.rs`
- terminal capability resolver and keybinding fallback table under
  `terminal.rs`
- codify deliberate-off capabilities: mouse capture stays disabled, OSC 8
  remains deferred
- snapshot coverage at narrow/normal/wide widths for any surface this phase
  touches (most won't, by design)

**Phase 3b — Measurement and rendering contracts:**

- `UiRenderable` trait with `display_lines` / `transcript_lines` /
  `desired_height` / `render` / `cursor` and the default impls that keep
  measure and draw in sync
- migrate the two or three simplest transcript cells onto it as a
  verification target; do not migrate complex surfaces yet

**Phase 3c — Surface stack and composer preservation:**

- focused transient surface stack with explicit push/replace policy
- composer text, cursor, pasted ids, images, stash state, and queue state
  preserved across stack pushes
- explicit reconciliation with `state/overlay.rs::Overlay` and the 9-tier
  priority queue (stack is focus router; priority queue is domain state)
- key-routing order codified: focused surface → keybinding context →
  composer → global

**Phase 3d — Footer, notifications, dialog frame:**

- pure footer props with width-collapse priority order
- notification display rows + priority + 4 severity tiers
  (delivery stays in the existing `NotificationBackend`)
- shared dialog frame primitive
- `UiStyles` facade covers all chrome/intent/picker semantics used by these
  primitives; no widget reaches `theme.X` directly

Outcome:

- Complex surfaces can measure before rendering.
- Dialogs and pickers no longer compete with composer state.
- Footer and keybinding hints stop duplicating width and terminal logic.
- Narrow terminal behavior is testable before the larger rewrites.
- Each substep ships with snapshots so the larger phases (4–11) can rely on
  the primitives being stable.

### Phase 4: Create `OverlayFrame` and `PickerScaffold`

Migrate the model picker first because it already has the richest behavior and
the most theme-sensitive styling.

Outcome:

- Model picker keeps role/model/effort behavior.
- Rendering code stops duplicating line padding, viewport, and selection
  styling.
- The old `model_picker_content()` string fallback can be removed.

### Phase 5: Permission, request, and MCP elicitation surfaces

Port permission and user-question surfaces onto typed view models before
rewiring every command that uses them.

Outcome:

- Tool permission prompts share option/feedback/cancel behavior.
- Long plan approvals and diffs can use pager plus sticky footer.
- Request-user-input / AskUserQuestion and MCP elicitation forms have typed
  validation, navigation, preview, and submit states.
- Ctrl-C/Escape behavior is local to the focused confirmation surface.

### Phase 6: External editor service

Replace direct editor spawning with a single service that:

- resolves `$VISUAL || $EDITOR || vi`
- parses command plus arguments with a real parser; do not split on whitespace
- pauses raw mode and alternate screen when needed
- waits for terminal editors
- restores the TUI
- emits transcript-visible system results

This service should be reused by `/memory`, prompt external editor, plan
editor, and future file-editing overlays.

Outcome:

- No widget or renderer launches a process.
- The update/runner layer owns terminal suspension and restoration.
- Editor success, cancel, and failure are available as structured events for
  transcript messages and toasts.

### Phase 7: Rebuild `/memory` as a typed picker

First expand the dialog/event payload to the typed `MemoryPickerRow` shape,
then migrate rendering to `PickerScaffold`. File-only data may be adapted into
`MemoryPickerRow::File` as a transitional source, but no renderer should depend
on the old path/label/scope-only structure.

Outcome:

- Memory picker shares overlay frame and selection styling with model picker.
- Memory row rendering can display scope, missing/existing state, path, and
  description without string concatenation.
- File and folder selections delegate side effects to the external editor /
  opener service.
- `/memory refresh` and list-only handlers should be removed or folded into the
  picker flow if they still exist.

### Phase 8: Conversation, diff, and pager surfaces

Add transcript cell view models and shared pager mechanics.

Outcome:

- Committed transcript history and active streaming state have distinct
  rendering contracts.
- Full transcript, help, status detail, and future large overlays share one
  scroll model.
- File diffs and permission diffs share a structured row model.
- Resize/reflow behavior uses source-backed entries, not previously wrapped
  lines as canonical state.

### Phase 9: Activity surface

Add `TurnActivityView` and migrate the right-side live panels to it.

Outcome:

- Tools, thinking, subagents, background tasks, and plan progress share one
  rendering contract.
- Transcript remains stable history.
- Live state is easier to snapshot because it is one view model.

### Phase 10: Input, suggestions, and footer unification

Move prompt rendering into the input widget path and remove duplicate input
rendering from top-level `render.rs`.

Outcome:

- Prefix stripping and visual cursor math live in one place.
- Attachments, suggestions, queued commands, and hints can be tested together.
- Memory prompt mode cannot reappear accidentally.
- Footer hints are rendered from props with one collapse policy.

### Phase 11: Slash command surface parity

Migrate remaining interactive slash commands to typed surfaces and transcript
results.

Outcome:

- Theme/config/model/permission/MCP/session/context/diff/help workflows use
  shared dialog, picker, fuzzy picker, permission, pager, and notification
  primitives.
- Deprecated or unsupported TS commands have explicit compatibility messages
  or documented non-goals.
- Every interactive command has at least one parity snapshot or focused unit
  test for cancellation/result behavior.

## Initial TS Command Audit Seed

This seed is not the final implementation table. It prevents the plan from
silently omitting command families discovered under
`/lyz/codespace/3rd/claude-code/src/commands`. Before implementation, split
these grouped rows into a checked-in command-by-command audit.

Mirror interaction when the feature exists:

- `add-dir`, `agents`, `bridge`, `config`, `context`, `ctx_viz`, `diff`
- `effort`, `fast`, `help`, `hooks`, `ide`, `keybindings`, `login`, `logout`
- `mcp`, `memory`, `model`, `output-style`, `permissions`, `plan`, `plugin`
- `privacy-settings`, `resume`, `sandbox-toggle`, `session`, `skills`
- `status`, `tasks`, `terminalSetup`, `theme`, `vim`

Preserve command-result behavior, opening typed UI only when the TS command has
an interactive surface and `coco-rs` has an equivalent backend:

- `autofix-pr`, `branch`, `clear`, `compact`, `copy`, `cost`, `doctor`, `env`
- `exit`, `export`, `extra-usage`, `feedback`, `files`, `issue`, `pr_comments`
- `rate-limit-options`, `reload-plugins`, `rename`, `reset-limits`, `review`
- `rewind`, `share`, `stats`, `summary`, `usage`

Treat as TS product/backend/platform-specific until proven otherwise:

- `ant-trace`, `backfill-sessions`, `break-cache`, `btw`, `bughunter`
- `chrome`, `color`, `debug-tool-call`, `desktop`, `good-claude`, `heapdump`
- `install-github-app`, `install-slack-app`, `mock-limits`, `mobile`
- `oauth-refresh`, `onboarding`, `passes`, `perf-issue`, `release-notes`
- `remote-env`, `remote-setup`, `stickers`, `tag`, `teleport`, `thinkback`
- `thinkback-play`, `upgrade`, `voice`

If `coco-rs` exposes a TS-specific command name, implement a compatibility
message or document the intentional omission.

## Design Rules

- Use structured row enums for closed UI states. Do not pass `serde_json::Value`
  between TUI state and widgets when both producer and consumer are in
  `coco-rs`.
- Keep render functions pure. No filesystem, process, config, or network side
  effects in widgets.
- Keep render functions deterministic from `AppState` plus frame area. Feature,
  settings, and environment-derived display decisions are resolved before
  render.
- Keep update functions responsible for behavior. Renderers should not decide
  what Enter does.
- Keep process and filesystem side effects behind narrow services or runner
  commands. Update may dispatch intent, but widgets must never perform effects.
- Keep transient surface key routing explicit. Local overlays and composer
  state handle local cancellation before the parent handles global
  interrupt/quit.
- Keep TS parity visible. Every migrated interactive surface should name the
  TS source component/command it mirrors and document any intentional
  divergence.
- Measure before rendering for complex surfaces. Do not infer multi-line height
  with ad hoc string splitting.
- Guard against zero-width wrap and stale glyphs after resize.
- Keep terminal-specific behavior centralized in terminal/layout helpers.
- Keep provider/model data structured. No model-id prefix inference in
  production UI.
- Prefer `ModelRole` for role selection. Do not add display-only model fields
  that bypass runtime config.
- Use i18n keys for labels and hints.
- Use snapshots for visible UI changes.
- Keep module files below the local size targets; split widgets and
  presentation modules before they become dumping grounds.
- Treat `codex-rs/tui` as a source of tested terminal mechanics, not as an
  ownership template. Avoid rebuilding a monolithic chat widget in `coco-rs`.
- `UiStyles` is the only coupling point between presentation code and the
  theme palette. No widget or view model reads `theme.X` directly.
- Preserve `Overlay::priority` tiers and `MAX_OVERLAY_QUEUE` overflow rules
  through every migration. The surface stack adds focus routing; it does
  not relax the existing priority guarantees.
- Mouse capture stays off. The terminal owns mouse events so native
  select-and-copy works; copy flows go through `Ctrl+O` / `/copy` and the
  `clipboard_copy` service.
- OSC 8 hyperlinks are deferred until ratatui provides a native span
  primitive. Do not embed raw escape sequences in spans.
- Live-tail rendering of the streaming cell goes through
  `ActiveCellTranscriptKey`; consumers must memoize on the key, not on the
  rendered `Vec<Line>`.

## Verification

Minimum verification for each phase:

- Unit tests for view model row construction and selection behavior.
- Unit tests or fixtures that compare migrated surfaces against the TS
  interaction inventory: keys, default focus, cancel/result text, loading/empty
  rows, disabled rows, and narrow-terminal behavior.
- A checked-in command parity audit that accounts for every TS command name as
  implemented, alias/compat-message, backend unsupported, or intentionally
  omitted.
- Unit tests for current-model preservation when the selected binding is absent
  from the catalog, typed unavailable summaries, memory payload adaptation, and
  external-editor command parsing.
- Unit tests for actual-index picker selection across filtering, disabled-row
  skipping, terminal keybinding fallback, footer collapse priority, and
  positive-width layout guards.
- Unit tests for Select-style input rows, multi-select submit behavior, edge
  callbacks, full-width digit handling, and FuzzyPicker direction/up-mode
  navigation.
- Unit tests for permission feedback input: Tab toggles input mode, empty
  feedback collapses on focus change, submitted feedback trims whitespace, and
  Escape/Ctrl-C stay local to confirmation.
- Unit tests for request-user-input / AskUserQuestion multi-question flow,
  preview state, multi-select rows, free-text rows, final review, and
  cancellation.
- Unit tests for prompt paste id seeding on resumed transcripts, attachment
  chip deletion, queued command editing, stash restore, and suggestion
  submit-blocking.
- `insta` snapshots for model picker, memory picker, input, and activity
  surfaces.
- `insta` snapshots for narrow-width picker side-content fallback, pager footer,
  live transcript tail, history search, paste placeholders, and footer collapse.
- `insta` snapshots for theme preview/cancel, output-style loading/fallback,
  permission prompts with feedback, long plan approval sticky footer, structured
  diffs, quick open/global search previews, and command result messages.
- Snapshot widths should include at least narrow, normal, and wide terminals so
  compact hints and side-preview fallback are exercised.
- Theme smoke snapshots for at least default, light, ANSI, and daltonized modes
  when a component uses selected/current/disabled styles.
- `just quick-check` from `coco-rs/` after Rust changes.

Final pre-commit gate remains `just pre-commit`, run once at the end of the
commit.
