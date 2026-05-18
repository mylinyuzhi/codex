//! Bridges between typed `Message` and `ServerNotification` history events.
//!
//! `coco_types::ServerNotification::MessageAppended` carries
//! `serde_json::Value` for its `message` field because `coco-types` is
//! provider-agnostic and cannot reference `coco_messages::Message`
//! directly. These helpers do the (de)serialization at the
//! `coco-messages` layer so engine emitters and TUI / SDK consumers
//! never write raw `serde_json::*` calls at the call site.
//!
//! See `engine-tui-unified-transcript-plan.md` §4.1.

use crate::Message;
use coco_types::ServerNotification;

/// Build a `MessageAppended` notification from a typed `Message`.
///
/// Returns `Err` only if the message contains content that fails
/// `serde_json::to_value` — practically infeasible for the in-tree
/// `Message` shape, but surfaced rather than panicked.
pub fn message_appended(msg: &Message) -> Result<ServerNotification, serde_json::Error> {
    Ok(ServerNotification::MessageAppended {
        message: serde_json::to_value(msg)?,
    })
}

/// Extract the typed `Message` from a `MessageAppended` notification.
/// Returns `None` for any other variant; `Some(Err)` if the payload
/// cannot be parsed back into the current `Message` shape (e.g. wire
/// from an incompatible future version).
pub fn try_appended_message(
    notif: &ServerNotification,
) -> Option<Result<Message, serde_json::Error>> {
    match notif {
        ServerNotification::MessageAppended { message } => {
            Some(serde_json::from_value(message.clone()))
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "event_helpers.test.rs"]
mod tests;
