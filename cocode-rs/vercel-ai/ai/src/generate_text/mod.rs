//! Generate text module.
//!
//! This module provides `generate_text` and `stream_text` functions
//! for text generation from language models.

mod build_call_options;
mod callback;
mod collect_tool_approvals;
mod content_utils;
mod execute_tool_call;
mod extract_reasoning_content;
mod extract_text_content;
#[allow(clippy::module_inception)]
mod generate_text;
mod generate_text_result;
mod generated_file;
mod output;
mod parse_tool_call;
mod prune_messages;
mod reasoning_output;
mod response_message;
mod smooth_stream;
mod step_result;
mod stop_condition;
mod stream_text;
mod to_response_messages;
mod tool_call_repair;
mod tool_error;
mod tool_output;

// --- Re-exports ---

// build_call_options (shared utility, used internally; export filter_active_tools)
pub use build_call_options::filter_active_tools;

// callback types
pub use callback::FinishEventMetadata;
pub use callback::GenerateTextCallbacks;
pub use callback::OnChunkEvent;
pub use callback::OnFinishEvent;
pub use callback::OnStartEvent;
pub use callback::OnStepFinishEvent;
pub use callback::OnStepStartEvent;
pub use callback::OnToolCallFinishEvent;
pub use callback::OnToolCallStartEvent;
pub use callback::StreamTextCallbacks;

// collect_tool_approvals
pub use collect_tool_approvals::AutoApproveCollector;
pub use collect_tool_approvals::PromptApprovalCollector;
pub use collect_tool_approvals::ToolApproval;
pub use collect_tool_approvals::ToolApprovalCollector;
pub use collect_tool_approvals::ToolApprovalRequest;
pub use collect_tool_approvals::ToolApprovalStatus;
pub use collect_tool_approvals::all_approved;
pub use collect_tool_approvals::apply_approvals;
pub use collect_tool_approvals::collect_tool_approvals;
pub use collect_tool_approvals::get_denied_approvals;

// content_utils (shared extraction functions)
pub use content_utils::extract_reasoning;
pub use content_utils::extract_reasoning_outputs;
pub use content_utils::extract_text;
pub use content_utils::extract_tool_calls;

// execute_tool_call
pub use execute_tool_call::execute_tool_call;
pub use execute_tool_call::execute_tool_calls;
pub use execute_tool_call::execute_tool_calls_with_concurrency;
pub use execute_tool_call::output_to_result_content;
pub use execute_tool_call::validate_tool_call;
pub use execute_tool_call::validate_tool_calls;

// extract_reasoning_content
pub use extract_reasoning_content::extract_reasoning_content;
pub use extract_reasoning_content::extract_reasoning_text;
pub use extract_reasoning_content::extract_reasoning_with_stats;
pub use extract_reasoning_content::has_reasoning_content;

// extract_text_content
pub use extract_text_content::extract_text_content;
pub use extract_text_content::extract_text_content_with_metadata;

// generate_text
pub use generate_text::GenerateTextOptions;
pub use generate_text::PrepareStepContext;
pub use generate_text::PrepareStepFn;
pub use generate_text::PrepareStepOverrides;
pub use generate_text::generate_text;

// generate_text_result (ToolCall, ToolResult, GenerateTextResult)
pub use generate_text_result::GenerateTextResult;
pub use generate_text_result::ToolCall;
pub use generate_text_result::ToolResult;

// generated_file
pub use generated_file::GeneratedFile;
pub use generated_file::GeneratedFiles;

// output
pub use output::Output;
pub use output::OutputMode;
pub use output::OutputStrategy;

// parse_tool_call
pub use parse_tool_call::ParsedToolCall;
pub use parse_tool_call::parse_tool_call_input;

// prune_messages
pub use prune_messages::PruneMessagesOptions;
pub use prune_messages::ReasoningPruneMode;
pub use prune_messages::ToolCallsPruneMode;
pub use prune_messages::ToolCallsPruneModeInner;
pub use prune_messages::prune_messages;

// reasoning_output
pub use reasoning_output::ReasoningOutput;

// response_message
pub use response_message::ResponseMessageData;
pub use response_message::build_assistant_message;
pub use response_message::build_assistant_message_from_text;
pub use response_message::build_single_tool_result_message;
pub use response_message::build_tool_result_message;

// smooth_stream
pub use smooth_stream::SmoothStream;
pub use smooth_stream::SmoothStreamConfig;
pub use smooth_stream::smooth_stream_iter;

// step_result (canonical StepResult)
pub use step_result::StepResult;

// stop_condition
pub use stop_condition::StopCondition;
pub use stop_condition::has_tool_call;
pub use stop_condition::is_stop_condition_met;
pub use stop_condition::response_contains;
pub use stop_condition::step_count_is;

// stream_text
pub use stream_text::StreamTextOptions;
pub use stream_text::StreamTextResult;
pub use stream_text::TextStreamPart;
pub use stream_text::stream_text;

// to_response_messages
pub use to_response_messages::ResponseMessages;
pub use to_response_messages::build_assistant_response;
pub use to_response_messages::build_text_response;
pub use to_response_messages::build_tool_result_message as build_tool_msg;
pub use to_response_messages::to_response_messages;
pub use to_response_messages::to_response_messages_from_tool_calls;
pub use to_response_messages::to_response_messages_with_text;

// tool_call_repair
pub use tool_call_repair::CustomRepairFunction;
pub use tool_call_repair::JsonRepairFunction;
pub use tool_call_repair::RepairResult;
pub use tool_call_repair::ToolCallRepairFunction;
pub use tool_call_repair::repair_tool_call;
pub use tool_call_repair::repair_tool_calls;
pub use tool_call_repair::validate_tool_call_for_repair;

// tool_error
pub use tool_error::ToolError;
pub use tool_error::ToolResult as ToolExecutionResult;
pub use tool_error::tool_error;
pub use tool_error::tool_error_with_context;

// tool_output
pub use tool_output::ToolOutput;
pub use tool_output::ToolOutputContent;
