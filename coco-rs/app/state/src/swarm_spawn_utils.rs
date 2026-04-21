//! Spawn utilities — CLI flag building and env var inheritance for teammates.
//!
//! TS: utils/swarm/spawnUtils.ts

use super::swarm_backend::TeammateSpawnConfig;
use super::swarm_constants::AGENT_TEAMS_ENV_VAR;
use super::swarm_constants::PLAN_MODE_REQUIRED_ENV_VAR;
use super::swarm_constants::TEAMMATE_COLOR_ENV_VAR;
use coco_config::EnvKey;
use coco_config::env;

/// Get the command used to spawn teammates.
///
/// TS: `getTeammateCommand()` — TEAMMATE_COMMAND env var or process executable.
pub fn get_teammate_command() -> String {
    env::var(super::swarm_constants::TEAMMATE_COMMAND_ENV_VAR).unwrap_or_else(|_| {
        std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "claude".to_string())
    })
}

/// Build the full CLI command to spawn a pane-based teammate.
///
/// TS: `PaneBackendExecutor.spawn()` builds the command string with identity
/// flags + inherited flags + env vars.
pub fn build_teammate_command(config: &TeammateSpawnConfig) -> String {
    let command = get_teammate_command();
    let agent_id = format!("{}@{}", config.name, config.team_name);

    let mut args = vec![
        format!("--agent-id={agent_id}"),
        format!("--agent-name={}", config.name),
        format!("--team-name={}", config.team_name),
        format!("--parent-session-id={}", config.parent_session_id),
    ];

    if let Some(color) = &config.color {
        args.push(format!("--agent-color={}", color.as_str()));
    }

    if config.plan_mode_required {
        args.push("--plan-mode-required".to_string());
    }

    if let Some(model) = &config.model {
        args.push(format!("--model={model}"));
    }

    // Build inherited env vars
    let env_vars = build_inherited_env_vars(config);

    // cd to working directory, then run with env
    format!(
        "cd {cwd} && {env_vars}{command} {args}",
        cwd = config.cwd,
        args = args.join(" ")
    )
}

/// Build inherited CLI flags for teammate processes.
///
/// TS: `buildInheritedCliFlags(options?)`
pub fn build_inherited_cli_flags(config: &TeammateSpawnConfig) -> Vec<String> {
    let mut flags = Vec::new();

    // Permission mode (unless plan mode required)
    if !config.plan_mode_required {
        for perm in &config.permissions {
            flags.push(format!("--permission={perm}"));
        }
    }

    // Model override
    if let Some(model) = &config.model {
        flags.push(format!("--model={model}"));
    }

    // Agent identity
    let agent_id = format!("{}@{}", config.name, config.team_name);
    flags.push(format!("--agent-id={agent_id}"));
    flags.push(format!("--agent-name={}", config.name));
    flags.push(format!("--team-name={}", config.team_name));
    flags.push(format!("--parent-session-id={}", config.parent_session_id));

    if let Some(color) = &config.color {
        flags.push(format!("--agent-color={}", color.as_str()));
    }

    if config.plan_mode_required {
        flags.push("--plan-mode-required".to_string());
    }

    flags
}

/// Build env vars to inherit into teammate processes.
///
/// TS: `buildInheritedEnvVars()`
pub fn build_inherited_env_vars(config: &TeammateSpawnConfig) -> String {
    let mut vars = Vec::new();

    // Always set these
    vars.push(format!("{AGENT_TEAMS_ENV_VAR}=1"));

    // Agent color
    if let Some(color) = &config.color {
        vars.push(format!("{TEAMMATE_COLOR_ENV_VAR}={}", color.as_str()));
    }

    // Plan mode required
    if config.plan_mode_required {
        vars.push(format!("{PLAN_MODE_REQUIRED_ENV_VAR}=1"));
    }

    // Inherit coco runtime env vars. Bedrock / Vertex / Foundry
    // routing vars were removed — re-add here alongside the provider
    // crate when those providers ship.
    for var in &[
        EnvKey::AnthropicBaseUrl,
        EnvKey::CocoConfigDir,
        EnvKey::CocoRemote,
        EnvKey::CocoRemoteMemoryDir,
    ] {
        if let Ok(val) = env::var(*var) {
            vars.push(format!("{var}={val}"));
        }
    }

    // Inherit external provider, proxy, and certificate env vars.
    for var in &[
        // AWS
        "AWS_REGION",
        "AWS_PROFILE",
        "GOOGLE_CLOUD_PROJECT",
        // Proxy
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "NO_PROXY",
        "no_proxy",
        // TLS certificates
        "SSL_CERT_FILE",
        "NODE_EXTRA_CA_CERTS",
        "REQUESTS_CA_BUNDLE",
        "CURL_CA_BUNDLE",
    ] {
        if let Ok(val) = std::env::var(var) {
            vars.push(format!("{var}={val}"));
        }
    }

    if vars.is_empty() {
        String::new()
    } else {
        format!("{} ", vars.join(" "))
    }
}

#[cfg(test)]
#[path = "swarm_spawn_utils.test.rs"]
mod tests;
