//! Terminal setup and management.
//!
//! Provides terminal initialization/restoration and the [`Tui`] wrapper
//! that manages the native scrollback terminal surface.

use std::fmt;
use std::io::IsTerminal;
use std::io::Stdout;
use std::io::Write;
use std::io::{self};
use std::panic;
use std::sync::OnceLock;

use crossterm::Command;
use crossterm::cursor::MoveToNextLine;
use crossterm::cursor::Show;
use crossterm::event::DisableBracketedPaste;
use crossterm::event::DisableFocusChange;
use crossterm::event::EnableBracketedPaste;
use crossterm::event::EnableFocusChange;
use crossterm::execute;
use crossterm::terminal::EnterAlternateScreen;
use crossterm::terminal::LeaveAlternateScreen;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;
use ratatui::backend::Backend;
use ratatui::backend::ClearType;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::layout::Size;

use crate::FrameLayout;
use crate::job_control::SuspendContext;
use crate::state::AppState;
use crate::surface::controller::NativeSurfaceController;
use crate::surface::modal::ModalSurfacePlacement;
use crate::surface::modal::ModalSurfaceState;
use crate::surface::modal::SurfaceFramePlan;
use crate::surface::viewport::interactive_viewport_desired_height;
use coco_tui_ui::engine::compatibility::TerminalCompatibility;
use coco_tui_ui::engine::terminal::SurfaceTerminal;

/// Type alias for the terminal backend.
pub type TerminalBackend = CrosstermBackend<Stdout>;

/// Type alias for the native surface terminal.
pub(crate) type NativeTerminal = SurfaceTerminal<TerminalBackend>;

pub(crate) const NATIVE_VIEWPORT_MIN_HEIGHT: u16 = 4;
pub(crate) const NATIVE_VIEWPORT_MAX_HEIGHT: u16 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EnableAlternateScroll;

impl Command for EnableAlternateScroll {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[?1007h")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Err(io::Error::other(
            "tried to execute EnableAlternateScroll using WinAPI; use ANSI instead",
        ))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DisableAlternateScroll;

impl Command for DisableAlternateScroll {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[?1007l")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Err(io::Error::other(
            "tried to execute DisableAlternateScroll using WinAPI; use ANSI instead",
        ))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone)]
pub struct TuiDrawOutcome {
    pub layout: FrameLayout,
    pub retained_surface_visible: bool,
    pub attention_requested: bool,
}

/// Enable the TUI-private terminal modes (raw mode, bracketed paste, and
/// focus-change reporting).
///
/// Shared by [`setup_terminal`] (initial install) and
/// [`crate::job_control::SuspendContext::suspend`] (re-arm after SIGCONT).
/// Idempotent at the terminal level: re-issuing the same escape sequences
/// while already in raw mode is a no-op.
pub(crate) fn enter_tui_modes(stdout: &mut Stdout) -> io::Result<()> {
    enable_raw_mode()?;
    execute!(stdout, EnableBracketedPaste, EnableFocusChange)?;
    Ok(())
}

/// Disable TUI-private terminal modes and leave alt-screen if an state had
/// entered it. `LeaveAlternateScreen` is intentionally idempotent here so panic
/// cleanup and suspend/external-process paths share one terminal reset.
pub(crate) fn leave_tui_modes() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(
        io::stdout(),
        DisableAlternateScroll,
        LeaveAlternateScreen,
        DisableBracketedPaste,
        DisableFocusChange,
    )?;
    Ok(())
}

/// Set up the terminal for TUI mode.
///
/// Enables raw mode, bracketed paste, and focus-change reporting. The normal
/// surface stays in the main terminal buffer so finalized history can be
/// inserted into native scrollback. Alt-screen is entered only for state
/// surfaces that explicitly request it.
///
/// Panic hook install is idempotent across repeated [`setup_terminal`]
/// calls (e.g. tests that build and drop multiple Tui instances).
pub(crate) fn setup_terminal() -> io::Result<NativeTerminal> {
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
    SurfaceTerminal::new(backend)
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
            // A panic inside a `PanicRestoreGuard` region (e.g. a contained
            // mermaid-layout panic that the caller `catch_unwind`s and recovers
            // from) must NOT tear down the terminal or print a backtrace — that
            // would corrupt the live render for a fully-recovered panic. Still
            // record it on the (off-screen) trace sink so a swallowed upstream
            // bug stays diagnosable.
            if coco_tui_ui::panic_guard::suppress_panic_restore() {
                tracing::warn!(
                    target: "tui::panic_guard",
                    panic = %panic_info,
                    "contained panic in catch_unwind region — recovering, terminal left intact"
                );
                return;
            }
            let _ = restore_terminal();
            original_hook(panic_info);
        }));
    });
}

/// TUI manager wrapping the native scrollback terminal surface.
pub struct Tui {
    terminal: NativeTerminal,
    surface: NativeSurfaceController,
    modal_surface: ModalSurfaceState,
    suspend_context: SuspendContext,
    compatibility: TerminalCompatibility,
    alt_screen_active: bool,
    alt_saved_viewport: Option<Rect>,
}

impl Tui {
    /// Create a new Tui with a fresh terminal.
    pub fn new() -> io::Result<Self> {
        let terminal = setup_terminal()?;
        let compatibility = TerminalCompatibility::detect();
        Ok(Self {
            terminal,
            surface: NativeSurfaceController::default(),
            modal_surface: ModalSurfaceState::default(),
            suspend_context: SuspendContext::new(),
            compatibility,
            alt_screen_active: false,
            alt_saved_viewport: None,
        })
    }

    pub(crate) fn native_scrollback_status_message(&self) -> Option<&'static str> {
        self.compatibility.status_message()
    }

    pub(crate) fn retained_surface_visible(&self) -> bool {
        !self.alt_screen_active
    }

    /// Draw one native surface frame.
    pub fn draw(&mut self, state: &AppState) -> io::Result<TuiDrawOutcome> {
        self.draw_with_frame_index(state, 0)
    }

    pub(crate) fn draw_with_frame_index(
        &mut self,
        state: &AppState,
        frame_index: u64,
    ) -> io::Result<TuiDrawOutcome> {
        let perf_config = state.ui.display_settings.performance;
        self.terminal.set_perf_stats_enabled(perf_config.enabled);
        if let Some(prepared) = self.suspend_context.prepare_resume_action() {
            prepared.apply(|| self.clear_surface_after_resume())?;
        }

        let size = self.terminal.size()?;
        self.terminal.sync_screen_size(size);
        let plan = self.modal_surface.plan_for_native_viewport(
            state,
            self.compatibility,
            std::time::Instant::now(),
            size.width,
            NATIVE_VIEWPORT_MAX_HEIGHT,
        );
        // Build the interactive live tail exactly once per frame. The sizing
        // pass (`sync_surface_area` → `interactive_viewport_desired_height`)
        // and the paint pass (`render_live_viewport`) both consume it, so we
        // compute it here and thread it through instead of rebuilding twice.
        let live_start = perf_config.enabled.then(std::time::Instant::now);
        let live =
            (size.width > 0).then(|| self.surface.prepare_live_tail(state, size.width, plan));
        let live_elapsed = live_start.map(|start| start.elapsed());
        if let Some(elapsed) = live_elapsed
            && crate::perf::should_log_stage(perf_config, frame_index, elapsed)
        {
            tracing::debug!(
                target: crate::perf::TARGET,
                frame_index,
                stage = "build_live_tail_lines",
                duration_us = crate::perf::duration_us(elapsed),
                lines = live.as_ref().map_or(0, Vec::len),
                width = size.width,
                "tui frame stage completed",
            );
        }
        let live_height = live.as_ref().map(|lines| lines.len() as u16);
        // Pass the size read above so the precomputed live tail (built at
        // `size.width`) and the viewport area are derived from one consistent
        // size, even if the terminal resizes mid-frame.
        let sync_start = perf_config.enabled.then(std::time::Instant::now);
        self.sync_surface_area(state, plan, size, live_height)?;
        let sync_elapsed = sync_start.map(|start| start.elapsed());
        if let Some(elapsed) = sync_elapsed
            && crate::perf::should_log_stage(perf_config, frame_index, elapsed)
        {
            tracing::debug!(
                target: crate::perf::TARGET,
                frame_index,
                stage = "sync_surface_area",
                duration_us = crate::perf::duration_us(elapsed),
                width = size.width,
                height = size.height,
                viewport = ?self.terminal.viewport_area(),
                "tui frame stage completed",
            );
        }
        let surface_start = perf_config.enabled.then(std::time::Instant::now);
        let outcome = self.surface.draw_with_plan_at_frame(
            &mut self.terminal,
            state,
            plan,
            live,
            frame_index,
        )?;
        let surface_elapsed = surface_start.map(|start| start.elapsed());
        if let Some(elapsed) = surface_elapsed
            && crate::perf::should_log_stage(perf_config, frame_index, elapsed)
        {
            tracing::debug!(
                target: crate::perf::TARGET,
                frame_index,
                stage = "native_surface_draw",
                duration_us = crate::perf::duration_us(elapsed),
                viewport_updates = self.terminal.last_viewport_draw_stats().buffer_updates,
                history_rows = self.terminal.last_history_insert_stats().wrapped_rows,
                "tui frame stage completed",
            );
        }
        Ok(TuiDrawOutcome {
            layout: outcome.layout,
            retained_surface_visible: self.retained_surface_visible(),
            attention_requested: plan.attention_requested,
        })
    }

    /// Initiate the Ctrl+Z suspend dance. Blocks until SIGCONT delivered
    /// (typically by `fg` in the parent shell), at which point we
    /// re-arm TUI modes and a [`PreparedResumeAction`] is queued for the
    /// next [`draw`].
    ///
    /// No-op on non-Unix platforms.
    pub fn trigger_suspend(&mut self) -> io::Result<()> {
        self.leave_modal_alt_screen()?;
        self.suspend_context.suspend()?;
        Ok(())
    }

    /// Leave TUI-private terminal modes before running an interactive
    /// child process such as `$EDITOR`.
    pub fn prepare_external_process(&mut self) -> io::Result<()> {
        let mut stdout = io::stdout();
        self.leave_modal_alt_screen()?;
        leave_tui_modes()?;
        if let Err(err) = execute!(stdout, MoveToNextLine(1), Show) {
            let _ = enter_tui_modes(&mut stdout);
            return Err(err);
        }
        stdout.flush()
    }

    /// Re-enter TUI modes after an external process exits and force the
    /// next frame to repaint the native surface.
    pub fn restore_after_external_process(&mut self) -> io::Result<()> {
        let mut stdout = io::stdout();
        enter_tui_modes(&mut stdout)?;
        self.leave_modal_alt_screen()?;
        self.clear_surface_after_resume()
    }

    /// Clear the terminal.
    pub fn clear(&mut self) -> io::Result<()> {
        self.terminal.clear_owned_scrollback()?;
        self.surface.reset();
        Ok(())
    }

    /// Get terminal size.
    pub fn size(&self) -> io::Result<ratatui::layout::Size> {
        self.terminal.size()
    }

    fn clear_surface_after_resume(&mut self) -> io::Result<()> {
        self.terminal.clear_owned_scrollback()?;
        self.surface.reset();
        Ok(())
    }

    fn prepare_shell_prompt_after_exit(&mut self) -> io::Result<()> {
        self.leave_modal_alt_screen()?;
        self.terminal.prepare_shell_prompt_after_exit()?;
        std::io::Write::flush(self.terminal.backend_mut())
    }

    fn sync_surface_area(
        &mut self,
        state: &AppState,
        plan: SurfaceFramePlan,
        size: ratatui::layout::Size,
        precomputed_live_height: Option<u16>,
    ) -> io::Result<()> {
        let wants_alt = matches!(plan.modal_placement, Some(ModalSurfacePlacement::AltScreen));

        if wants_alt && !self.alt_screen_active {
            self.alt_saved_viewport = Some(self.terminal.viewport_area());
            execute!(
                self.terminal.backend_mut(),
                EnterAlternateScreen,
                EnableAlternateScroll
            )?;
            self.alt_screen_active = true;
            self.terminal.backend_mut().clear_region(ClearType::All)?;
            self.terminal.invalidate_viewport();
        } else if !wants_alt && self.alt_screen_active {
            self.leave_modal_alt_screen()?;
        }

        let area = if self.alt_screen_active {
            Rect::new(0, 0, size.width, size.height)
        } else {
            let desired_height = interactive_viewport_desired_height(
                state,
                size.width,
                NATIVE_VIEWPORT_MAX_HEIGHT,
                plan,
                precomputed_live_height,
            );
            native_viewport_area_with_max(
                self.terminal.history_bottom_y(),
                size,
                desired_height,
                NATIVE_VIEWPORT_MAX_HEIGHT,
            )
        };
        if self.terminal.viewport_area() != area {
            tracing::debug!(
                target: "tui::surface",
                previous = ?self.terminal.viewport_area(),
                next = ?area,
                viewport_height = area.height,
                history_bottom_y = self.terminal.history_bottom_y(),
                alt_screen_active = self.alt_screen_active,
                "sync surface area"
            );
            self.terminal
                .apply_viewport_area(area, !self.alt_screen_active)?;
        }
        Ok(())
    }

    fn leave_modal_alt_screen(&mut self) -> io::Result<()> {
        if self.alt_screen_active {
            execute!(
                self.terminal.backend_mut(),
                DisableAlternateScroll,
                LeaveAlternateScreen
            )?;
            self.alt_screen_active = false;
        }
        if let Some(saved) = self.alt_saved_viewport.take() {
            self.terminal.set_viewport_area(saved);
            self.terminal.invalidate_viewport();
        }
        Ok(())
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = self.prepare_shell_prompt_after_exit();
        let _ = restore_terminal();
        // zsh shows PROMPT_EOL_MARK (`%`) when the command's final output
        // does not end in a newline. Terminal mode restore emits escape
        // sequences, so the newline must be the last best-effort write.
        let _ = self.terminal.backend_mut().write_all(b"\r\n");
        let _ = std::io::Write::flush(self.terminal.backend_mut());
    }
}

#[cfg(test)]
pub(crate) fn native_viewport_area(anchor_y: u16, size: Size, desired_height: u16) -> Rect {
    native_viewport_area_with_max(anchor_y, size, desired_height, NATIVE_VIEWPORT_MAX_HEIGHT)
}

pub(crate) fn native_viewport_area_with_max(
    anchor_y: u16,
    size: Size,
    desired_height: u16,
    max_height: u16,
) -> Rect {
    if size.height == 0 {
        return Rect::new(0, 0, size.width, 0);
    }
    let height = desired_height
        .clamp(
            NATIVE_VIEWPORT_MIN_HEIGHT,
            max_height.max(NATIVE_VIEWPORT_MIN_HEIGHT),
        )
        .min(size.height);
    let y = anchor_y.min(size.height.saturating_sub(height));
    Rect::new(0, y, size.width, height)
}

#[cfg(test)]
#[path = "terminal.test.rs"]
mod tests;
