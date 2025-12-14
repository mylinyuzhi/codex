//! Subagent execution context.

use crate::subagent::definition::AgentDefinition;
use crate::subagent::tool_filter::ToolFilter;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

// Note: HashSet is still used for filter_unresolved_tool_uses, not for parent_tools

/// Isolated execution context for a subagent.
#[derive(Debug)]
pub struct SubagentContext {
    /// The agent definition.
    pub definition: Arc<AgentDefinition>,

    /// Working directory for this subagent.
    pub cwd: PathBuf,

    /// Tool filter based on definition.
    pub tool_filter: ToolFilter,

    /// Unique identifier for this subagent instance.
    pub agent_id: String,

    /// Whether permission prompts should be suppressed.
    pub suppress_permissions: bool,

    /// Parent's cancellation token (for abort propagation).
    pub cancellation_token: CancellationToken,

    /// Resolved model for this subagent.
    pub model: String,
}

impl SubagentContext {
    /// Create a new subagent context.
    ///
    /// Tool access is determined solely by the agent's definition - no parent tool
    /// intersection is performed. Subagents can use any tool their definition allows.
    pub fn new(
        definition: Arc<AgentDefinition>,
        cwd: PathBuf,
        cancellation_token: CancellationToken,
        model: String,
    ) -> Self {
        let tool_filter = ToolFilter::new(&definition);
        let agent_id = format!("agent-{}", uuid::Uuid::new_v4());

        Self {
            definition,
            cwd,
            tool_filter,
            agent_id,
            suppress_permissions: true,
            cancellation_token,
            model,
        }
    }

    /// Set the resolved model (builder pattern).
    pub fn with_model(mut self, model: String) -> Self {
        self.model = model;
        self
    }

    /// Set whether to suppress permissions.
    pub fn with_suppress_permissions(mut self, suppress: bool) -> Self {
        self.suppress_permissions = suppress;
        self
    }

    /// Set async mode for background execution (restricts to ASYNC_SAFE_TOOLS).
    pub fn with_async(mut self, is_async: bool) -> Self {
        self.tool_filter = self.tool_filter.with_async(is_async);
        self
    }

    /// Check if a tool is allowed in this context.
    pub fn is_tool_allowed(&self, tool_name: &str) -> bool {
        self.tool_filter.is_allowed(tool_name)
    }

    /// Get the rejection reason for a tool.
    pub fn tool_rejection_reason(&self, tool_name: &str) -> Option<String> {
        self.tool_filter.rejection_reason(tool_name)
    }

    /// Create fork context entry messages with boundary markers.
    ///
    /// If `fork_context` is enabled for this agent, this method returns the parent
    /// messages with a boundary marker appended. Unresolved tool calls are filtered out.
    pub fn create_fork_context_entry(&self, parent_messages: &[ResponseItem]) -> Vec<ResponseItem> {
        if !self.definition.fork_context {
            return vec![];
        }

        let boundary = "\
### FORKING CONVERSATION CONTEXT ###
### ENTERING SUB-AGENT ROUTINE ###
Entered sub-agent context

PLEASE NOTE:
- The messages above are from the main thread prior to sub-agent execution.
- Context messages may include tool_use blocks for tools not available here.
- Only complete the specific sub-agent task assigned below.";

        // Filter out unresolved tool calls from parent messages
        let filtered_messages = self.filter_unresolved_tool_uses(parent_messages);

        let mut result = filtered_messages;
        result.push(ResponseItem::Message {
            id: None,
            role: "system".to_string(),
            content: vec![ContentItem::InputText {
                text: boundary.to_string(),
            }],
        });

        result
    }

    /// Filter out FunctionCall items that don't have corresponding outputs.
    fn filter_unresolved_tool_uses(&self, messages: &[ResponseItem]) -> Vec<ResponseItem> {
        // Collect all call_ids that have responses
        let resolved_ids: HashSet<String> = messages
            .iter()
            .filter_map(|m| match m {
                ResponseItem::FunctionCallOutput { call_id, .. } => Some(call_id.clone()),
                _ => None,
            })
            .collect();

        // Filter out FunctionCall items without corresponding outputs
        messages
            .iter()
            .filter(|m| match m {
                ResponseItem::FunctionCall { call_id, .. } => resolved_ids.contains(call_id),
                _ => true,
            })
            .cloned()
            .collect()
    }
}

impl Clone for SubagentContext {
    fn clone(&self) -> Self {
        Self {
            definition: self.definition.clone(),
            cwd: self.cwd.clone(),
            tool_filter: self.tool_filter.clone(),
            agent_id: self.agent_id.clone(),
            suppress_permissions: self.suppress_permissions,
            cancellation_token: self.cancellation_token.clone(),
            model: self.model.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subagent::definition::AgentRunConfig;
    use crate::subagent::definition::AgentSource;
    use crate::subagent::definition::ModelConfig;
    use crate::subagent::definition::PromptConfig;
    use crate::subagent::definition::ToolAccess;

    fn test_definition() -> Arc<AgentDefinition> {
        Arc::new(AgentDefinition {
            agent_type: "TestAgent".to_string(),
            display_name: None,
            when_to_use: None,
            tools: ToolAccess::List(vec!["Read".to_string(), "Glob".to_string()]),
            disallowed_tools: vec!["Dangerous".to_string()],
            source: AgentSource::User,
            model_config: ModelConfig::default(),
            fork_context: false,
            prompt_config: PromptConfig::default(),
            run_config: AgentRunConfig::default(),
            input_config: None,
            output_config: None,
            approval_mode: Default::default(),
            critical_system_reminder: None,
        })
    }

    #[test]
    fn test_context_creation() {
        let definition = test_definition();
        let context = SubagentContext::new(
            definition,
            PathBuf::from("/tmp"),
            CancellationToken::new(),
            "test-model".to_string(),
        );

        assert!(context.agent_id.starts_with("agent-"));
        assert!(context.suppress_permissions);
    }

    #[test]
    fn test_tool_allowed() {
        let definition = test_definition();
        let context = SubagentContext::new(
            definition,
            PathBuf::from("/tmp"),
            CancellationToken::new(),
            "test-model".to_string(),
        );

        assert!(context.is_tool_allowed("Read"));
        assert!(context.is_tool_allowed("Glob"));
        assert!(!context.is_tool_allowed("Shell")); // Not in allowed list
        assert!(!context.is_tool_allowed("Dangerous")); // Explicitly disallowed
        assert!(!context.is_tool_allowed("Task")); // Always blocked
    }

    #[test]
    fn test_with_model() {
        let definition = test_definition();
        let context = SubagentContext::new(
            definition,
            PathBuf::from("/tmp"),
            CancellationToken::new(),
            "initial-model".to_string(),
        )
        .with_model("new-model".to_string());

        assert_eq!(context.model, "new-model");
    }

    fn fork_context_definition() -> Arc<AgentDefinition> {
        Arc::new(AgentDefinition {
            agent_type: "ForkAgent".to_string(),
            display_name: None,
            when_to_use: None,
            tools: ToolAccess::All,
            disallowed_tools: vec![],
            source: AgentSource::Builtin,
            model_config: ModelConfig::default(),
            fork_context: true, // Enable fork_context
            prompt_config: PromptConfig::default(),
            run_config: AgentRunConfig::default(),
            input_config: None,
            output_config: None,
            approval_mode: Default::default(),
            critical_system_reminder: None,
        })
    }

    #[test]
    fn test_fork_context_disabled() {
        let definition = test_definition(); // fork_context: false
        let context = SubagentContext::new(
            definition,
            PathBuf::from("/tmp"),
            CancellationToken::new(),
            "test-model".to_string(),
        );

        let parent_messages = vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: "Hello".to_string(),
            }],
        }];

        let result = context.create_fork_context_entry(&parent_messages);
        assert!(result.is_empty());
    }

    #[test]
    fn test_fork_context_enabled() {
        let definition = fork_context_definition();
        let context = SubagentContext::new(
            definition,
            PathBuf::from("/tmp"),
            CancellationToken::new(),
            "test-model".to_string(),
        );

        let parent_messages = vec![ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: "Hello".to_string(),
            }],
        }];

        let result = context.create_fork_context_entry(&parent_messages);
        assert_eq!(result.len(), 2); // Original message + boundary
        if let ResponseItem::Message { role, content, .. } = &result[1] {
            assert_eq!(role, "system");
            if let ContentItem::InputText { text } = &content[0] {
                assert!(text.contains("FORKING CONVERSATION CONTEXT"));
            } else {
                panic!("Expected InputText");
            }
        } else {
            panic!("Expected Message");
        }
    }

    #[test]
    fn test_filter_unresolved_tool_uses() {
        use codex_protocol::models::FunctionCallOutputPayload;

        let definition = fork_context_definition();
        let context = SubagentContext::new(
            definition,
            PathBuf::from("/tmp"),
            CancellationToken::new(),
            "test-model".to_string(),
        );

        let parent_messages = vec![
            ResponseItem::FunctionCall {
                id: None,
                name: "read_file".to_string(),
                arguments: "{}".to_string(),
                call_id: "call-1".to_string(),
            },
            ResponseItem::FunctionCallOutput {
                call_id: "call-1".to_string(),
                output: FunctionCallOutputPayload {
                    content: "file content".to_string(),
                    success: Some(true),
                    content_items: None,
                },
            },
            ResponseItem::FunctionCall {
                id: None,
                name: "write_file".to_string(),
                arguments: "{}".to_string(),
                call_id: "call-2".to_string(), // No corresponding output
            },
        ];

        let result = context.create_fork_context_entry(&parent_messages);
        // Should have: resolved call-1, output for call-1, boundary
        // call-2 should be filtered out
        assert_eq!(result.len(), 3);
    }
}
