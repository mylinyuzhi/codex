use super::*;
use crate::LanguageModelMessage;
use cocode_protocol::CacheScope;
use cocode_protocol::PromptCacheConfig;
use cocode_protocol::ProviderApi;
use pretty_assertions::assert_eq;

fn default_config() -> PromptCacheConfig {
    PromptCacheConfig {
        enabled: true,
        skip_cache_write: false,
    }
}

fn make_prompt() -> Vec<LanguageModelMessage> {
    vec![
        LanguageModelMessage::system("You are a helpful assistant."),
        LanguageModelMessage::user_text("Hello"),
        LanguageModelMessage::assistant(vec![crate::AssistantContentPart::text("Hi there!")]),
        LanguageModelMessage::user_text("What is 2+2?"),
    ]
}

#[test]
fn test_breakpoint_on_last_message() {
    let config = default_config();
    let mut prompt = make_prompt();
    apply_message_breakpoints(
        &mut prompt,
        &config,
        ProviderApi::Anthropic,
        "claude-sonnet-4",
    );

    // Last message (user "What is 2+2?") should have provider_options
    let last = &prompt[3];
    match last {
        LanguageModelMessage::User {
            provider_options, ..
        } => {
            let opts = provider_options
                .as_ref()
                .expect("should have provider_options");
            let anthropic = opts.0.get("anthropic").expect("should have anthropic key");
            let cc = anthropic
                .get("cacheControl")
                .expect("should have cacheControl");
            assert_eq!(cc, &serde_json::json!({"type": "ephemeral"}));
        }
        _ => panic!("expected User message"),
    }

    // Other messages should NOT have provider_options
    match &prompt[1] {
        LanguageModelMessage::User {
            provider_options, ..
        } => assert!(provider_options.is_none()),
        _ => panic!("expected User message"),
    }
}

#[test]
fn test_skip_cache_write_shifts_breakpoint() {
    let config = PromptCacheConfig {
        enabled: true,
        skip_cache_write: true,
    };
    let mut prompt = make_prompt();
    apply_message_breakpoints(
        &mut prompt,
        &config,
        ProviderApi::Anthropic,
        "claude-sonnet-4",
    );

    // Second-to-last message (assistant "Hi there!") should have provider_options
    match &prompt[2] {
        LanguageModelMessage::Assistant {
            provider_options, ..
        } => {
            assert!(provider_options.is_some());
        }
        _ => panic!("expected Assistant message"),
    }

    // Last message should NOT have provider_options
    match &prompt[3] {
        LanguageModelMessage::User {
            provider_options, ..
        } => assert!(provider_options.is_none()),
        _ => panic!("expected User message"),
    }
}

#[test]
fn test_noop_for_non_anthropic() {
    let config = default_config();
    let mut prompt = make_prompt();
    apply_message_breakpoints(&mut prompt, &config, ProviderApi::Openai, "gpt-4o");

    // No messages should have provider_options
    for msg in &prompt {
        match msg {
            LanguageModelMessage::User {
                provider_options, ..
            } => assert!(provider_options.is_none()),
            LanguageModelMessage::Assistant {
                provider_options, ..
            } => assert!(provider_options.is_none()),
            _ => {}
        }
    }
}

#[test]
fn test_disabled_config() {
    let config = PromptCacheConfig {
        enabled: false,
        skip_cache_write: false,
    };
    let mut prompt = make_prompt();
    apply_message_breakpoints(
        &mut prompt,
        &config,
        ProviderApi::Anthropic,
        "claude-sonnet-4",
    );

    match &prompt[3] {
        LanguageModelMessage::User {
            provider_options, ..
        } => assert!(provider_options.is_none()),
        _ => panic!("expected User message"),
    }
}

#[test]
fn test_empty_prompt() {
    let config = default_config();
    let mut prompt: Vec<LanguageModelMessage> = vec![];
    apply_message_breakpoints(
        &mut prompt,
        &config,
        ProviderApi::Anthropic,
        "claude-sonnet-4",
    );
    assert!(prompt.is_empty());
}

#[test]
fn test_build_cache_provider_options_global_scope() {
    let opts = build_cache_provider_options(Some(CacheScope::Global));
    let opts = opts.expect("should produce options for Global scope");
    let anthropic = opts.0.get("anthropic").expect("should have anthropic key");
    let cc = anthropic
        .get("cacheControl")
        .expect("should have cacheControl");
    let cc_map: serde_json::Map<String, serde_json::Value> =
        serde_json::from_value(cc.clone()).unwrap();
    assert_eq!(cc_map.get("type").unwrap(), "ephemeral");
    assert_eq!(cc_map.get("scope").unwrap(), "global");
}

#[test]
fn test_build_cache_provider_options_org_scope() {
    let opts = build_cache_provider_options(Some(CacheScope::Org));
    let opts = opts.expect("should produce options for Org scope");
    let anthropic = opts.0.get("anthropic").expect("should have anthropic key");
    let cc = anthropic
        .get("cacheControl")
        .expect("should have cacheControl");
    let cc_map: serde_json::Map<String, serde_json::Value> =
        serde_json::from_value(cc.clone()).unwrap();
    assert_eq!(cc_map.get("type").unwrap(), "ephemeral");
    assert!(
        cc_map.get("scope").is_none(),
        "Org scope should not include scope field"
    );
}

#[test]
fn test_build_cache_provider_options_no_scope() {
    let opts = build_cache_provider_options(None);
    assert!(opts.is_none(), "None scope should produce no options");
}

#[test]
fn test_breakpoint_on_tool_message() {
    let config = default_config();
    let mut prompt = vec![
        LanguageModelMessage::system("system"),
        LanguageModelMessage::user_text("hello"),
        LanguageModelMessage::assistant(vec![crate::AssistantContentPart::tool_call(
            "call_1",
            "Read",
            serde_json::json!({}),
        )]),
        LanguageModelMessage::tool(vec![crate::ToolContentPart::ToolResult(
            crate::ToolResultPart::new(
                "call_1",
                "Read",
                crate::ToolResultContent::Text {
                    value: "file content".to_string(),
                    provider_options: None,
                },
            ),
        )]),
    ];
    apply_message_breakpoints(
        &mut prompt,
        &config,
        ProviderApi::Anthropic,
        "claude-sonnet-4",
    );

    // Last message (Tool) should have provider_options
    match &prompt[3] {
        LanguageModelMessage::Tool {
            provider_options, ..
        } => assert!(
            provider_options.is_some(),
            "Tool message should get cache breakpoint"
        ),
        _ => panic!("expected Tool message"),
    }
}

#[test]
fn test_system_only_prompt_no_breakpoint() {
    let config = default_config();
    let mut prompt = vec![LanguageModelMessage::system("system prompt")];
    apply_message_breakpoints(
        &mut prompt,
        &config,
        ProviderApi::Anthropic,
        "claude-sonnet-4",
    );

    // System messages should NOT get breakpoints (they use build_for_cache blocks)
    match &prompt[0] {
        LanguageModelMessage::System {
            provider_options, ..
        } => assert!(provider_options.is_none()),
        _ => panic!("expected System message"),
    }
}

#[test]
fn test_skip_cache_write_with_two_messages() {
    let config = PromptCacheConfig {
        enabled: true,
        skip_cache_write: true,
    };
    // System + User → skip_cache_write shifts to index 0 (System), which is skipped
    let mut prompt = vec![
        LanguageModelMessage::system("system"),
        LanguageModelMessage::user_text("hello"),
    ];
    apply_message_breakpoints(
        &mut prompt,
        &config,
        ProviderApi::Anthropic,
        "claude-sonnet-4",
    );

    // Index 0 is System → skipped, no breakpoint on either message
    match &prompt[0] {
        LanguageModelMessage::System {
            provider_options, ..
        } => assert!(provider_options.is_none()),
        _ => panic!("expected System message"),
    }
    match &prompt[1] {
        LanguageModelMessage::User {
            provider_options, ..
        } => assert!(provider_options.is_none()),
        _ => panic!("expected User message"),
    }
}
