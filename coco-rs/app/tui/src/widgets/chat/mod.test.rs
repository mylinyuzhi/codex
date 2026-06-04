use super::TRANSCRIPT_LINE_CHAR_CAP;
use super::nested_memory_chip_path;
use super::single_line_capped;
use super::transcript_safe_line;
use coco_messages::AttachmentMessage;
use coco_messages::LlmMessage;
use coco_messages::Message;
use coco_types::AttachmentKind;

#[test]
fn test_transcript_safe_line_caps_single_line_output() {
    let input = "x".repeat(TRANSCRIPT_LINE_CHAR_CAP + 20);

    let rendered = transcript_safe_line(&input);

    assert_eq!(rendered.chars().count(), TRANSCRIPT_LINE_CHAR_CAP);
    assert!(rendered.ends_with('…'));
}

#[test]
fn test_single_line_capped_collapses_whitespace_without_collecting_full_input() {
    let input = format!("alpha\n{}\nomega", "beta ".repeat(TRANSCRIPT_LINE_CHAR_CAP));

    let rendered = single_line_capped(&input, 32);

    assert!(rendered.starts_with("alpha beta"));
    assert_eq!(rendered.chars().count(), 32);
    assert!(rendered.ends_with('…'));
}

#[test]
fn nested_memory_chip_path_extracts_path_for_memory_kinds_only() {
    // A nested-CLAUDE.md reminder collapses to just its `{path}` (the `Contents
    // of …:` framing + `<system-reminder>` wrapper are stripped) for the chip.
    let body = "<system-reminder>\nContents of /repo/utils/foo/CLAUDE.md:\n\n# foo rules\n</system-reminder>";
    let memory = Message::Attachment(AttachmentMessage::api(
        AttachmentKind::NestedMemory,
        LlmMessage::user_text(body),
    ));
    // No cwd → absolute path unchanged.
    assert_eq!(
        nested_memory_chip_path(&memory, None).as_deref(),
        Some("/repo/utils/foo/CLAUDE.md")
    );
    // Under cwd → relativized for the compact chip.
    assert_eq!(
        nested_memory_chip_path(&memory, Some("/repo")).as_deref(),
        Some("utils/foo/CLAUDE.md")
    );
    // Outside cwd → left absolute (e.g. a global memory file).
    assert_eq!(
        nested_memory_chip_path(&memory, Some("/other")).as_deref(),
        Some("/repo/utils/foo/CLAUDE.md")
    );

    // A non-memory attachment is left to the generic `◇` preview path.
    let other = Message::Attachment(AttachmentMessage::api(
        AttachmentKind::DateChange,
        LlmMessage::user_text("The date has changed to 2026-06-02."),
    ));
    assert_eq!(nested_memory_chip_path(&other, Some("/repo")), None);
}
