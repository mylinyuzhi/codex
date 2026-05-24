use super::*;

#[test]
fn test_wrap_with_tag() {
    let content = "Hello world";

    let wrapped = wrap_with_tag(content, XmlTag::SystemReminder);
    assert_eq!(
        wrapped,
        "<system-reminder>\nHello world\n</system-reminder>"
    );

    let wrapped = wrap_with_tag(content, XmlTag::NewDiagnostics);
    assert_eq!(
        wrapped,
        "<new-diagnostics>\nHello world\n</new-diagnostics>"
    );

    let wrapped = wrap_with_tag(content, XmlTag::None);
    assert_eq!(wrapped, "Hello world");
}

#[test]
fn test_wrap_system_reminder() {
    let content = "Important context";
    let wrapped = wrap_system_reminder(content);
    assert_eq!(
        wrapped,
        "<system-reminder>\nImportant context\n</system-reminder>"
    );
}

#[test]
fn test_extract_system_reminder() {
    // Basic case
    let wrapped = "<system-reminder>\nHello world\n</system-reminder>";
    assert_eq!(extract_system_reminder(wrapped), Some("Hello world"));

    // Without newlines
    let wrapped = "<system-reminder>Hello world</system-reminder>";
    assert_eq!(extract_system_reminder(wrapped), Some("Hello world"));

    // Multi-line content
    let wrapped = "<system-reminder>\nLine 1\nLine 2\nLine 3\n</system-reminder>";
    assert_eq!(
        extract_system_reminder(wrapped),
        Some("Line 1\nLine 2\nLine 3")
    );

    // No match
    let text = "Just plain text";
    assert_eq!(extract_system_reminder(text), None);

    // Partial tag
    let text = "<system-reminder>unclosed";
    assert_eq!(extract_system_reminder(text), None);
}

#[test]
fn test_roundtrip() {
    let original = "This is some important context\nwith multiple lines";
    let wrapped = wrap_system_reminder(original);
    let extracted = extract_system_reminder(&wrapped);
    assert_eq!(extracted, Some(original));
}

#[test]
fn test_has_system_reminder() {
    assert!(has_system_reminder(
        "<system-reminder>content</system-reminder>"
    ));
    assert!(has_system_reminder(
        "prefix <system-reminder>content</system-reminder> suffix"
    ));
    assert!(!has_system_reminder("plain text"));
    assert!(!has_system_reminder("<system-reminder>unclosed"));
}

#[test]
fn test_has_tag() {
    let text = "<new-diagnostics>content</new-diagnostics>";
    assert!(has_tag(text, XmlTag::NewDiagnostics));
    assert!(!has_tag(text, XmlTag::SystemReminder));
    assert!(!has_tag(text, XmlTag::None));
}

#[test]
fn test_extract_other_tags() {
    let text = "<new-diagnostics>\nDiagnostic info\n</new-diagnostics>";
    let content = extract_tag_content(text, "new-diagnostics");
    assert_eq!(content, Some("Diagnostic info"));
}
