//! Live integration tests for codex-api adapters.
//!
//! These tests run against real LLM provider APIs and require credentials
//! configured in `.env.test` or via environment variables.
//!
//! # Running Tests
//!
//! ```bash
//! # Run all integration tests (all configured providers)
//! cargo test -p codex-api --test live --ignored -- --test-threads=1
//!
//! # Run tests for a specific provider
//! cargo test -p codex-api --test live --ignored genai -- --test-threads=1
//! cargo test -p codex-api --test live --ignored anthropic -- --test-threads=1
//! cargo test -p codex-api --test live --ignored openai -- --test-threads=1
//! cargo test -p codex-api --test live --ignored volc_ark -- --test-threads=1
//! cargo test -p codex-api --test live --ignored zai -- --test-threads=1
//!
//! # Run a specific test category within a provider
//! cargo test -p codex-api --test live --ignored genai::test_tool -- --test-threads=1
//! cargo test -p codex-api --test live --ignored anthropic::test_text -- --test-threads=1
//! ```
//!
//! # Configuration
//!
//! Set environment variables for each provider:
//! - `CODEX_API_TEST_{PROVIDER}_API_KEY` - Required
//! - `CODEX_API_TEST_{PROVIDER}_MODEL` - Required
//! - `CODEX_API_TEST_{PROVIDER}_BASE_URL` - Optional
//!
//! Or use a `.env.test` file in the crate root.

mod common;

// Provider-specific test modules
#[path = "live/anthropic.rs"]
mod anthropic;
#[path = "live/genai.rs"]
mod genai;
#[path = "live/openai.rs"]
mod openai;
#[path = "live/volc_ark.rs"]
mod volc_ark;
#[path = "live/zai.rs"]
mod zai;

// ============================================================================
// Configuration Tests
// ============================================================================

#[test]
fn test_list_configured_providers() {
    let providers = common::config::list_configured_providers();
    eprintln!("Configured providers: {:?}", providers);
    // This test just verifies the function doesn't panic
}

#[test]
fn test_all_builtin_adapters_available() {
    // Verify all built-in adapters are registered
    assert!(common::get_adapter("genai").is_some());
    assert!(common::get_adapter("anthropic").is_some());
    assert!(common::get_adapter("volc_ark").is_some());
    assert!(common::get_adapter("zai").is_some());
}
