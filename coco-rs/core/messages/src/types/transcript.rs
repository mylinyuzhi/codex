use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

use coco_types::AgentColorEntry;
use coco_types::AgentNameEntry;
use coco_types::AgentSettingEntry;
use coco_types::AiTitleEntry;
use coco_types::AttributionSnapshotEntry;
use coco_types::CustomTitleEntry;
use coco_types::PrLinkEntry;
use coco_types::SummaryEntry;
use coco_types::TagEntry;
use coco_types::TaskSummaryEntry;

use super::Message;

/// Full transcript message (SerializedMessage + transcript metadata).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptMessage {
    pub message: Message,
    pub cwd: String,
    pub user_type: String,
    pub session_id: String,
    pub timestamp: String,
    pub version: String,
    pub parent_uuid: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logical_parent_uuid: Option<Uuid>,
    #[serde(default)]
    pub is_sidechain: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entrypoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_id: Option<String>,
}

/// Discriminated union of all transcript entry types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TranscriptEntry {
    Transcript(Box<TranscriptMessage>),
    Summary(SummaryEntry),
    CustomTitle(CustomTitleEntry),
    AiTitle(AiTitleEntry),
    Tag(TagEntry),
    AgentName(AgentNameEntry),
    AgentColor(AgentColorEntry),
    AgentSetting(AgentSettingEntry),
    TaskSummary(TaskSummaryEntry),
    PrLink(PrLinkEntry),
    AttributionSnapshot(AttributionSnapshotEntry),
}
