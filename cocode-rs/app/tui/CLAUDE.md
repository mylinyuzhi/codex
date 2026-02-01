# CLAUDE.md - cocode-tui Development Guide

This file provides guidance for developing the TUI (Terminal User Interface) for cocode-rs.

## Architecture Overview

The TUI follows **The Elm Architecture (TEA)** pattern with async event handling:

```
┌─────────────────────────────────────────────────────────────────┐
│                         TUI Layer                                │
│                                                                  │
│  ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐  │
│  │  Model   │    │  Update  │    │   View   │    │  Events  │  │
│  │ (State)  │◄───│(Messages)│◄───│(Widgets) │◄───│(Crossterm)│  │
│  └──────────┘    └──────────┘    └──────────┘    └──────────┘  │
│       │                                               ▲         │
│       └───────────────────────────────────────────────┘         │
│                     (render triggers redraw)                     │
└─────────────────────────────────────────────────────────────────┘
```

### Core Components

1. **Model** - Application state (immutable updates)
2. **Message** - Events that trigger state changes
3. **Update** - Pure functions: `(Model, Message) -> Model`
4. **View** - Pure functions: `Model -> Frame` (immediate mode rendering)

## Styling Conventions (from AGENTS.md)

### Use Stylize Helpers

```rust
// GOOD - use Stylize trait helpers
use ratatui::style::Stylize;

"text".dim()
"text".bold()
"text".cyan()
"text".italic()
"text".underlined()
url.cyan().underlined()  // chaining

// BAD - avoid manual Style construction
Span::styled("text", Style::default().fg(Color::Cyan))
```

### Simple Conversions

```rust
// GOOD - simple .into() when type is obvious
"text".into()           // for Span
vec![span1, span2].into()  // for Line

// Use explicit when type is ambiguous
Line::from(spans)
Span::from(text)
```

### Color Rules

```rust
// NEVER use .white() - breaks themes
"text".white()  // BAD

// Use default foreground (no color) instead
"text".into()   // GOOD - uses terminal default
```

### Styling Examples

```rust
// File status indicators
vec!["  └ ".into(), "M".red(), " ".dim(), "tui/src/app.rs".dim()]

// Computed styles (runtime) - Span::styled is OK
let style = compute_style(state);
Span::styled(text, style)
// OR
Span::from(text).set_style(style)
```

### Compactness Rule

Prefer the form that stays on one line after rustfmt:
- If `Line::from(vec![…])` avoids wrapping, use it
- If `vec![…].into()` avoids wrapping, use it
- If both wrap, pick the one with fewer wrapped lines

## Text Wrapping

```rust
// Plain strings: use textwrap::wrap
let wrapped = textwrap::wrap(&text, width);

// Ratatui Lines: use wrapping helpers
use crate::wrapping::{word_wrap_lines, word_wrap_line};

// Indentation: use RtOptions
textwrap::Options::new(width)
    .initial_indent("  ")
    .subsequent_indent("    ")
```

## Async Event Handling

### Event Stream Setup

```rust
use crossterm::event::{EventStream, KeyEventKind};
use tokio_stream::StreamExt;

// Filter for KeyPress only (cross-platform)
if key.kind == KeyEventKind::Press {
    tx.send(Event::Key(key)).unwrap();
}
```

### Event Types

```rust
pub enum Event {
    // Terminal events
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),

    // Internal events
    Tick,           // Animation/status updates
    Render,         // Frame request

    // Agent events
    Agent(LoopEvent),
}
```

### Main Loop Pattern with `tokio::select!`

```rust
let mut tick_interval = tokio::time::interval(Duration::from_millis(250));
let mut render_interval = tokio::time::interval(Duration::from_millis(16)); // ~60 FPS
let mut event_stream = EventStream::new();

loop {
    tokio::select! {
        _ = tick_interval.tick() => {
            tx.send(Event::Tick)?;
        }
        _ = render_interval.tick() => {
            tx.send(Event::Render)?;
        }
        Some(Ok(event)) = event_stream.next() => {
            match event {
                CrosstermEvent::Key(key) if key.kind == KeyEventKind::Press => {
                    tx.send(Event::Key(key))?;
                }
                CrosstermEvent::Resize(w, h) => {
                    tx.send(Event::Resize(w, h))?;
                }
                _ => {}
            }
        }
        Some(loop_event) = agent_rx.recv() => {
            tx.send(Event::Agent(loop_event))?;
        }
    }
}
```

## Terminal Management

### Setup and Teardown

```rust
pub fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableBracketedPaste,
        EnableFocusChange,
    )?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend)
}

pub fn restore_terminal() -> Result<()> {
    disable_raw_mode()?;
    execute!(
        io::stdout(),
        LeaveAlternateScreen,
        DisableBracketedPaste,
        DisableFocusChange,
    )?;
    Ok(())
}

// Implement Drop for automatic cleanup
impl Drop for Tui {
    fn drop(&mut self) {
        let _ = restore_terminal();
    }
}
```

### Panic Handler

```rust
// Install panic hook to restore terminal on crash
let original_hook = std::panic::take_hook();
std::panic::set_hook(Box::new(move |panic| {
    let _ = restore_terminal();
    original_hook(panic);
}));
```

## Key Keyboard Shortcuts

Priority shortcuts to implement:

| Key | Action | Message |
|-----|--------|---------|
| **Tab** | Toggle plan mode | `Message::TogglePlanMode` |
| **Ctrl+T** | Cycle thinking level | `Message::CycleThinkingLevel` |
| **Ctrl+M** | Cycle/switch model | `Message::CycleModel` |
| **Ctrl+C** | Interrupt/Exit | `Message::Interrupt` |
| **Ctrl+L** | Clear/redraw screen | `Message::ClearScreen` |
| **Escape** | Cancel/close overlay | `Message::Cancel` |

### Thinking Level Cycling

```rust
fn cycle_thinking_level(current: &ThinkingLevel) -> ThinkingLevel {
    use ReasoningEffort::*;
    let next = match current.effort {
        None => Low,
        Minimal => Low,
        Low => Medium,
        Medium => High,
        High => XHigh,
        XHigh => None,
    };
    ThinkingLevel::new(next)
}
```

## State Management

### Model Structure

```rust
pub struct Model {
    // Session state (from agent)
    pub session: SessionState,

    // UI state
    pub ui: UiState,

    // Running state
    pub running: RunningState,
}

pub struct SessionState {
    pub messages: Vec<ChatMessage>,
    pub current_model: String,
    pub thinking_level: ThinkingLevel,
    pub plan_mode: bool,
    pub plan_file: Option<PathBuf>,
    pub token_usage: TokenUsage,
}

pub struct UiState {
    pub input: String,
    pub scroll_offset: i32,
    pub focus: Focus,
    pub overlay: Option<Overlay>,
    pub streaming: Option<StreamingState>,
}

#[derive(Default, PartialEq, Eq)]
pub enum RunningState {
    #[default]
    Running,
    Done,
}
```

## Widget Development

### Widget Trait Implementation

```rust
impl Widget for MyWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Use Block for borders
        let block = Block::default()
            .title("Title".bold())
            .borders(Borders::ALL)
            .border_style(Style::default().dim());

        let inner = block.inner(area);
        block.render(area, buf);

        // Render content in inner area
        // ...
    }
}
```

### StatefulWidget for Interactive Elements

```rust
impl StatefulWidget for MyStatefulWidget {
    type State = MyWidgetState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        // Access and modify state during render
        // Useful for scroll position, selection, etc.
    }
}
```

## Testing

### Snapshot Tests with Insta

```rust
#[test]
fn test_widget_render() {
    let widget = MyWidget::new(/* ... */);
    let area = Rect::new(0, 0, 80, 24);
    let mut buf = Buffer::empty(area);

    widget.render(area, &mut buf);

    insta::assert_snapshot!(buffer_to_string(&buf));
}
```

### Running Tests

```bash
# Run TUI tests
cargo test -p cocode-tui

# Check pending snapshots
cargo insta pending-snapshots -p cocode-tui

# Accept snapshots after review
cargo insta accept -p cocode-tui
```

## Development Workflow

```bash
# From codex/ directory (NOT cocode-rs/)

# Quick check
cargo check -p cocode-tui --manifest-path cocode-rs/Cargo.toml

# Format (auto, no approval needed)
cargo fmt --manifest-path cocode-rs/Cargo.toml

# Test
cargo test -p cocode-tui --manifest-path cocode-rs/Cargo.toml

# Lint fix (ask user first)
cargo clippy -p cocode-tui --manifest-path cocode-rs/Cargo.toml --fix

# Pre-commit (REQUIRED)
cargo build --manifest-path cocode-rs/Cargo.toml
```

## References

### Official Documentation
- [Ratatui Documentation](https://ratatui.rs/)
- [The Elm Architecture (TEA)](https://ratatui.rs/concepts/application-patterns/the-elm-architecture/)
- [Async Event Handling](https://ratatui.rs/tutorials/counter-async-app/async-event-stream/)
- [Event Handling Patterns](https://ratatui.rs/concepts/event-handling/)

### Templates
- [Async Template](https://github.com/ratatui/async-template) - Tokio + Crossterm async pattern
- [Component Template](https://ratatui.rs/templates/component/tui-rs/) - Component-based architecture

### Related Crates
- [awesome-ratatui](https://github.com/ratatui/awesome-ratatui) - Curated TUI examples
- codex-rs/tui - Reference implementation in this repo

## Code Conventions

### DO
- Use `i32`/`i64` (never `u32`/`u64`)
- Inline format args: `format!("{var}")` not `format!("{}", var)`
- Use `.into()` for simple conversions
- Chain Stylize helpers for readability
- Implement `Drop` for cleanup
- Filter `KeyEventKind::Press` for cross-platform compatibility

### DON'T
- Use `.unwrap()` in non-test code (use `?` or `.expect("reason")`)
- Use `.white()` in any styling (breaks themes)
- Block the render loop waiting for input
- Conflate key handling with state updates in draw loop
- Use `cd cocode-rs/` (stay in `codex/` directory)
