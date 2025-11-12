//! WebSearch tool handler.

use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;

use crate::function_tool::FunctionCallError;
use crate::protocol::EventMsg;
use crate::protocol::WebSearchToolCallEvent;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;
use crate::tools::web_search::{WebSearchProvider, format_results_for_llm};

pub struct WebSearchHandler {
    provider: Arc<dyn WebSearchProvider>,
    max_results: usize,
}

impl WebSearchHandler {
    pub fn new(provider: Arc<dyn WebSearchProvider>, max_results: usize) -> Self {
        Self {
            provider,
            max_results,
        }
    }
}

#[derive(Deserialize)]
struct WebSearchArgs {
    query: String,
}

#[async_trait]
impl ToolHandler for WebSearchHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        let ToolInvocation {
            session,
            turn,
            payload,
            call_id,
            ..
        } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "web_search handler received unsupported payload".to_string(),
                ));
            }
        };

        let args: WebSearchArgs = serde_json::from_str(&arguments).map_err(|e| {
            FunctionCallError::RespondToModel(format!("failed to parse function arguments: {e:?}"))
        })?;

        // Send event that web search is starting
        session
            .send_event(
                turn.as_ref(),
                EventMsg::WebSearchToolCall(WebSearchToolCallEvent {
                    call_id: call_id.clone(),
                    query: args.query.clone(),
                    provider: self.provider.name().to_string(),
                }),
            )
            .await;

        // Execute the search
        let results = self
            .provider
            .search(&args.query, self.max_results)
            .await
            .map_err(|e| {
                FunctionCallError::RespondToModel(format!(
                    "web search failed: {}. You may need to adjust your query or try again.",
                    e
                ))
            })?;

        // Format results for LLM
        let formatted_output = format_results_for_llm(&results, self.provider.name());

        Ok(ToolOutput::Function {
            content: formatted_output,
            content_items: None,
            success: Some(!results.is_empty()),
        })
    }
}
