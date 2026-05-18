//! Process suspend / resume (Ctrl+Z / fg) on Unix.
//!
//! In raw mode the terminal no longer translates Ctrl+Z into a
//! `SIGTSTP` for us — the keystroke just lands as an ordinary
//! `KeyEvent`. To get the vim / less-style "Ctrl+Z drops you to the
//! shell, `fg` brings me back" behaviour we intercept the key in
//! [`crate::app::App::convert_crossterm_event`] and call
//! [`Tui::trigger_suspend`](crate::terminal::Tui::trigger_suspend),
//! which delegates here.
//!
//! Flow (Unix):
//!
//! 1. [`crate::terminal::leave_tui_modes`] — turn off raw mode, leave any
//!    state alt-screen, and disable bracketed paste / focus change reporting.
//! 2. Show the cursor on a fresh normal-buffer row so the shell sees its
//!    prompt.
//! 3. Record a pending [`ResumeAction`] for the next draw.
//! 4. `libc::kill(0, SIGTSTP)` — kernel stops the process group. The
//!    call returns synchronously, but the next user-mode instruction
//!    doesn't execute until SIGCONT arrives (default `fg` behaviour,
//!    or external `kill -CONT $pid`).
//! 5. [`crate::terminal::enter_tui_modes`] — re-arm raw mode etc.
//! 6. [`SuspendContext::prepare_resume_action`] is consumed inside
//!    [`crate::terminal::Tui::draw`] on the next frame, where
//!    [`PreparedResumeAction::apply`] clears the native surface and forces a
//!    full repaint. If a large state is still active, `Tui` re-enters
//!    alt-screen through normal state placement.
//!
//! Windows: no `SIGTSTP`; all entry points become no-ops.
//!
//! Design notes live in `docs/coco-rs/ui/rendering-hardening-and-rollback.md`.

#[cfg(unix)]
use std::io;
#[cfg(unix)]
use std::sync::Arc;
#[cfg(unix)]
use std::sync::Mutex;

#[cfg(unix)]
use crossterm::cursor::MoveToNextLine;
#[cfg(unix)]
use crossterm::cursor::Show;
#[cfg(unix)]
use crossterm::execute;

// ───────────────────────── Unix implementation ─────────────────────────

/// Records that a `Ctrl+Z → fg` cycle just happened so the next draw
/// can clear the now-dirty terminal state and force a full repaint.
#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResumeAction {
    Restore,
}

/// Opaque handle returned from
/// [`SuspendContext::prepare_resume_action`]. `Tui::draw` consumes it by
/// calling [`PreparedResumeAction::apply`].
#[cfg(unix)]
#[derive(Debug)]
pub struct PreparedResumeAction(ResumeAction);

#[cfg(unix)]
impl PreparedResumeAction {
    /// Force a full repaint after SIGCONT. The caller owns the concrete
    /// terminal surface and provides the clear/invalidate operation.
    pub fn apply<F>(self, mut clear_surface: F) -> io::Result<()>
    where
        F: FnMut() -> io::Result<()>,
    {
        match self.0 {
            ResumeAction::Restore => {
                clear_surface()?;
                Ok(())
            }
        }
    }
}

/// State machine for suspend / resume. Cheap to construct; the only
/// inner mutable bit is an `Option<ResumeAction>` guarded by a Mutex
/// (Mutex chosen over RefCell because `Tui` is `Send`-able and we want
/// to keep that property without re-checking).
#[cfg(unix)]
#[derive(Default)]
pub struct SuspendContext {
    resume_pending: Arc<Mutex<Option<ResumeAction>>>,
}

#[cfg(unix)]
impl SuspendContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop into shell, blocking until SIGCONT brings us back.
    ///
    /// Errors come from the terminal-mode IO (raw mode toggle, escape
    /// sequence write) or the `libc::kill` syscall. The pending resume
    /// flag is set even if `kill` succeeds — see step 3 of the module
    /// docs.
    pub fn suspend(&self) -> io::Result<()> {
        let mut stdout = io::stdout();

        // 1. Disable our private terminal modes so the shell can read
        //    user input normally (otherwise keystrokes wouldn't echo).
        crate::terminal::leave_tui_modes()?;

        // 2. Show the cursor on a fresh row in the normal buffer. This
        //    also undoes any `Hide` queued by the post-draw cursor pin
        //    path before yielding to the shell.
        if let Err(err) = execute!(stdout, MoveToNextLine(1), Show) {
            return restore_after_suspend_error(&mut stdout, err);
        }

        // 3. Record what the next draw needs to do.
        if let Ok(mut guard) = self.resume_pending.lock() {
            *guard = Some(ResumeAction::Restore);
        }

        // 4. Deliver SIGTSTP to our process group. `kill(0, sig)` hits
        //    every process in the caller's pgroup, including the
        //    caller, so the kernel stops us before returning to user
        //    mode. Control resumes here only after SIGCONT.
        let rc = unsafe { libc::kill(0, libc::SIGTSTP) };
        if rc != 0 {
            return restore_after_suspend_error(&mut stdout, io::Error::last_os_error());
        }

        // 5. We're back. Re-arm TUI modes; surface clear happens on the next
        //    `Tui::draw` via `PreparedResumeAction::apply`.
        crate::terminal::enter_tui_modes(&mut stdout)?;
        Ok(())
    }

    /// Consume any pending resume action. Called from
    /// [`crate::terminal::Tui::draw`] at the top of each frame so the
    /// surface repaint happens before render reads from the terminal.
    pub fn prepare_resume_action(&self) -> Option<PreparedResumeAction> {
        let action = self.resume_pending.lock().ok()?.take()?;
        Some(PreparedResumeAction(action))
    }
}

#[cfg(unix)]
fn restore_after_suspend_error(stdout: &mut io::Stdout, err: io::Error) -> io::Result<()> {
    match crate::terminal::enter_tui_modes(stdout) {
        Ok(()) => Err(err),
        Err(restore_err) => Err(io::Error::new(
            restore_err.kind(),
            format!(
                "failed to restore TUI modes after suspend error ({err}); restore error: {restore_err}"
            ),
        )),
    }
}

// ─────────────────────── Non-Unix no-op stubs ──────────────────────────

#[cfg(not(unix))]
#[derive(Default)]
pub struct SuspendContext;

#[cfg(not(unix))]
#[derive(Debug)]
pub struct PreparedResumeAction;

#[cfg(not(unix))]
impl SuspendContext {
    pub fn new() -> Self {
        Self
    }

    pub fn suspend(&self) -> std::io::Result<()> {
        Ok(())
    }

    pub fn prepare_resume_action(&self) -> Option<PreparedResumeAction> {
        None
    }
}

#[cfg(not(unix))]
impl PreparedResumeAction {
    pub fn apply<F>(self, mut clear_surface: F) -> std::io::Result<()>
    where
        F: FnMut() -> std::io::Result<()>,
    {
        clear_surface()?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "job_control.test.rs"]
mod tests;
