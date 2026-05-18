use super::finalize_user_cancel;
use super::history_push_and_emit;
use super::last_message_is_user_interruption;
use coco_messages::Message;
use coco_messages::MessageHistory;
use coco_messages::SystemMessage;
use coco_messages::create_user_interruption_system_message;
use coco_messages::create_user_message;
use coco_types::CoreEvent;
use coco_types::ServerNotification;
use pretty_assertions::assert_eq;
use tokio::sync::mpsc;

#[tokio::test]
async fn finalize_user_cancel_pushes_typed_marker_and_emits() {
    let mut history = MessageHistory::new();
    let (tx, mut rx) = mpsc::channel::<CoreEvent>(16);
    let tx = Some(tx);

    finalize_user_cancel(&mut history, true, &tx).await;

    assert_eq!(history.messages.len(), 1);
    let Message::System(SystemMessage::UserInterruption(m)) = &history.messages[0] else {
        panic!("expected typed UserInterruption variant");
    };
    assert!(m.for_tool_use);

    let evt = rx.try_recv().expect("MessageAppended emitted");
    let CoreEvent::Protocol(ServerNotification::MessageAppended { .. }) = evt else {
        panic!("expected MessageAppended protocol event");
    };
}

#[tokio::test]
async fn finalize_user_cancel_dedups_against_typed_marker() {
    let mut history = MessageHistory::new();
    let (tx, _rx) = mpsc::channel::<CoreEvent>(16);
    let tx = Some(tx);

    finalize_user_cancel(&mut history, false, &tx).await;
    finalize_user_cancel(&mut history, false, &tx).await;

    assert_eq!(history.messages.len(), 1);
}

#[test]
fn last_message_is_user_interruption_recognizes_typed_form() {
    let mut history = MessageHistory::new();
    assert!(!last_message_is_user_interruption(&history));
    history.push(create_user_interruption_system_message(false));
    assert!(last_message_is_user_interruption(&history));
}

#[test]
fn last_message_is_user_interruption_recognizes_legacy_text_form() {
    let mut history = MessageHistory::new();
    history.push(coco_messages::create_user_interruption_message(true));
    assert!(last_message_is_user_interruption(&history));
}

#[test]
fn last_message_is_user_interruption_negative() {
    let mut history = MessageHistory::new();
    history.push(create_user_message("hi"));
    assert!(!last_message_is_user_interruption(&history));
}

#[tokio::test]
async fn history_push_and_emit_emits_when_channel_attached() {
    let mut history = MessageHistory::new();
    let (tx, mut rx) = mpsc::channel::<CoreEvent>(16);
    let tx = Some(tx);
    history_push_and_emit(&mut history, create_user_message("hi"), &tx).await;
    assert_eq!(history.messages.len(), 1);
    let evt = rx.try_recv().expect("expected event");
    let CoreEvent::Protocol(ServerNotification::MessageAppended { .. }) = evt else {
        panic!("expected MessageAppended");
    };
}

#[tokio::test]
async fn history_push_and_emit_is_no_op_event_when_channel_none() {
    let mut history = MessageHistory::new();
    let tx: Option<mpsc::Sender<CoreEvent>> = None;
    history_push_and_emit(&mut history, create_user_message("hi"), &tx).await;
    assert_eq!(history.messages.len(), 1);
}
