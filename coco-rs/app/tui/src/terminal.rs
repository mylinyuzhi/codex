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
/// Max rendered rows the *streaming* live tail may occupy in the inline
/// viewport. Bounding it keeps the per-turn growth phase short (≤ this many
/// repaints, once) and the viewport height constant for the rest of the turn —
/// codex keeps its bottom pane fixed for the same reason. The dropped leading
/// rows are NOT lost: they commit to native scrollback at the next markdown
/// boundary and definitively at finalize. Display-only cap (see
/// `SurfaceStreamDriver::prepare`); the markdown commit boundary is untouched.
pub(crate) const STREAMING_LIVE_TAIL_CAP: u16 = 8;

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
    /// Grow-only viewport-height watermark held during streaming so the
    /// live-tail viewport stops oscillating as lines commit to scrollback
    /// (see `apply_streaming_height_floor`). 0 when idle.
    streaming_height_high_water: u16,
    /// Grow-only viewport-height watermark held while an interactive prompt
    /// (AskUserQuestion / permission) is open, so switching between questions of
    /// different option counts never SHRINKS the pane and the bottom edge stays
    /// put. Separate from `streaming_height_high_water` because it resets when
    /// the prompt closes (not at turn end). 0 when no prompt is active.
    prompt_height_high_water: u16,
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
            streaming_height_high_water: 0,
            prompt_height_high_water: 0,
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
        // This is pure CPU work (no terminal writes) and therefore stays
        // OUTSIDE the synchronized-update window opened below.
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
        // === One synchronized-update window for the whole paint. ===
        // `?2026h` is emitted BEFORE `sync_surface_area` so the viewport
        // resize/clear/scroll is deferred by the terminal and never presents a
        // blank region between the clear and the repaint (the input-bar
        // flicker). The window brackets the clear, the native history insert,
        // and the viewport draw; the single ESU flush presents the composed
        // frame. ESU is emitted even when the inner draw errors so the terminal
        // never stays stuck in deferred-present.
        self.terminal.begin_synchronized_update()?;
        let drawn = self.draw_native_frame(state, plan, size, live, frame_index);
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
        live: Option<Vec<ratatui::text::Line<'static>>>,
        frame_index: u64,
    ) -> io::Result<crate::surface::controller::NativeSurfaceDrawOutcome> {
        let perf_config = state.ui.display_settings.performance;
        // The live tail is one display row per line, so its length is the
        // precomputed viewport content height for the sizing pass.
        let live_height = live.as_ref().map(|lines| lines.len() as u16);
        // Pass the size read by the caller so the precomputed live tail (built
        // at `size.width`) and the viewport area are derived from one consistent
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
        Ok(outcome)
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

    /// Floor the live-tail viewport height at its grow-only high-water mark
    /// when `freeze` is set, removing the per-frame size change that bounces the
    /// input bar; pass `freeze = false` to relax back to the natural height and
    /// clear the watermark. The freeze predicate (`streaming_height_freeze`)
    /// spans the whole active turn, not just streaming spans — see that helper.
    fn apply_streaming_height_floor(&mut self, desired: u16, freeze: bool) -> u16 {
        let (height, high_water) =
            streaming_height_floor(desired, self.streaming_height_high_water, freeze);
        self.streaming_height_high_water = high_water;
        height
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
            // An active interactive prompt (AskUserQuestion / permission) may
            // grow past the streaming cap so all its options are visible,
            // pushing finalized history up into scrollback (codex bottom-pane
            // sizes to content). Streaming/idle keeps the smaller cap.
            let max_h = interactive_viewport_max_height(state, size.height);
            let desired_height = interactive_viewport_desired_height(
                state,
                size.width,
                max_h,
                plan,
                precomputed_live_height,
            );
            // Freeze the live-tail height grow-only for the whole active turn so
            // the viewport stops oscillating as lines grow and then commit to
            // scrollback (the bottom-bar jitter). The viewport top anchors to the
            // bottom of finalized history; `native_viewport_area_with_max` pins
            // it to the screen bottom once history fills the screen, so a stable
            // height keeps the input bar's bottom edge fixed. Gating on the turn
            // (not just streaming) matters because `is_streaming()` flips off at
            // every tool call and message boundary mid-turn — see
            // `streaming_height_freeze`.
            let prev = self.terminal.viewport_area();
            let was_floored = self.streaming_height_high_water > 0;
            let freeze = streaming_height_freeze(state);
            let turn_height = self.apply_streaming_height_floor(desired_height, freeze);
            // Prompt-scoped grow-only floor: while an interactive prompt is open
            // its height only grows, so switching between questions of different
            // option counts never shrinks the pane (the in-prompt bottom-edge
            // wobble). The turn floor above is prompt-exempt and reset its own
            // watermark to 0, so this never double-counts. Reset + repin when the
            // prompt closes so the post-prompt content re-pins to the bottom.
            let active_prompt = state.ui.interaction.active_prompt.is_some();
            let prompt_was_floored = self.prompt_height_high_water > 0;
            let desired_height = if active_prompt {
                let (h, hw) =
                    streaming_height_floor(turn_height, self.prompt_height_high_water, true);
                self.prompt_height_high_water = hw;
                h
            } else {
                self.prompt_height_high_water = 0;
                turn_height
            };
            let relaxing = (was_floored && !freeze) || (prompt_was_floored && !active_prompt);
            let anchor = self.terminal.history_bottom_y();
            let area = native_viewport_area_with_max(anchor, size, desired_height, max_h);
            // Hold the input bar's bottom edge steady for the single frame the
            // freeze relaxes (turn end, or an interactive prompt taking over):
            // the grow-only height drops (e.g. 12→5) one frame before
            // `history_bottom_y` advances, which would slide the input UP. Pin
            // the bottom to the prior frame's bottom for that one transition
            // frame; the next frame re-anchors to history normally. `relaxing`
            // matches every relax cause, not just stream-finish.
            let area = hold_bottom_edge_on_relax(area, prev, size, relaxing);
            // The held bottom is cosmetic for one frame: the viewport top stays
            // at the tall streaming `history_bottom_y`, so without a re-pin the
            // next frame re-anchors there and the input settles high with a blank
            // gap below (once the conversation has overflowed the screen). Force a
            // history replay this frame so `move_viewport_down_for_history`
            // re-seats the viewport right after history — bottom-pinned when full,
            // below-content when short.
            if needs_repin_on_relax(relaxing, area, prev) {
                self.surface.request_repin_replay();
            }
            area
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

/// Grow-only viewport height while `freeze` holds.
///
/// Returns `(height_to_use, next_high_water)`. While `freeze`, the height never
/// drops below the running high-water mark, so the live-tail viewport stops
/// oscillating as lines grow and then commit to scrollback — the root cause of
/// the bottom-bar jitter. When `freeze` clears it passes `desired` through and
/// clears the watermark so the viewport relaxes to its natural size.
/// Terminal-sync-independent on purpose: DEC mode 2026 only makes each frame's
/// *presentation* atomic; it cannot stop consecutive frames from having
/// different heights, so the freeze is what actually holds the bottom edge
/// steady (mirrors codex-rs's fixed-height inline viewport).
fn streaming_height_floor(desired: u16, high_water: u16, freeze: bool) -> (u16, u16) {
    if freeze {
        let height = desired.max(high_water);
        (height, height)
    } else {
        (desired, 0)
    }
}

/// Whether the live-tail viewport height should be frozen grow-only this frame.
///
/// True for the whole active turn (`turn_active`) or any streaming span, so the
/// floor spans tool calls and message boundaries. `is_streaming()` alone flips
/// to `None` at every `ToolUseQueued` (→ `flush_streaming_to_messages`) and
/// assistant `MessageAppended` within a turn; gating the floor on it resets the
/// watermark mid-turn and the top-anchored input bar bounces UP each time the
/// live tail collapses. `turn_active()` stays true across the whole turn, so the
/// union holds the bottom edge steady through tool calls and message boundaries.
/// Interactive prompts (AskUserQuestion / permission) are exempt: their viewport
/// sizes to content and must stay free to shrink as the user navigates options.
fn streaming_height_freeze(state: &AppState) -> bool {
    (state.is_streaming() || state.ui.ephemeral.turn_active())
        && state.ui.interaction.active_prompt.is_none()
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

/// Hold the viewport's bottom edge at `prev`'s bottom when it would otherwise
/// rise (input bar jumping up). Used for the single stream→idle transition
/// frame where the grow-only height relaxes before `history_bottom_y` catches
/// up. No-op unless `transitioning` and the bottom would actually move up.
fn hold_bottom_edge_on_relax(area: Rect, prev: Rect, size: Size, transitioning: bool) -> Rect {
    if !transitioning || area.height == 0 || area.bottom() >= prev.bottom() {
        return area;
    }
    let max_y = size.height.saturating_sub(area.height);
    let y = prev.bottom().saturating_sub(area.height).min(max_y);
    Rect::new(area.x, y, area.width, area.height)
}

/// Whether the turn-end relax needs a one-shot history re-pin replay. True when
/// the grow-only freeze just released (`relaxing`) and the viewport is shrinking
/// (`area.height < prev.height`). The shrink alone keeps the tall streaming
/// `history_bottom_y`, so the finalized content must be replayed to re-seat the
/// viewport right after history; otherwise the input bar settles high with a
/// blank gap below once the conversation has overflowed the screen. A growing
/// relax (an interactive prompt taking over) needs no re-pin.
fn needs_repin_on_relax(relaxing: bool, area: Rect, prev: Rect) -> bool {
    relaxing && area.height < prev.height
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
