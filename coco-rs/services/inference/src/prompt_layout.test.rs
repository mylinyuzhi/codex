use pretty_assertions::assert_eq;
use vercel_ai_provider::ProviderOptions;
use vercel_ai_provider::Warning;

use super::*;

#[test]
fn test_put_take_layout_options_round_trips_namespace_fields() {
    let layout = PromptLayoutOptions {
        instructions: Some("base".to_string()),
        system_blocks: Some(vec![AnthropicSystemBlock {
            text: "system".to_string(),
            cache_control: Some(AnthropicCacheControl {
                type_name: "ephemeral".to_string(),
                ttl: Some("5m".to_string()),
            }),
        }]),
        system_instruction: Some("gemini".to_string()),
        layout_warnings: vec![Warning::other("dropped part")],
        prompt_hash_inputs: Some(PromptHashInputs {
            system_text_hash: 1,
            cache_control_hash: Some(2),
            developer_text_hash: Some(3),
            contextual_user_text_hash: 4,
            contextual_user_char_count: 5,
            tools_hash: 6,
            per_tool_hashes: vec![("read".to_string(), 7)],
        }),
    };

    let mut opts = ProviderOptions::default();
    put_layout_options(&mut opts, &layout);

    let namespace = opts
        .get(PROMPT_LAYOUT_NAMESPACE)
        .expect("prompt layout namespace");
    assert!(namespace.contains_key("instructions"));
    assert!(namespace.contains_key("system_blocks"));
    assert!(namespace.contains_key("system_instruction"));
    assert!(namespace.contains_key("layout_warnings"));
    assert!(namespace.contains_key("prompt_hash_inputs"));

    assert_eq!(take_layout_options(&opts), Some(layout));
}

#[test]
fn test_put_layout_options_omits_absent_slots() {
    let layout = PromptLayoutOptions {
        instructions: Some("base".to_string()),
        ..Default::default()
    };

    let mut opts = ProviderOptions::default();
    put_layout_options(&mut opts, &layout);

    let namespace = opts
        .get(PROMPT_LAYOUT_NAMESPACE)
        .expect("prompt layout namespace");
    assert!(namespace.contains_key("instructions"));
    assert!(!namespace.contains_key("system_blocks"));
    assert!(!namespace.contains_key("system_instruction"));
    assert!(!namespace.contains_key("prompt_hash_inputs"));
}
