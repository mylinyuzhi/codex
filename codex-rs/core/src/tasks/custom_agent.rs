use std::sync::Arc;

use async_trait::async_trait;
use codex_protocol::protocol::Event;
use tokio_util::sync::CancellationToken;

use crate::codex::TurnContext;
use crate::codex_delegate::run_codex_conversation_one_shot;
use crate::state::TaskKind;
use codex_protocol::user_input::UserInput;

use super::SessionTask;
use super::SessionTaskContext;

/// Task that executes a custom configured agent.
///
/// Custom agents are defined in ~/.codex/agents/ or .codex/agents/ as TOML files.
/// They specify system prompts, model overrides, tool restrictions, and other
/// configuration that customizes the agent's behavior.
#[derive(Clone)]
pub(crate) struct CustomAgentTask {
    /// Name of the custom agent (must match a *.toml file)
    pub agent_name: String,
}

#[async_trait]
impl SessionTask for CustomAgentTask {
    fn kind(&self) -> TaskKind {
        TaskKind::CustomAgent
    }

    async fn run(
        self: Arc<Self>,
        session: Arc<SessionTaskContext>,
        ctx: Arc<TurnContext>,
        input: Vec<UserInput>,
        cancellation_token: CancellationToken,
    ) -> Option<String> {
        // Start sub-codex conversation with the custom agent configuration
        let receiver = match start_custom_agent_conversation(
            session.clone(),
            ctx.clone(),
            input,
            cancellation_token.clone(),
            &self.agent_name,
        )
        .await
        {
            Some(receiver) => receiver,
            None => return None,
        };

        // Forward events from subagent to parent session
        while let Ok(event) = receiver.recv().await {
            if cancellation_token.is_cancelled() {
                break;
            }

            // Forward all events to the parent session
            // (approvals are already handled by codex_delegate.rs)
            let sess = session.clone_session();
            sess.send_event(ctx.as_ref(), event.msg).await;
        }

        None
    }
}

async fn start_custom_agent_conversation(
    session: Arc<SessionTaskContext>,
    ctx: Arc<TurnContext>,
    input: Vec<UserInput>,
    cancellation_token: CancellationToken,
    agent_name: &str,
) -> Option<async_channel::Receiver<Event>> {
    let config = ctx.client.config();
    let sub_agent_config = config.as_ref().clone();

    // Configuration will be loaded and applied by codex_delegate.rs
    // based on the SubAgentSource::Other(agent_name)
    (run_codex_conversation_one_shot(
        sub_agent_config,
        session.auth_manager(),
        input,
        session.clone_session(),
        ctx.clone(),
        cancellation_token,
        None,
        codex_protocol::protocol::SubAgentSource::Other(agent_name.to_string()),
    )
    .await)
        .ok()
        .map(|io| io.rx_event)
}
