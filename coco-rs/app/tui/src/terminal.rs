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
use crossterm::terminal::LeaveAlternateScreen;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;
use ratatui::backend::ClearType;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::layout::Size;

use crate::FrameLayout;
use crate::job_control::SuspendContext;
use crate::state::AppState;
use crate::surface::controller::NativeSurfaceController;
use crate::surface::controller::NativeSurfaceFramePlan;
use crate::surface::modal::ModalSurfacePlacement;
use crate::surface::modal::ModalSurfaceState;
use crate::surface::modal::SurfaceFramePlan;
use crate::surface::viewport::interactive_viewport_desired_height;
use coco_tui_ui::engine::compatibility::TerminalCompatibility;
use coco_tui_ui::engine::terminal::SurfaceBackend;
use coco_tui_ui::engine::terminal::SurfaceTerminal;

/// Type alias for the terminal backend.
pub type TerminalBackend = CrosstermBackend<Stdout>;

/// Type alias for the native surface terminal.
pub(crate) type NativeTerminal = SurfaceTerminal<TerminalBackend>;

pub(crate) const NATIVE_VIEWPORT_MIN_HEIGHT: u16 = 4;
pub(crate) const NATIVE_VIEWPORT_MAX_HEIGHT: u16 = 12;
/// Max rendered rows the *streaming* live tail may occupy in the inline
/// viewport. Bounding it keeps the per-turn growth phase short (≤ this many
/// repaints, once) and the viewport height constant for the rest of the turn —
/// codex keeps its bottom pane fixed for the same reason. The dropped leading
/// rows are NOT lost: they commit to native scrollback at the next markdown
/// boundary and definitively at finalize. Display-only cap (see
/// `SurfaceStreamDriver::prepare`); the markdown commit boundary is untouched.
pub(crate) const STREAMING_LIVE_TAIL_CAP: u16 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(test)]
struct EnableAlternateScroll;

#[cfg(test)]
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
pub struct Tui<B: SurfaceBackend = TerminalBackend> {
    terminal: SurfaceTerminal<B>,
    surface: NativeSurfaceController,
    modal_surface: ModalSurfaceState,
    suspend_context: SuspendContext,
    compatibility: TerminalCompatibility,
    alt_screen_active: bool,
    alt_saved_viewport: Option<Rect>,
    main_screen_viewport_pin: NativeViewportPin,
    restore_terminal_on_drop: bool,
    #[cfg(test)]
    last_geometry_commit: Option<NativeViewportGeometryCommit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeViewportPin {
    Flowing,
    BottomPinned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeViewportGeometry {
    area: Rect,
    pin: NativeViewportPin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeViewportGeometryCommit {
    previous_viewport: Rect,
    desired_viewport: Rect,
    committed_viewport: Rect,
    shrink_requested_rows: u16,
    shrink_committed_rows: u16,
    reveal_tail_rows: u16,
    append_fill_rows: u16,
    shrink_deferred_rows: u16,
}

impl NativeViewportGeometryCommit {
    fn without_shrink(previous_viewport: Rect, desired_viewport: Rect) -> Self {
        Self {
            previous_viewport,
            desired_viewport,
            committed_viewport: desired_viewport,
            shrink_requested_rows: 0,
            shrink_committed_rows: 0,
            reveal_tail_rows: 0,
            append_fill_rows: 0,
            shrink_deferred_rows: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeViewportCommitInputs {
    pin: NativeViewportPin,
    previous_viewport: Rect,
    desired_viewport: Rect,
    terminal_height: u16,
    history_tail_reveal_rows: u16,
    guaranteed_append_rows: u16,
}

impl Tui<TerminalBackend> {
    /// Create a new Tui with a fresh terminal.
    pub fn new() -> io::Result<Self> {
        let terminal = setup_terminal()?;
        Ok(Self::from_terminal(
            terminal,
            TerminalCompatibility::detect(),
            /*restore_terminal_on_drop*/ true,
        ))
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
}

impl<B> Tui<B>
where
    B: SurfaceBackend,
{
    fn from_terminal(
        terminal: SurfaceTerminal<B>,
        compatibility: TerminalCompatibility,
        restore_terminal_on_drop: bool,
    ) -> Self {
        Self {
            terminal,
            surface: NativeSurfaceController::default(),
            modal_surface: ModalSurfaceState::default(),
            suspend_context: SuspendContext::new(),
            compatibility,
            alt_screen_active: false,
            alt_saved_viewport: None,
            main_screen_viewport_pin: NativeViewportPin::Flowing,
            restore_terminal_on_drop,
            #[cfg(test)]
            last_geometry_commit: None,
        }
    }

    #[cfg(test)]
    fn new_for_test(terminal: SurfaceTerminal<B>, compatibility: TerminalCompatibility) -> Self {
        Self::from_terminal(
            terminal,
            compatibility,
            /*restore_terminal_on_drop*/ false,
        )
    }

    #[cfg(test)]
    fn terminal(&self) -> &SurfaceTerminal<B> {
        &self.terminal
    }

    #[cfg(test)]
    fn last_geometry_commit_for_test(&self) -> Option<NativeViewportGeometryCommit> {
        self.last_geometry_commit
    }

    pub(crate) fn native_scrollback_status_message(&self) -> Option<&'static str> {
        self.compatibility.status_message()
    }

    pub(crate) fn retained_surface_visible(&self) -> bool {
        !self.alt_screen_active
    }

    /// Draw one native surface frame.
    pub fn draw(&mut self, state: &AppState) -> Result<TuiDrawOutcome, B::Error> {
        self.draw_with_frame_index(state, 0)
    }

    pub(crate) fn draw_with_frame_index(
        &mut self,
        state: &AppState,
        frame_index: u64,
    ) -> Result<TuiDrawOutcome, B::Error> {
        let perf_config = state.ui.display_settings.performance;
        self.terminal.set_perf_stats_enabled(perf_config.enabled);
        if self.suspend_context.prepare_resume_action().is_some() {
            self.clear_surface_after_resume()?;
        }

        let size = self.terminal.size()?;
        self.terminal.sync_screen_size(size);
        let now = std::time::Instant::now();
        let plan = self.modal_surface.plan_for_native_viewport(
            state,
            self.compatibility,
            now,
            size.width,
            NATIVE_VIEWPORT_MAX_HEIGHT,
        );
        // Build the interactive live tail exactly once per frame. The sizing
        // pass (`sync_surface_area` → `interactive_viewport_desired_height`)
        // and the paint pass (`render_live_viewport`) both consume it, so we
        // compute it here and thread it through instead of rebuilding twice.
        // This is pure CPU work (no terminal writes) and therefore stays
        // OUTSIDE the synchronized-update window opened below.
        let live_start = perf_config.enabled.then(std::time::Instant::now);
        let native_frame = self
            .surface
            .prepare_native_frame(state, size.width, plan, now);
        let live_elapsed = live_start.map(|start| start.elapsed());
        if let Some(elapsed) = live_elapsed
            && crate::perf::should_log_stage(perf_config, frame_index, elapsed)
        {
            tracing::debug!(
                target: crate::perf::TARGET,
                frame_index,
                stage = "build_live_tail_lines",
                duration_us = crate::perf::duration_us(elapsed),
                lines = native_frame.live_lines.as_ref().map_or(0, Vec::len),
                width = size.width,
                "tui frame stage completed",
            );
        }
        // === One synchronized-update window for the whole paint. ===
        // `?2026h` is emitted BEFORE `sync_surface_area` so the viewport
        // resize/clear/scroll is deferred by the terminal and never presents a
        // blank region between the clear and the repaint (the input-bar
        // flicker). The window brackets the clear, the native history insert,
        // and the viewport draw; the single ESU flush presents the composed
        // frame. ESU is emitted even when the inner draw errors so the terminal
        // never stays stuck in deferred-present.
        self.terminal.begin_synchronized_update()?;
        let drawn = self.draw_native_frame(state, plan, size, native_frame, frame_index);
        let present_start = perf_config.enabled.then(std::time::Instant::now);
        let ended = self.terminal.end_synchronized_update();
        if let Some(start) = present_start {
            let elapsed = start.elapsed();
            if crate::perf::should_log_stage(perf_config, frame_index, elapsed) {
                tracing::debug!(
                    target: crate::perf::TARGET,
                    frame_index,
                    stage = "present_flush",
                    duration_us = crate::perf::duration_us(elapsed),
                    "tui frame stage completed",
                );
            }
        }
        let outcome = match (drawn, ended) {
            (Ok(outcome), Ok(())) => outcome,
            (Err(err), _) | (Ok(_), Err(err)) => return Err(err),
        };
        Ok(TuiDrawOutcome {
            layout: outcome.layout,
            retained_surface_visible: self.retained_surface_visible(),
            attention_requested: plan.attention_requested,
        })
    }

    /// Paint the native surface for one frame: the viewport resize/clear/scroll
    /// (`sync_surface_area`) followed by the history insert + viewport draw.
    ///
    /// Runs entirely inside the caller's synchronized-update window so the clear
    /// and the repaint compose atomically. Returns the surface draw outcome; the
    /// caller always emits ESU regardless of this result. Each stage keeps its
    /// own perf span so the `tui::perf::frame` log breakdown is unchanged.
    fn draw_native_frame(
        &mut self,
        state: &AppState,
        plan: SurfaceFramePlan,
        size: ratatui::layout::Size,
        native_frame: NativeSurfaceFramePlan,
        frame_index: u64,
    ) -> Result<crate::surface::controller::NativeSurfaceDrawOutcome, B::Error> {
        let perf_config = state.ui.display_settings.performance;
        // The live tail is one display row per line, so its length is the
        // precomputed viewport content height for the sizing pass.
        let live_height = native_frame
            .live_lines
            .as_ref()
            .map(|lines| lines.len() as u16);
        // Pass the size read by the caller so the precomputed live tail (built
        // at `size.width`) and the viewport area are derived from one consistent
        // size, even if the terminal resizes mid-frame.
        let sync_start = perf_config.enabled.then(std::time::Instant::now);
        let geometry_commit =
            self.sync_surface_area(state, plan, size, live_height, &native_frame)?;
        #[cfg(test)]
        {
            self.last_geometry_commit = Some(geometry_commit);
        }
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
        if geometry_commit.reveal_tail_rows > 0 {
            let gap_before_fill = self
                .terminal
                .viewport_area()
                .top()
                .saturating_sub(self.terminal.history_bottom_y());
            let filled = self
                .surface
                .fill_history_tail_gap(&mut self.terminal, geometry_commit.reveal_tail_rows)?;
            let remaining_gap_rows = self
                .terminal
                .viewport_area()
                .top()
                .saturating_sub(self.terminal.history_bottom_y());
            let fill_status = if filled > 0 {
                "filled"
            } else if gap_before_fill == 0 {
                "already_aligned_after_viewport_apply"
            } else {
                "no_cached_tail_rows"
            };
            tracing::debug!(
                target: "tui::surface::geometry",
                requested_rows = geometry_commit.reveal_tail_rows,
                gap_before_fill,
                filled_rows = filled,
                remaining_gap_rows,
                fill_status,
                "filled native history tail gap"
            );
        }
        let surface_start = perf_config.enabled.then(std::time::Instant::now);
        let outcome = self.surface.draw_with_plan_at_frame(
            &mut self.terminal,
            state,
            plan,
            native_frame,
            frame_index,
        )?;
        let history_bottom_y_after = self.terminal.history_bottom_y();
        let viewport_top_after = self.terminal.viewport_area().top();
        let unbacked_gap_rows = viewport_top_after.saturating_sub(history_bottom_y_after);
        let flowing_seats_flush = flowing_viewport_seats_flush(
            self.main_screen_viewport_pin,
            viewport_top_after,
            history_bottom_y_after,
        );
        // Invariant (post de-stick): a Flowing viewport seats flush on history.
        // A non-zero gap under a Flowing pin means a stale-anchor / second-writer
        // regression (the /clear-gap class). A BottomPinned viewport may carry a
        // transient backed gap pending tail-reveal (shrink_deferred_rows > 0), so
        // the pin guard is load-bearing — do NOT drop it.
        debug_assert!(
            flowing_seats_flush,
            "flowing viewport must seat flush against history: pin={:?} viewport_top={} \
             history_bottom_y={} unbacked_gap_rows={} committed={:?} reveal_tail_rows={} \
             append_fill_rows={} shrink_deferred_rows={}",
            self.main_screen_viewport_pin,
            viewport_top_after,
            history_bottom_y_after,
            unbacked_gap_rows,
            geometry_commit.committed_viewport,
            geometry_commit.reveal_tail_rows,
            geometry_commit.append_fill_rows,
            geometry_commit.shrink_deferred_rows,
        );
        if !flowing_seats_flush {
            tracing::warn!(
                target: "tui::surface::geometry",
                pin = ?self.main_screen_viewport_pin,
                viewport_top = viewport_top_after,
                history_bottom_y = history_bottom_y_after,
                unbacked_gap_rows,
                committed_viewport = ?geometry_commit.committed_viewport,
                reveal_tail_rows = geometry_commit.reveal_tail_rows,
                append_fill_rows = geometry_commit.append_fill_rows,
                shrink_deferred_rows = geometry_commit.shrink_deferred_rows,
                "flowing viewport is not seated flush against history"
            );
        }
        tracing::debug!(
            target: "tui::surface::geometry",
            pin = ?self.main_screen_viewport_pin,
            previous_viewport = ?geometry_commit.previous_viewport,
            desired_viewport = ?geometry_commit.desired_viewport,
            committed_viewport = ?geometry_commit.committed_viewport,
            terminal_height = size.height,
            history_bottom_y_after,
            shrink_requested_rows = geometry_commit.shrink_requested_rows,
            shrink_committed_rows = geometry_commit.shrink_committed_rows,
            reveal_tail_rows = geometry_commit.reveal_tail_rows,
            append_fill_rows = geometry_commit.append_fill_rows,
            shrink_deferred_rows = geometry_commit.shrink_deferred_rows,
            unbacked_gap_rows,
            input_bottom = outcome.layout.input.bottom(),
            viewport_bottom = self.terminal.viewport_area().bottom(),
            "native surface geometry committed"
        );
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
        Ok(outcome)
    }

    /// Clear the terminal.
    pub fn clear(&mut self) -> Result<(), B::Error> {
        self.terminal.clear_owned_scrollback()?;
        self.surface.reset();
        self.main_screen_viewport_pin = NativeViewportPin::Flowing;
        Ok(())
    }

    /// Get terminal size.
    pub fn size(&self) -> Result<ratatui::layout::Size, B::Error> {
        self.terminal.size()
    }

    fn clear_surface_after_resume(&mut self) -> Result<(), B::Error> {
        self.terminal.clear_owned_scrollback()?;
        self.surface.reset();
        self.main_screen_viewport_pin = NativeViewportPin::Flowing;
        Ok(())
    }

    fn prepare_shell_prompt_after_exit(&mut self) -> Result<(), B::Error> {
        self.leave_modal_alt_screen()?;
        self.terminal.prepare_shell_prompt_after_exit()?;
        self.terminal.backend_mut().flush()
    }

    fn sync_surface_area(
        &mut self,
        state: &AppState,
        plan: SurfaceFramePlan,
        size: ratatui::layout::Size,
        precomputed_live_height: Option<u16>,
        native_frame: &NativeSurfaceFramePlan,
    ) -> Result<NativeViewportGeometryCommit, B::Error> {
        let wants_alt = matches!(plan.modal_placement, Some(ModalSurfacePlacement::AltScreen));

        if wants_alt && !self.alt_screen_active {
            self.alt_saved_viewport = Some(self.terminal.viewport_area());
            self.terminal.backend_mut().enter_modal_alt_screen()?;
            self.alt_screen_active = true;
            self.terminal.backend_mut().clear_region(ClearType::All)?;
            self.terminal.invalidate_viewport();
        } else if !wants_alt && self.alt_screen_active {
            self.leave_modal_alt_screen()?;
        }

        if self.alt_screen_active {
            let previous_viewport = self.terminal.viewport_area();
            let history_bottom_y_before = self.terminal.history_bottom_y();
            let desired_area = Rect::new(0, 0, size.width, size.height);
            let commit =
                NativeViewportGeometryCommit::without_shrink(previous_viewport, desired_area);
            apply_native_viewport_commit(
                &mut self.terminal,
                commit,
                history_bottom_y_before,
                size.height,
                self.alt_screen_active,
                self.main_screen_viewport_pin,
            )?;
            return Ok(commit);
        }

        sync_main_surface_area(
            &mut self.terminal,
            &mut self.main_screen_viewport_pin,
            state,
            plan,
            size,
            precomputed_live_height,
            native_frame,
        )
    }

    fn leave_modal_alt_screen(&mut self) -> Result<(), B::Error> {
        if self.alt_screen_active {
            self.terminal.backend_mut().leave_modal_alt_screen()?;
            self.alt_screen_active = false;
        }
        if let Some(saved) = self.alt_saved_viewport.take() {
            self.terminal.set_viewport_area(saved);
            self.terminal.invalidate_viewport();
        }
        Ok(())
    }
}

fn sync_main_surface_area<B>(
    terminal: &mut SurfaceTerminal<B>,
    main_screen_viewport_pin: &mut NativeViewportPin,
    state: &AppState,
    plan: SurfaceFramePlan,
    size: ratatui::layout::Size,
    precomputed_live_height: Option<u16>,
    native_frame: &NativeSurfaceFramePlan,
) -> Result<NativeViewportGeometryCommit, B::Error>
where
    B: SurfaceBackend,
{
    let previous_viewport = terminal.viewport_area();
    let history_bottom_y_before = terminal.history_bottom_y();
    let max_h = interactive_viewport_max_height(state, size.height);
    let desired_height = interactive_viewport_desired_height(
        state,
        size.width,
        max_h,
        plan,
        precomputed_live_height,
    );
    // Anchor on the OWNED viewport top, not `history_bottom_y()`.
    // history_bottom_y mutates mid-frame (clear/insert/reveal), and this pass
    // runs BEFORE the history emission. The owned viewport top is the previous
    // frame's settled seat; the emission is the single seat-mover.
    let viewport_height = native_viewport_height(size, desired_height, max_h);
    let bottom_pinned_y = size.height.saturating_sub(viewport_height);
    let geometry = native_viewport_geometry_with_max(
        terminal.viewport_area().top(),
        size,
        desired_height,
        max_h,
        terminal.history_backs_row(bottom_pinned_y),
    );
    *main_screen_viewport_pin = geometry.pin;
    let commit = commit_native_viewport_geometry(NativeViewportCommitInputs {
        pin: *main_screen_viewport_pin,
        previous_viewport,
        desired_viewport: geometry.area,
        terminal_height: size.height,
        history_tail_reveal_rows: native_frame.history_tail_reveal_rows,
        guaranteed_append_rows: native_frame.guaranteed_append_rows(),
    });
    apply_native_viewport_commit(
        terminal,
        commit,
        history_bottom_y_before,
        size.height,
        /*alt_screen_active*/ false,
        *main_screen_viewport_pin,
    )?;
    Ok(commit)
}

fn apply_native_viewport_commit<B>(
    terminal: &mut SurfaceTerminal<B>,
    commit: NativeViewportGeometryCommit,
    history_bottom_y_before: u16,
    terminal_height: u16,
    alt_screen_active: bool,
    pin: NativeViewportPin,
) -> Result<(), B::Error>
where
    B: SurfaceBackend,
{
    if terminal.viewport_area() != commit.committed_viewport {
        tracing::debug!(
            target: "tui::surface",
            previous = ?terminal.viewport_area(),
            next = ?commit.committed_viewport,
            desired = ?commit.desired_viewport,
            viewport_height = commit.committed_viewport.height,
            viewport_bottom = commit.committed_viewport.bottom(),
            terminal_height,
            history_bottom_y_before,
            history_bottom_y = terminal.history_bottom_y(),
            alt_screen_active,
            bottom_pinned = commit.committed_viewport.bottom() == terminal_height,
            pin = ?pin,
            "sync surface area"
        );
        terminal.apply_viewport_area(commit.committed_viewport, !alt_screen_active)?;
    }
    Ok(())
}

fn commit_native_viewport_geometry(
    inputs: NativeViewportCommitInputs,
) -> NativeViewportGeometryCommit {
    let NativeViewportCommitInputs {
        pin,
        previous_viewport,
        desired_viewport,
        terminal_height,
        history_tail_reveal_rows,
        guaranteed_append_rows,
    } = inputs;
    let mut committed_viewport = desired_viewport;
    let mut shrink_requested_rows = 0;
    let mut shrink_committed_rows = 0;
    let mut reveal_tail_rows = 0;
    let mut append_fill_rows = 0;

    let bottom_pinned_shrink = pin == NativeViewportPin::BottomPinned
        && previous_viewport.bottom() == terminal_height
        && desired_viewport.bottom() == terminal_height
        && desired_viewport.top() > previous_viewport.top();

    if bottom_pinned_shrink {
        shrink_requested_rows = desired_viewport.top() - previous_viewport.top();
        let backed_rows = history_tail_reveal_rows.saturating_add(guaranteed_append_rows);
        shrink_committed_rows = shrink_requested_rows.min(backed_rows);
        if shrink_committed_rows < shrink_requested_rows {
            committed_viewport.y = previous_viewport
                .top()
                .saturating_add(shrink_committed_rows);
            committed_viewport.height = terminal_height.saturating_sub(committed_viewport.y);
        }
        reveal_tail_rows = history_tail_reveal_rows.min(shrink_committed_rows);
        append_fill_rows = shrink_committed_rows.saturating_sub(reveal_tail_rows);
    }

    NativeViewportGeometryCommit {
        previous_viewport,
        desired_viewport,
        committed_viewport,
        shrink_requested_rows,
        shrink_committed_rows,
        reveal_tail_rows,
        append_fill_rows,
        shrink_deferred_rows: shrink_requested_rows.saturating_sub(shrink_committed_rows),
    }
}

#[cfg(test)]
fn draw_native_frame_for_test<B>(
    terminal: &mut SurfaceTerminal<B>,
    surface: &mut NativeSurfaceController,
    main_screen_viewport_pin: &mut NativeViewportPin,
    state: &AppState,
    plan: SurfaceFramePlan,
    size: Size,
    native_frame: NativeSurfaceFramePlan,
) -> Result<NativeViewportGeometryCommit, B::Error>
where
    B: SurfaceBackend,
{
    let live_height = native_frame
        .live_lines
        .as_ref()
        .map(|lines| lines.len() as u16);
    let commit = sync_main_surface_area(
        terminal,
        main_screen_viewport_pin,
        state,
        plan,
        size,
        live_height,
        &native_frame,
    )?;
    if commit.reveal_tail_rows > 0 {
        surface.fill_history_tail_gap(terminal, commit.reveal_tail_rows)?;
    }
    surface.draw_with_plan_at_frame(terminal, state, plan, native_frame, 0)?;
    assert!(flowing_viewport_seats_flush(
        *main_screen_viewport_pin,
        terminal.viewport_area().top(),
        terminal.history_bottom_y(),
    ));
    Ok(commit)
}

impl<B> Drop for Tui<B>
where
    B: SurfaceBackend,
{
    fn drop(&mut self) {
        let _ = self.prepare_shell_prompt_after_exit();
        if self.restore_terminal_on_drop {
            let _ = restore_terminal();
        }
        // zsh shows PROMPT_EOL_MARK (`%`) when the command's final output
        // does not end in a newline. Terminal mode restore emits escape
        // sequences, so the newline must be the last best-effort write.
        let _ = self.terminal.backend_mut().write_drop_trailing_newline();
    }
}

#[cfg(test)]
pub(crate) fn native_viewport_area(anchor_y: u16, size: Size, desired_height: u16) -> Rect {
    native_viewport_geometry_with_max(
        anchor_y,
        size,
        desired_height,
        NATIVE_VIEWPORT_MAX_HEIGHT,
        /*history_backs_pinned_row*/ false,
    )
    .area
}

/// Max inline-viewport height for this frame. Streaming/idle is capped at
/// [`NATIVE_VIEWPORT_MAX_HEIGHT`] so finalized history stays visible, but while
/// an interactive prompt (AskUserQuestion / permission) is active the viewport
/// may grow to nearly the full screen so all options fit — the viewport only
/// grows to the prompt's actual desired height, so small prompts stay small and
/// large ones push history up into scrollback. Mirrors codex's bottom pane,
/// which sizes to content rather than a fixed cap.
fn interactive_viewport_max_height(state: &AppState, screen_height: u16) -> u16 {
    if state.ui.interaction.active_prompt.is_some() {
        screen_height
            .saturating_sub(NATIVE_VIEWPORT_MIN_HEIGHT)
            .max(NATIVE_VIEWPORT_MAX_HEIGHT)
            .min(screen_height)
    } else {
        NATIVE_VIEWPORT_MAX_HEIGHT
    }
}

#[cfg(any(test, feature = "testing"))]
pub(crate) fn native_viewport_area_with_max(
    anchor_y: u16,
    size: Size,
    desired_height: u16,
    max_height: u16,
) -> Rect {
    native_viewport_geometry_with_max(
        anchor_y,
        size,
        desired_height,
        max_height,
        /*history_backs_pinned_row*/ false,
    )
    .area
}

/// The flowing-seat invariant: a Flowing viewport must sit flush on history
/// (`viewport_top == history_bottom_y`). BottomPinned viewports are exempt —
/// they may carry a transient backed gap pending tail-reveal
/// (`shrink_deferred_rows > 0`). The pin guard is load-bearing; a Flowing gap
/// is the `/clear`-class stale-anchor / second-writer regression.
fn flowing_viewport_seats_flush(
    pin: NativeViewportPin,
    viewport_top: u16,
    history_bottom_y: u16,
) -> bool {
    pin != NativeViewportPin::Flowing || viewport_top == history_bottom_y
}

/// Compute the inline viewport geometry for a frame.
///
/// The bottom-pin state is a pure function of whether finalized history still
/// reaches the bottom-pinned row: once history can no longer back that row
/// (`anchor_y < bottom_pinned_y`) the viewport reverts to flowing and seats
/// flush against history. It is intentionally NOT sticky — a latched pin that
/// outlived its history is exactly what strands an unbacked gap when history
/// shrinks (`/clear`, reflow, display-toggle, rewind).
fn native_viewport_geometry_with_max(
    anchor_y: u16,
    size: Size,
    desired_height: u16,
    max_height: u16,
    history_backs_pinned_row: bool,
) -> NativeViewportGeometry {
    if size.height == 0 {
        return NativeViewportGeometry {
            area: Rect::new(0, 0, size.width, 0),
            pin: NativeViewportPin::Flowing,
        };
    }
    let height = native_viewport_height(size, desired_height, max_height);
    let bottom_pinned_y = size.height.saturating_sub(height);
    let pin = if anchor_y >= bottom_pinned_y || history_backs_pinned_row {
        NativeViewportPin::BottomPinned
    } else {
        NativeViewportPin::Flowing
    };
    let y = match pin {
        NativeViewportPin::Flowing => anchor_y,
        NativeViewportPin::BottomPinned => bottom_pinned_y,
    };
    NativeViewportGeometry {
        area: Rect::new(0, y, size.width, height),
        pin,
    }
}

fn native_viewport_height(size: Size, desired_height: u16, max_height: u16) -> u16 {
    if size.height == 0 {
        return 0;
    }
    desired_height
        .clamp(
            NATIVE_VIEWPORT_MIN_HEIGHT,
            max_height.max(NATIVE_VIEWPORT_MIN_HEIGHT),
        )
        .min(size.height)
}

#[cfg(test)]
#[path = "terminal.test.rs"]
mod tests;
