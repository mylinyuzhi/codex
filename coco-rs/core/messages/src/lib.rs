//! Message creation, normalization, history, cost tracking, lookups.
//!
//! TS: utils/messages.ts (~193K LOC), history.ts, cost-tracker.ts

pub mod cost;
pub mod creation;
pub mod history;
pub mod lookups;
pub mod normalize;
pub mod pipeline;
pub mod predicates;
pub mod wrapping;

// Message-family types live in `coco-types` (the wire-protocol crate
// owns its payload shapes). Re-export at this crate root so the
// established `coco_messages::Message` import path keeps working for
// the operations layer that does normalization / history.
pub use coco_types::messages::*;

pub use cost::CostTracker;
pub use cost::calculate_cost_usd;
pub use cost::format_cost;
pub use cost::get_model_pricing;
pub use creation::INTERRUPT_MESSAGE;
pub use creation::INTERRUPT_MESSAGE_FOR_TOOL_USE;
pub use creation::create_assistant_error_message;
pub use creation::create_assistant_message;
pub use creation::create_compact_boundary_message;
pub use creation::create_error_tool_result;
pub use creation::create_info_message;
pub use creation::create_meta_message;
pub use creation::create_permission_denied_message;
pub use creation::create_progress_message;
pub use creation::create_tool_result_message;
pub use creation::create_tool_result_message_with_parts;
pub use creation::create_user_interruption_message;
pub use creation::create_user_interruption_system_message;
pub use creation::create_user_message;
pub use creation::create_user_message_with_parts;
pub use creation::create_user_message_with_parts_and_uuid;
pub use creation::create_user_message_with_uuid;
pub use history::MessageHistory;
pub use lookups::MessageLookups;
pub use lookups::build_message_lookups;
pub use normalize::EXIT_PLAN_MODE_INJECTED_PLAN_FIELD;
pub use normalize::EXIT_PLAN_MODE_INJECTED_PLAN_FILE_PATH_FIELD;
pub use normalize::ensure_user_first;
pub use normalize::merge_consecutive_assistant_messages;
pub use normalize::merge_consecutive_user_messages;
pub use normalize::normalize_messages_for_api;
pub use normalize::strip_images_from_messages;
pub use normalize::strip_signature_blocks;
pub use normalize::to_llm_prompt;
pub use predicates::count_tool_calls_in_last_assistant_turn;
