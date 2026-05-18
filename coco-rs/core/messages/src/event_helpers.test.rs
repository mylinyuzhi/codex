use super::message_appended;
use super::try_appended_message;
use crate::SystemMessage;
use crate::SystemUserInterruptionMessage;
use crate::create_user_interruption_system_message;
use coco_types::ServerNotification;

#[test]
fn round_trip_system_user_interruption_message() {
    let original = create_user_interruption_system_message(true);
    let notif = message_appended(&original).expect("serialize");

    let ServerNotification::MessageAppended { .. } = &notif else {
        panic!("expected MessageAppended variant");
    };

    let decoded = try_appended_message(&notif)
        .expect("variant matches")
        .expect("deserialize");

    let crate::Message::System(SystemMessage::UserInterruption(SystemUserInterruptionMessage {
        for_tool_use,
        ..
    })) = decoded
    else {
        panic!("expected SystemMessage::UserInterruption after round-trip");
    };
    assert!(for_tool_use);
}

#[test]
fn try_appended_message_returns_none_for_other_variant() {
    let notif = ServerNotification::MessageTruncated { keep_count: 3 };
    assert!(try_appended_message(&notif).is_none());
}
