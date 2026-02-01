mod background;
mod context;
mod definition;
pub mod definitions;
mod filter;
mod manager;
pub mod signal;
mod spawn;
mod transcript;

pub use background::BackgroundAgent;
pub use context::ChildToolUseContext;
pub use definition::AgentDefinition;
pub use filter::filter_tools_for_agent;
pub use manager::{AgentExecuteFn, AgentInstance, AgentStatus, SpawnResult, SubagentManager};
pub use signal::{
    backgroundable_agent_ids, is_agent_backgroundable, register_backgroundable_agent,
    trigger_background_transition, unregister_backgroundable_agent,
};
pub use spawn::SpawnInput;
pub use transcript::TranscriptRecorder;
