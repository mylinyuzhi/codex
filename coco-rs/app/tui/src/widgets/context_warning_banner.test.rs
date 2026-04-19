use super::CONTEXT_CRITICAL_THRESHOLD;
use super::CONTEXT_WARNING_THRESHOLD;
use super::ContextWarningBanner;

#[test]
fn gating_matches_thresholds() {
    assert!(!ContextWarningBanner::should_display(None));
    assert!(!ContextWarningBanner::should_display(Some(0.0)));
    assert!(!ContextWarningBanner::should_display(Some(
        CONTEXT_WARNING_THRESHOLD - 0.1
    )));
    assert!(ContextWarningBanner::should_display(Some(
        CONTEXT_WARNING_THRESHOLD
    )));
    assert!(ContextWarningBanner::should_display(Some(
        CONTEXT_CRITICAL_THRESHOLD
    )));
    assert!(ContextWarningBanner::should_display(Some(100.0)));
}
