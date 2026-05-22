use std::io::Write;
use std::sync::Arc;

use pretty_assertions::assert_eq;
use tokio::sync::RwLock;

use super::*;
use coco_context::FileReadState;
use coco_context::attachment::Attachment;
use coco_context::attachment::DirectoryAttachment;
use coco_context::attachment::FileAttachment;
use coco_context::attachment::ImageAttachment;
use coco_messages::Message;
use coco_messages::wrapping::extract_text_from_message;

fn read_tool_name() -> &'static str {
    coco_types::ToolName::Read.as_str()
}

fn bash_tool_name() -> &'static str {
    coco_types::ToolName::Bash.as_str()
}

#[test]
fn attachment_to_messages_file_emits_two_system_reminders() {
    let att = Attachment::File(FileAttachment {
        filename: "/abs/path/foo.rs".to_string(),
        content: "fn main() { println!(\"hi\") }".to_string(),
        truncated: false,
        display_path: "foo.rs".to_string(),
        offset: None,
        limit: None,
    });

    let msgs = attachment_to_messages(&att);
    assert_eq!(
        msgs.len(),
        2,
        "TS parity: createToolUseMessage + createToolResultMessage = 2 messages"
    );

    let call = extract_text_from_message(&msgs[0]);
    assert!(call.contains("<system-reminder>"));
    assert!(call.contains("</system-reminder>"));
    assert!(call.contains(&format!(
        "Called the {} tool with the following input:",
        read_tool_name()
    )));
    assert!(call.contains("\"file_path\":\"/abs/path/foo.rs\""));
    assert!(
        !call.contains("Result of calling"),
        "first message is tool_use only, no result"
    );

    let result = extract_text_from_message(&msgs[1]);
    assert!(result.contains("<system-reminder>"));
    assert!(result.contains(&format!("Result of calling the {} tool:", read_tool_name())));
    assert!(result.contains("fn main()"));
    assert!(
        !result.contains("Called the"),
        "second message is tool_result only"
    );
}

#[test]
fn attachment_to_messages_directory_emits_two_bash_reminders() {
    let att = Attachment::Directory(DirectoryAttachment {
        path: "/abs/dir".to_string(),
        content: "foo.rs\nbar.rs".to_string(),
        display_path: "dir".to_string(),
    });

    let msgs = attachment_to_messages(&att);
    assert_eq!(msgs.len(), 2);

    let call = extract_text_from_message(&msgs[0]);
    assert!(call.contains(&format!("Called the {} tool", bash_tool_name())));
    assert!(call.contains("\"command\":\"ls dir\""));

    let result = extract_text_from_message(&msgs[1]);
    assert!(result.contains(&format!("Result of calling the {} tool:", bash_tool_name())));
    assert!(result.contains("foo.rs"));
    assert!(result.contains("bar.rs"));
}

#[test]
fn attachment_to_messages_image_emits_text_then_image_message() {
    let att = Attachment::Image(ImageAttachment {
        filename: "/abs/path/pic.png".to_string(),
        media_type: "image/png".to_string(),
        base64_data: Some("AAAA".to_string()),
        display_path: "pic.png".to_string(),
    });

    let msgs = attachment_to_messages(&att);
    assert_eq!(
        msgs.len(),
        2,
        "TS parity: tool_use text + tool_result image = 2 messages"
    );

    let call = extract_text_from_message(&msgs[0]);
    assert!(call.contains("<system-reminder>"));
    assert!(call.contains(&format!(
        "Called the {} tool with the following input:",
        read_tool_name()
    )));
    assert!(call.contains("\"file_path\":\"/abs/path/pic.png\""));

    let user_msg = match &msgs[1] {
        Message::User(u) => u,
        other => panic!("expected user message with image part, got {other:?}"),
    };
    let content = match &user_msg.message {
        coco_messages::LlmMessage::User { content, .. } => content,
        other => panic!("expected LlmMessage::User, got {other:?}"),
    };
    assert_eq!(content.len(), 1, "image message has a single image part");
    assert!(
        matches!(&content[0], coco_messages::UserContent::File(_)),
        "second part is the image, unwrapped (no <system-reminder>)"
    );
    // The image-bearing message itself contains no text → not wrapped.
    let img_text = extract_text_from_message(&msgs[1]);
    assert!(
        !img_text.contains("<system-reminder>"),
        "image message has no text wrapper, got: {img_text:?}"
    );
}

#[test]
fn attachment_to_messages_image_without_base64_returns_empty() {
    let att = Attachment::Image(ImageAttachment {
        filename: "img.png".to_string(),
        media_type: "image/png".to_string(),
        base64_data: None,
        display_path: "img.png".to_string(),
    });

    assert!(attachment_to_messages(&att).is_empty());
}

#[test]
fn attachment_to_messages_already_read_file_returns_empty() {
    let att = Attachment::AlreadyReadFile(coco_context::attachment::AlreadyReadFileAttachment {
        filename: "/already.rs".to_string(),
        display_path: "already.rs".to_string(),
    });
    assert!(attachment_to_messages(&att).is_empty());
}

#[test]
fn changed_file_to_message_emits_modification_note() {
    let att = Attachment::File(FileAttachment {
        filename: "/abs/edited.rs".to_string(),
        content: "diff snippet here".to_string(),
        truncated: false,
        display_path: "edited.rs".to_string(),
        offset: None,
        limit: None,
    });

    let msg = changed_file_to_message(&att).expect("changed file produces a message");
    let text = extract_text_from_message(&msg);

    assert!(text.contains("Note: edited.rs was modified"));
    assert!(text.contains("Don't tell the user this"));
    assert!(text.contains("diff snippet here"));
}

#[tokio::test]
async fn resolve_turn_inputs_loads_at_mentioned_file_content() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("hello.txt");
    {
        let mut f = std::fs::File::create(&file).unwrap();
        f.write_all(b"file body bytes").unwrap();
    }

    let frs = Arc::new(RwLock::new(FileReadState::new()));
    let prompt = format!("read this @{}", file.display());
    let inputs = resolve_turn_inputs_text_only(&prompt, dir.path(), &frs).await;

    assert_eq!(
        inputs.attachment_messages.len(),
        2,
        "single file mention -> tool_use + tool_result (TS parity)"
    );
    let call = extract_text_from_message(&inputs.attachment_messages[0]);
    assert!(call.contains(&format!("Called the {} tool", read_tool_name())));
    let result = extract_text_from_message(&inputs.attachment_messages[1]);
    assert!(result.contains(&format!("Result of calling the {} tool:", read_tool_name())));
    assert!(
        result.contains("file body bytes"),
        "file content reaches the model: {result}"
    );

    assert_eq!(inputs.mentioned_paths.len(), 1);
    assert_eq!(inputs.mentioned_paths[0], file);

    // The user message itself carries only the prompt — content is in the
    // separate system-reminder attachments.
    let user_text = extract_text_from_message(&inputs.user_message);
    assert!(user_text.contains("read this"));
}

#[tokio::test]
async fn resolve_turn_inputs_dedups_same_file_across_calls() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("dup.txt");
    {
        let mut f = std::fs::File::create(&file).unwrap();
        f.write_all(b"first read").unwrap();
    }

    let frs = Arc::new(RwLock::new(FileReadState::new()));
    let prompt = format!("look at @{}", file.display());

    let first = resolve_turn_inputs_text_only(&prompt, dir.path(), &frs).await;
    assert_eq!(first.attachment_messages.len(), 2);

    let second = resolve_turn_inputs_text_only(&prompt, dir.path(), &frs).await;
    assert!(
        second.attachment_messages.is_empty(),
        "second mention dedups via FileReadState (AlreadyReadFile produces no message)"
    );
    // The path is still reported on `mentioned_paths` so callers can
    // refresh post-compact restoration.
    assert_eq!(second.mentioned_paths.len(), 1);
}

#[tokio::test]
async fn resolve_turn_inputs_emits_user_first_then_attachments() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("ordered.txt");
    {
        let mut f = std::fs::File::create(&file).unwrap();
        f.write_all(b"order body").unwrap();
    }

    let frs = Arc::new(RwLock::new(FileReadState::new()));
    let prompt = format!("@{} please summarize", file.display());
    let inputs = resolve_turn_inputs_text_only(&prompt, dir.path(), &frs).await;

    let messages = build_messages_for_turn(&inputs);
    assert!(messages.len() >= 3);
    // First message must be the user prompt, then the two attachment
    // messages (tool_use + tool_result, TS parity).
    assert!(matches!(messages[0], Message::User(_)));
    assert!(matches!(messages[1], Message::Attachment(_)));
    assert!(matches!(messages[2], Message::Attachment(_)));
}

#[tokio::test]
async fn resolve_turn_inputs_no_mentions_yields_only_user_message() {
    let dir = tempfile::tempdir().unwrap();
    let frs = Arc::new(RwLock::new(FileReadState::new()));
    let inputs =
        resolve_turn_inputs_text_only("just a plain prompt with no mentions", dir.path(), &frs)
            .await;

    assert!(inputs.attachment_messages.is_empty());
    assert!(inputs.changed_file_messages.is_empty());
    assert!(inputs.mentioned_paths.is_empty());

    let messages = build_messages_for_turn(&inputs);
    assert_eq!(messages.len(), 1);
    assert!(matches!(messages[0], Message::User(_)));
}
