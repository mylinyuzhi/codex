//! Session creation from SDK start parameters.
//!
//! Extracted from `processor.rs` to keep modules under 500 LoC.

use std::path::PathBuf;
use std::sync::Arc;

use cocode_app_server_protocol::SessionStartRequestParams;
use cocode_config::ConfigManager;
use cocode_config::ConfigOverrides;
use cocode_protocol::SandboxMode;
use cocode_session::Session;
use cocode_session::SessionState;

use crate::mcp_bridge::SdkMcpBridge;
use crate::permission::SdkPermissionBridge;
use crate::session_builder;
use crate::session_builder::SdkHookBridge;

/// Per-connection session handle with runtime state.
pub struct SessionHandle {
    pub state: SessionState,
    pub hook_bridge: Option<Arc<SdkHookBridge>>,
    /// MCP bridge for routing SDK-managed tool calls (if SDK tools were registered).
    pub mcp_bridge: Option<Arc<SdkMcpBridge>>,
    /// The permission bridge for the currently-running turn (if any).
    /// Stored here so the processor can route `ApprovalResolve` to it.
    pub permission_bridge: Option<Arc<SdkPermissionBridge>>,
    pub turn_number: i32,
}

/// Create a `SessionState` from SDK start parameters.
pub async fn create_session(
    config: &ConfigManager,
    params: &SessionStartRequestParams,
) -> anyhow::Result<(SessionState, session_builder::SdkParamsResult)> {
    let working_dir = params
        .cwd
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let mut overrides = ConfigOverrides::default().with_cwd(working_dir.clone());

    if let Some(ref sandbox) = params.sandbox {
        let mode = match sandbox.mode {
            cocode_app_server_protocol::SandboxMode::None => SandboxMode::FullAccess,
            cocode_app_server_protocol::SandboxMode::ReadOnly => SandboxMode::ReadOnly,
            cocode_app_server_protocol::SandboxMode::Strict => SandboxMode::ReadOnly,
        };
        overrides.sandbox_mode = Some(mode);
    }

    let snapshot = Arc::new(config.build_config(overrides)?);
    let selections = config.build_all_selections();
    let mut session = Session::with_selections(working_dir, selections);

    if let Some(max) = params.max_turns {
        session.set_max_turns(Some(max));
    }

    let mut state = SessionState::new(session, snapshot)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if let Some(ref system_prompt) = params.system_prompt {
        match system_prompt {
            cocode_app_server_protocol::SystemPromptConfig::Raw(prompt) => {
                state.set_system_prompt_override(prompt.clone());
            }
            cocode_app_server_protocol::SystemPromptConfig::Structured { append, .. } => {
                if let Some(text) = append {
                    state.set_system_prompt_suffix(text.clone());
                }
            }
        }
    } else if let Some(ref suffix) = params.system_prompt_suffix {
        state.set_system_prompt_suffix(suffix.clone());
    }

    if let Some(ref mode) = params.permission_mode {
        state.set_permission_mode_from_str(mode);
    }
    if let Some(ref model) = params.model {
        state.set_model_override(model);
    }

    let sdk_result = session_builder::apply_sdk_params(&mut state, params).await?;
    Ok((state, sdk_result))
}
