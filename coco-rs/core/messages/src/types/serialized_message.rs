use serde::Deserialize;
use serde::Serialize;

use coco_types::Entrypoint;
use coco_types::UserType;

use super::Message;

/// Serialized message for log persistence (session replay, analytics).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedMessage {
    pub message: Message,
    pub cwd: String,
    pub user_type: UserType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entrypoint: Option<Entrypoint>,
    pub session_id: String,
    pub timestamp: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
}
