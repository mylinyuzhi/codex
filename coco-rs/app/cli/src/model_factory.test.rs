//! Unit tests for the ModelSpec → LanguageModelV4 dispatch factory.
//!
//! These don't make real network calls — they just verify the dispatch
//! routes each `ProviderApi` variant to the right provider crate and
//! returns an `Arc<dyn LanguageModelV4>` without panicking.

use super::*;

fn spec(api: ProviderApi, model_id: &str) -> ModelSpec {
    ModelSpec {
        provider: format!("{api:?}").to_lowercase(),
        api,
        model_id: model_id.into(),
        display_name: model_id.into(),
    }
}

#[test]
fn build_anthropic_succeeds() {
    let s = spec(ProviderApi::Anthropic, "claude-opus-4-6");
    let result = build_language_model_from_spec(&s);
    assert!(
        result.is_ok(),
        "anthropic factory must succeed (err: {:?})",
        result.err().map(|e| e.to_string())
    );
}

#[test]
fn build_openai_succeeds() {
    let s = spec(ProviderApi::Openai, "gpt-4o-mini");
    let result = build_language_model_from_spec(&s);
    assert!(
        result.is_ok(),
        "openai factory must succeed (err: {:?})",
        result.err().map(|e| e.to_string())
    );
}

#[test]
fn build_gemini_dispatches_to_google_provider() {
    // The Google provider validates its API key at construction time,
    // so without `GOOGLE_GENERATIVE_AI_API_KEY` this returns an error
    // — but the error proves dispatch went to the right provider
    // (message mentions "google" or an API key issue). Anthropic /
    // OpenAI construct lazily so their tests assert `is_ok`.
    let s = spec(ProviderApi::Gemini, "gemini-2.5-flash");
    let result = build_language_model_from_spec(&s);
    match result {
        Ok(_) => {}
        Err(e) => {
            let s = e.to_string().to_lowercase();
            assert!(
                s.contains("google") || s.contains("api key"),
                "gemini dispatch must route to google provider; got: {s}"
            );
        }
    }
}

#[test]
fn build_volcengine_rejects_with_clear_error() {
    let s = spec(ProviderApi::Volcengine, "doubao-1.5");
    let err = build_language_model_from_spec(&s)
        .map(|_| ())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("Volcengine"),
        "error names the provider: {err}"
    );
}

#[test]
fn build_openai_compat_rejects_with_clear_error() {
    let s = spec(ProviderApi::OpenaiCompat, "local-model");
    let err = build_language_model_from_spec(&s)
        .map(|_| ())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("OpenaiCompat"),
        "error names the provider: {err}"
    );
}
