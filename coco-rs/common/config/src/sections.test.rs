use pretty_assertions::assert_eq;

use super::*;
use crate::EnvKey;
use crate::EnvSnapshot;
use crate::settings::Settings;

#[test]
fn test_bash_config_finalize_clamps_max_output_bytes() {
    let settings = Settings {
        tool: PartialToolSettings {
            bash: Some(PartialBashSettings {
                max_output_bytes: Some(999_999),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let config = ToolConfig::resolve(&settings, &EnvSnapshot::default());
    assert_eq!(
        config.bash.max_output_bytes,
        crate::sections::BASH_MAX_OUTPUT_BYTES_UPPER
    );
}

#[test]
fn test_bash_config_finalize_rejects_negative_max_output_bytes() {
    let settings = Settings {
        tool: PartialToolSettings {
            bash: Some(PartialBashSettings {
                max_output_bytes: Some(-5),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let config = ToolConfig::resolve(&settings, &EnvSnapshot::default());
    assert_eq!(config.bash.max_output_bytes, 0);
}

#[test]
fn test_tool_config_json_first_env_override() {
    let settings = Settings {
        tool: PartialToolSettings {
            max_tool_concurrency: Some(4),
            glob_timeout_seconds: Some(12),
            bash: Some(PartialBashSettings {
                auto_background_on_timeout: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let env = EnvSnapshot::from_pairs([
        (EnvKey::CocoMaxToolUseConcurrency, "8"),
        (EnvKey::CocoBashAutoBackgroundOnTimeout, "1"),
    ]);

    let config = ToolConfig::resolve(&settings, &env);

    assert_eq!(config.max_tool_concurrency, 8);
    assert_eq!(config.glob_timeout_seconds, 12);
    assert!(config.bash.auto_background_on_timeout);
}

#[test]
fn test_memory_config_env_disables_memory() {
    let settings = Settings {
        memory: PartialMemorySettings {
            enabled: Some(true),
            extraction_enabled: Some(true),
            ..Default::default()
        },
        ..Default::default()
    };
    let env = EnvSnapshot::from_pairs([(EnvKey::CocoDisableAutoMemory, "yes")]);

    let config = MemoryConfig::resolve(&settings, &env);

    assert!(!config.enabled);
    assert!(!config.extraction_enabled);
}

#[test]
fn test_sandbox_config_json_first_env_override() {
    let settings = Settings {
        sandbox: PartialSandboxSettings {
            enabled: Some(false),
            mode: Some(coco_types::SandboxMode::ReadOnly),
            ..Default::default()
        },
        ..Default::default()
    };
    let env = EnvSnapshot::from_pairs([
        (EnvKey::CocoSandboxEnabled, "1"),
        (EnvKey::CocoSandboxMode, "workspace-write"),
    ]);

    let config = SandboxConfig::resolve(&settings, &env);

    assert!(config.enabled);
    assert_eq!(config.mode, coco_types::SandboxMode::WorkspaceWrite);
}

#[test]
fn test_mcp_runtime_config_json_first_env_override() {
    let settings = Settings {
        mcp_runtime: PartialMcpRuntimeSettings {
            tool_timeout_ms: Some(5_000),
        },
        ..Default::default()
    };
    let env = EnvSnapshot::from_pairs([(EnvKey::CocoMcpToolTimeoutMs, "2500")]);

    let config = McpRuntimeConfig::resolve(&settings, &env);

    assert_eq!(config.tool_timeout_ms, Some(2_500));
}
