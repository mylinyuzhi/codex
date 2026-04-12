//! Terminal setup and management.
//!
//! Provides terminal initialization/restoration and the [`Tui`] wrapper
//! that manages the ratatui terminal instance.

use std::io::IsTerminal;
use std::io::Stdout;
use std::io::{self};
use std::panic;

use crossterm::event::DisableBracketedPaste;
use crossterm::event::DisableFocusChange;
use crossterm::event::EnableBracketedPaste;
use crossterm::event::EnableFocusChange;
use crossterm::execute;
use crossterm::terminal::EnterAlternateScreen;
use crossterm::terminal::LeaveAlternateScreen;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

/// Type alias for the terminal backend.
pub type TerminalBackend = CrosstermBackend<Stdout>;

/// Type alias for the ratatui terminal.
pub type RatatuiTerminal = Terminal<TerminalBackend>;

/// Set up the terminal for TUI mode.
///
/// Enables raw mode, enters alternate screen, enables bracketed paste
/// and focus change events. Installs a panic hook that restores the
/// terminal on crash.
pub fn setup_terminal() -> io::Result<RatatuiTerminal> {
    if !io::stdin().is_terminal() {
        return Err(io::Error::other("stdin is not a terminal"));
    }
    if !io::stdout().is_terminal() {
        return Err(io::Error::other("stdout is not a terminal"));
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableBracketedPaste,
        EnableFocusChange,
    )?;

    set_panic_hook();

    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend)
}

/// Restore the terminal to its original state.
pub fn restore_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(
        io::stdout(),
        DisableBracketedPaste,
        DisableFocusChange,
        LeaveAlternateScreen,
    )?;
    Ok(())
}

/// Install a panic hook that restores the terminal before printing the panic.
fn set_panic_hook() {
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        let _ = restore_terminal();
        original_hook(panic_info);
    }));
}

/// TUI manager wrapping the ratatui terminal.
pub struct Tui {
    terminal: RatatuiTerminal,
}

impl Tui {
    /// Create a new Tui with a fresh terminal.
    pub fn new() -> io::Result<Self> {
        let terminal = setup_terminal()?;
        Ok(Self { terminal })
    }

    /// Create from an existing terminal (for testing).
    pub fn with_terminal(terminal: RatatuiTerminal) -> Self {
        Self { terminal }
    }

    /// Draw a frame.
    pub fn draw<F>(&mut self, f: F) -> io::Result<()>
    where
        F: FnOnce(&mut ratatui::Frame),
    {
        self.terminal.draw(f)?;
        Ok(())
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
