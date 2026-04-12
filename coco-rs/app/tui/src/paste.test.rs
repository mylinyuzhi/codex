//! Tests for paste handling.

use crate::paste::PasteManager;
use crate::paste::is_paste_pill;

#[test]
fn test_is_paste_pill() {
    assert!(is_paste_pill("[Pasted text #1]"));
    assert!(is_paste_pill("[Pasted text #42]"));
    assert!(is_paste_pill("[Image #1]"));
    assert!(!is_paste_pill("[Something else]"));
    assert!(!is_paste_pill("not a pill"));
    assert!(!is_paste_pill("[unclosed"));
}

#[test]
fn test_paste_manager_text() {
    let mut mgr = PasteManager::new();
    let pill = mgr.add_text("hello world".to_string());
    assert_eq!(pill, "[Pasted text #1]");
    assert_eq!(mgr.entries().len(), 1);
    assert!(!mgr.entries()[0].is_image);
}

#[test]
fn test_paste_manager_resolve() {
    let mut mgr = PasteManager::new();
    mgr.add_text("content A".to_string());
    mgr.add_text("content B".to_string());

    let input = "See [Pasted text #1] and [Pasted text #2]";
    let resolved = mgr.resolve(input);
    assert_eq!(resolved, "See content A and content B");
}

#[test]
fn test_paste_manager_numbering() {
    let mut mgr = PasteManager::new();
    let p1 = mgr.add_text("a".to_string());
    let p2 = mgr.add_image("/tmp/img.png".to_string());
    assert_eq!(p1, "[Pasted text #1]");
    assert_eq!(p2, "[Image #2]");
}
