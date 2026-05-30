use std::sync::Arc;

use coco_messages::AttachmentMessage;
use coco_messages::LlmMessage;
use coco_messages::Message;
use coco_types::AttachmentKind;

use super::message_to_cells;

#[test]
fn ui_hidden_attachment_derives_no_cells() {
    let msg = Message::Attachment(AttachmentMessage::api(
        AttachmentKind::DeferredToolsDelta,
        LlmMessage::user_text("deferred tool reminder"),
    ));

    assert!(message_to_cells(Arc::new(msg)).is_empty());
}
