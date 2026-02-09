use super::*;

#[test]
fn test_toast_creation() {
    let toast = Toast::info("t1", "Test message");
    assert_eq!(toast.id, "t1");
    assert_eq!(toast.message, "Test message");
    assert_eq!(toast.severity, ToastSeverity::Info);
    assert!(!toast.is_expired());
}

#[test]
fn test_toast_severity_icons() {
    assert_eq!(ToastSeverity::Info.icon(), "i");
    assert_eq!(ToastSeverity::Success.icon(), "+");
    assert_eq!(ToastSeverity::Warning.icon(), "!");
    assert_eq!(ToastSeverity::Error.icon(), "x");
}

#[test]
fn test_toast_with_duration() {
    let toast = Toast::info("t1", "Test").with_duration(Duration::from_secs(5));
    assert_eq!(toast.duration, Duration::from_secs(5));
}

#[test]
fn test_toast_expired() {
    let mut toast = Toast::info("t1", "Test");
    toast.duration = Duration::from_millis(1);
    std::thread::sleep(Duration::from_millis(5));
    assert!(toast.is_expired());
}

#[test]
fn test_toast_widget_render() {
    let toasts = vec![
        Toast::info("t1", "Info message"),
        Toast::warning("t2", "Warning message"),
    ];
    let widget = ToastWidget::new(&toasts);

    let area = Rect::new(0, 0, 50, 10);
    let mut buf = Buffer::empty(area);
    widget.render(area, &mut buf);

    // Should render without panic
}

#[test]
fn test_toast_widget_calculate_area() {
    let toasts = vec![Toast::info("t1", "Test")];
    let widget = ToastWidget::new(&toasts);

    let frame_area = Rect::new(0, 0, 100, 50);
    let area = widget.calculate_area(frame_area);

    assert!(area.x > 0);
    assert_eq!(area.y, 1);
    assert!(area.width > 0);
    assert!(area.height > 0);
}
