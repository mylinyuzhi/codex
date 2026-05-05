//! `coco-inference::ApiClient` plumbing smoke test.
//!
//! Verifies fingerprint round-trip and usage accumulation. Builds its
//! own `ApiClient` rather than reusing `LiveTarget.client` so the
//! fingerprint-against-`(provider, model)` assertion stays a real
//! check rather than a tautology against the cached client.

use anyhow::Result;
use coco_inference::LanguageModelMessage;
use coco_inference::QueryParams;

use crate::common::build_client;
use crate::common::usage_report;

/// Builds an `ApiClient` for `(provider, model)` and runs one
/// non-streaming query through it. Asserts: response text non-empty,
/// usage accumulator updated, fingerprint matches the provider name.
pub async fn run(provider: &str, model_id: &str) -> Result<()> {
    let client = build_client(provider, model_id)?;

    assert_eq!(
        client.fingerprint().provider,
        provider,
        "fingerprint provider should match"
    );
    assert_eq!(
        client.fingerprint().api_model_name,
        model_id,
        "fingerprint api_model_name should match"
    );

    let prompt = vec![
        LanguageModelMessage::system("You are a helpful assistant. Be concise."),
        LanguageModelMessage::user_text("Reply with the single word: ok"),
    ];

    let params = QueryParams {
        prompt,
        max_tokens: Some(64),
        thinking_level: None,
        fast_mode: false,
        tools: None,
        context_management: None,
        query_source: Some("coco-tests-live::inference_smoke".into()),
        agent_id: None,
        time_since_last_assistant_ms: None,
        agentic: false,
        cache: None,
    };

    let result = client.query(&params).await?;
    usage_report::record(provider, model_id, "inference_smoke.run", &result.usage);

    let usage = client.accumulated_usage().await;
    assert!(
        usage.total.input_tokens > 0,
        "{provider}/{model_id}: usage accumulator did not record input tokens"
    );
    assert!(
        usage.total.output_tokens > 0,
        "{provider}/{model_id}: usage accumulator did not record output tokens"
    );
    assert!(
        result.usage.input_tokens > 0,
        "{provider}/{model_id}: per-call input tokens should be > 0"
    );
    assert!(
        !result.content.is_empty(),
        "{provider}/{model_id}: response content was empty"
    );
    Ok(())
}
