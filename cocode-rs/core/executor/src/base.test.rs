use super::*;

#[test]
fn test_executor_config_defaults() {
    let config = ExecutorConfig::default();
    assert_eq!(config.max_turns, Some(200));
    assert_eq!(config.context_window, 200_000);
    assert_eq!(config.max_output_tokens, 16_384);
    assert!(config.enable_micro_compaction);
    assert!(config.enable_streaming_tools);
    assert_eq!(config.features, cocode_protocol::Features::with_defaults());
}

#[test]
fn test_builder_defaults() {
    let builder = ExecutorBuilder::new();
    assert!(builder.api_client.is_none());
    assert!(builder.model_hub.is_none());
    assert!(builder.tool_registry.is_none());
    assert!(builder.hooks.is_none());
    assert!(builder.spawn_agent_fn.is_none());
}

#[test]
fn test_builder_configuration() {
    let builder = ExecutorBuilder::new()
        .model(ModelSpec::new("test-provider", "test-model"))
        .max_turns(100)
        .context_window(128000)
        .max_output_tokens(8192)
        .enable_micro_compaction(false)
        .enable_streaming_tools(false);

    assert_eq!(
        builder.config.model,
        ModelSpec::new("test-provider", "test-model")
    );
    assert_eq!(builder.config.max_turns, Some(100));
    assert_eq!(builder.config.context_window, 128000);
    assert_eq!(builder.config.max_output_tokens, 8192);
    assert!(!builder.config.enable_micro_compaction);
    assert!(!builder.config.enable_streaming_tools);
}
