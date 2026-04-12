//! Message creation, normalization, filtering, history, cost tracking, lookups.
//!
//! TS: utils/messages.ts (~193K LOC), history.ts, cost-tracker.ts

pub mod cost;
pub mod creation;
pub mod filtering;
pub mod history;
pub mod lookups;
pub mod normalize;
pub mod predicates;
pub mod wrapping;

pub use cost::CostTracker;
pub use cost::calculate_cost_usd;
pub use cost::format_cost;
pub use cost::get_model_pricing;
pub use creation::create_assistant_error_message;
pub use creation::create_assistant_message;
pub use creation::create_cancellation_message;
pub use creation::create_compact_boundary_message;
pub use creation::create_error_tool_result;
pub use creation::create_info_message;
pub use creation::create_meta_message;
pub use creation::create_permission_denied_message;
pub use creation::create_progress_message;
pub use creation::create_tool_result_message;
pub use creation::create_user_message;
pub use creation::create_user_message_with_parts;
pub use history::MessageHistory;
pub use lookups::MessageLookups;
pub use lookups::build_message_lookups;
pub use normalize::ensure_user_first;
pub use normalize::merge_consecutive_assistant_messages;
pub use normalize::merge_consecutive_user_messages;
pub use normalize::normalize_messages_for_api;
pub use normalize::strip_images_from_messages;
pub use normalize::strip_signature_blocks;
pub use normalize::to_llm_prompt;
