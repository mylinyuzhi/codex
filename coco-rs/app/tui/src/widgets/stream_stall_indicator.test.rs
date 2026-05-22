use super::StreamStallIndicator;

#[test]
fn should_display_gates_on_stream_stall_flag() {
    assert!(!StreamStallIndicator::should_display(false));
    assert!(StreamStallIndicator::should_display(true));
}
