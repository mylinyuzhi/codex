use super::*;

#[test]
fn drain_returns_pushed_in_fifo_order_and_clears() {
    let inbox = NoticeInbox::new();
    inbox.push(MemoryUserNotice {
        written_paths: vec!["a.md".into()],
        verb: NoticeVerb::Saved,
    });
    inbox.push(MemoryUserNotice {
        written_paths: vec!["b.md".into(), "c.md".into()],
        verb: NoticeVerb::Improved,
    });
    assert_eq!(inbox.len(), 2);
    let drained = inbox.drain();
    assert_eq!(drained.len(), 2);
    assert_eq!(drained[0].written_paths, vec!["a.md".to_string()]);
    assert_eq!(drained[0].verb, NoticeVerb::Saved);
    assert_eq!(drained[1].verb, NoticeVerb::Improved);
    // Drain clears.
    assert_eq!(inbox.len(), 0);
    assert!(inbox.drain().is_empty());
}

#[test]
fn verb_str_round_trips() {
    assert_eq!(NoticeVerb::Saved.as_str(), "Saved");
    assert_eq!(NoticeVerb::Improved.as_str(), "Improved");
}

#[test]
fn clones_share_storage() {
    let a = NoticeInbox::new();
    let b = a.clone();
    a.push(MemoryUserNotice {
        written_paths: vec!["x.md".into()],
        verb: NoticeVerb::Saved,
    });
    // Either handle drains the same underlying buffer.
    let drained = b.drain();
    assert_eq!(drained.len(), 1);
    assert_eq!(a.len(), 0);
}
