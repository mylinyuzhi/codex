use coco_types::Capability;
use coco_types::ReasoningEffort;
use coco_types::ThinkingLevel;

use crate::model::ModelInfo;

/// Check if a model supports effort/thinking configuration.
pub fn model_supports_effort(model_info: &ModelInfo) -> bool {
    model_info.has_capability(Capability::ExtendedThinking)
}

/// Check if a model supports max (xhigh) effort.
pub fn model_supports_max_effort(model_info: &ModelInfo) -> bool {
    model_info
        .supported_thinking_levels
        .as_ref()
        .is_some_and(|levels| levels.iter().any(|l| l.effort == ReasoningEffort::XHigh))
}

/// Get the default thinking level for a model.
pub fn get_default_thinking_level(model_info: &ModelInfo) -> Option<ThinkingLevel> {
    model_info.default_thinking().cloned()
}
