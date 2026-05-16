//! Terminal setup and management.
//!
//! Provides terminal initialization/restoration and the [`Tui`] wrapper
//! that manages the ratatui terminal instance.

use std::io::IsTerminal;
use std::io::Stdout;
use std::io::Write;
use std::io::{self};
use std::panic;
use std::sync::OnceLock;

use crossterm::cursor::Hide;
use crossterm::cursor::MoveTo;
use crossterm::cursor::Show;
use crossterm::event::DisableBracketedPaste;
use crossterm::event::DisableFocusChange;
use crossterm::event::EnableBracketedPaste;
use crossterm::event::EnableFocusChange;
use crossterm::execute;
use crossterm::queue;
use crossterm::terminal::EnterAlternateScreen;
use crossterm::terminal::LeaveAlternateScreen;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::cursor::CursorClaim;
use crate::job_control::SuspendContext;

/// Type alias for the terminal backend.
pub type TerminalBackend = CrosstermBackend<Stdout>;

/// Type alias for the ratatui terminal.
pub type RatatuiTerminal = Terminal<TerminalBackend>;

/// Enable the TUI-private terminal modes (raw mode, alt-screen,
/// bracketed paste, focus-change reporting).
///
/// Shared by [`setup_terminal`] (initial install) and
/// [`crate::job_control::SuspendContext::suspend`] (re-arm after SIGCONT).
/// Idempotent at the terminal level: re-issuing the same escape sequences
/// while already in raw mode is a no-op.
pub(crate) fn enter_tui_modes(stdout: &mut Stdout) -> io::Result<()> {
    enable_raw_mode()?;
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableBracketedPaste,
        EnableFocusChange,
    )?;
    Ok(())
}

/// Disable the TUI-private terminal modes. Mirror image of
/// [`enter_tui_modes`].
pub(crate) fn leave_tui_modes() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(
        io::stdout(),
        LeaveAlternateScreen,
        DisableBracketedPaste,
        DisableFocusChange,
    )?;
    Ok(())
}

/// Set up the terminal for TUI mode.
///
/// Enables alt-screen, raw mode, bracketed paste, and focus-change
/// reporting. Uses ratatui's default [`Viewport::Fullscreen`] so the
/// whole alt-screen is the canvas — same model as TS Claude Code via
/// Ink.
///
/// Panic hook install is idempotent across repeated [`setup_terminal`]
/// calls (e.g. tests that build and drop multiple Tui instances).
pub fn setup_terminal() -> io::Result<RatatuiTerminal> {
    if !io::stdin().is_terminal() {
        return Err(io::Error::other("stdin is not a terminal"));
    }
    if !io::stdout().is_terminal() {
        return Err(io::Error::other("stdout is not a terminal"));
    }

    let mut stdout = io::stdout();
    enter_tui_modes(&mut stdout)?;

    install_panic_hook_once();

    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend)
}

/// Restore the terminal to its original state — leaves alt-screen and
/// disables the modes [`enter_tui_modes`] installed.
pub fn restore_terminal() -> io::Result<()> {
    leave_tui_modes()?;
    Ok(())
}

/// Install the panic hook exactly once across the lifetime of the
/// process. `setup_terminal` may be called multiple times (e.g. tests
/// that build a `Tui` then drop it), but `panic::take_hook` is global
/// and replacing it twice would chain wrong original handlers.
fn install_panic_hook_once() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let original_hook = panic::take_hook();
        panic::set_hook(Box::new(move |panic_info| {
            let _ = restore_terminal();
            original_hook(panic_info);
        }));
    });
}

/// TUI manager wrapping the ratatui terminal.
pub struct Tui {
    terminal: RatatuiTerminal,
    suspend_context: SuspendContext,
}

impl Tui {
    /// Create a new Tui with a fresh terminal.
    pub fn new() -> io::Result<Self> {
        let terminal = setup_terminal()?;
        Ok(Self {
            terminal,
            suspend_context: SuspendContext::new(),
        })
    }

    /// Create from an existing terminal (for testing).
    pub fn with_terminal(terminal: RatatuiTerminal) -> Self {
        Self {
            terminal,
            suspend_context: SuspendContext::new(),
        }
    }

    /// Draw a frame to the alt-screen.
    ///
    /// The render closure returns [`Option<CursorClaim>`]: where (and
    /// how) the cursor should land at the end of this frame. We collect
    /// it inside the `terminal.draw` body, let ratatui flush its own
    /// pass (which emits `Hide` because we never call
    /// `frame.set_cursor_position`), and then post-draw we override:
    ///
    /// - `Some(claim)` → `queue!(SetCursorStyle, MoveTo, Show)` so the
    ///   cursor lands at the exact column/row with the requested shape.
    /// - `None`        → `queue!(Hide, MoveTo(0, 0))` so the cursor has
    ///   a defined home; otherwise terminals like iTerm2 / Terminal.app
    ///   re-show it at the last write position (status bar end) on
    ///   focus-gained.
    ///
    /// Before painting, applies any pending [`crate::job_control::PreparedResumeAction`]
    /// left by a prior `Ctrl+Z → fg` cycle so the terminal modes are
    /// re-armed before the render closure runs.
    pub fn draw<F>(&mut self, render_fn: F) -> io::Result<()>
    where
        F: FnOnce(&mut ratatui::Frame) -> Option<CursorClaim>,
    {
        if let Some(prepared) = self.suspend_context.prepare_resume_action() {
            prepared.apply(&mut self.terminal)?;
        }

        let mut claim: Option<CursorClaim> = None;
        self.terminal.draw(|frame| {
            claim = render_fn(frame);
        })?;

        let backend = self.terminal.backend_mut();
        match claim {
            Some(c) => {
                queue!(backend, c.style, MoveTo(c.position.x, c.position.y), Show,)?;
            }
            None => {
                queue!(backend, Hide, MoveTo(0, 0))?;
            }
        }
        backend.flush()
    }

    /// Initiate the Ctrl+Z suspend dance. Blocks until SIGCONT delivered
    /// (typically by `fg` in the parent shell), at which point we
    /// re-arm TUI modes and a [`PreparedResumeAction`] is queued for the
    /// next [`draw`].
    ///
    /// No-op on non-Unix platforms.
    pub fn trigger_suspend(&self) -> io::Result<()> {
        self.suspend_context.suspend()
    }

    /// Clear the terminal.
    pub fn clear(&mut self) -> io::Result<()> {
        self.terminal.clear()
    }

    /// Get terminal size.
    pub fn size(&self) -> io::Result<ratatui::layout::Size> {
        self.terminal.size()
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = restore_terminal();
    }
}
