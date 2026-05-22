//! Pure state-machine tests for [`SuspendContext`]. Cannot exercise the
//! real syscall path (`libc::kill(0, SIGTSTP)`) inside nextest — it
//! would stop the test binary.

#![cfg(unix)]

use super::*;

#[test]
fn prepare_resume_returns_none_on_a_fresh_context() {
    let ctx = SuspendContext::new();
    assert!(ctx.prepare_resume_action().is_none());
}

#[test]
fn prepare_resume_consumes_a_pending_action() {
    let ctx = SuspendContext::new();
    *ctx.resume_pending.lock().unwrap() = Some(ResumeAction::Restore);

    let prepared = ctx
        .prepare_resume_action()
        .expect("first take should yield the pending action");
    // Round-tripped through PreparedResumeAction(ResumeAction::Restore).
    assert!(matches!(
        prepared,
        PreparedResumeAction(ResumeAction::Restore)
    ));

    assert!(
        ctx.prepare_resume_action().is_none(),
        "the pending slot must be drained after one take"
    );
}
