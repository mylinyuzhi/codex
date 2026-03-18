//! Live integration tests for vercel-ai providers.
//!
//! These tests run against real LLM provider APIs and require credentials
//! configured in `.env.test` or via environment variables.
//!
//! # Test Organization
//!
//! Tests are organized in a parameterized suite structure where the same test
//! logic runs against multiple providers. Each test function follows the pattern:
//! `test_{feature}_{provider}`.
//!
//! # Running Tests
//!
//! ```bash
//! # Run all integration tests (all configured providers)
//! cargo test -p vercel-ai --test live -- --test-threads=1
//!
//! # Run all tests for a specific provider
//! cargo test -p vercel-ai --test live openai -- --test-threads=1
//! cargo test -p vercel-ai --test live anthropic -- --test-threads=1
//!
//! # Run specific test category
//! cargo test -p vercel-ai --test live test_basic -- --test-threads=1
//! cargo test -p vercel-ai --test live test_tools -- --test-threads=1
//!
//! # Run specific provider + feature
//! cargo test -p vercel-ai --test live test_basic_openai -- --test-threads=1
//! cargo test -p vercel-ai --test live test_streaming_anthropic -- --test-threads=1
//! ```
//!
//! # Configuration
//!
//! Set environment variables for each provider:
//! - `VERCEL_AI_TEST_{PROVIDER}_API_KEY` - Required
//! - `VERCEL_AI_TEST_{PROVIDER}_MODEL` - Required
//! - `VERCEL_AI_TEST_{PROVIDER}_BASE_URL` - Optional
//!
//! Or use a `.env.test` file in the crate root.
//!
//! # Capability Gating
//!
//! Control which test categories run via environment variables:
//!
//! ```bash
//! # Global: only run text and streaming tests for all providers
//! VERCEL_AI_TEST_CAPABILITIES=text,streaming
//!
//! # Per-provider: override global setting
//! VERCEL_AI_TEST_OPENAI_CAPABILITIES=text,streaming,tools,vision
//! VERCEL_AI_TEST_ANTHROPIC_CAPABILITIES=text,tools
//! ```
//!
//! Available capabilities: `text`, `streaming`, `tools`, `vision`, `cross_provider`.
//! If no capability env vars are set, all capabilities are enabled by default.

mod common;

// Test suite modules
#[path = "live/suite/mod.rs"]
mod suite;

use anyhow::Result;

// ============================================================================
// Helper Macro for Test Generation
// ============================================================================

/// Macro to generate a test function for a specific provider.
///
/// Usage:
/// - `provider_test!(provider_name, test_fn)` — no capability check
/// - `provider_test!(provider_name, "capability", test_fn)` — with capability gate
macro_rules! provider_test {
    ($provider:expr, $test_fn:path) => {{
        let (_provider, model) = require_provider!($provider);
        $test_fn(&model).await
    }};
    ($provider:expr, $capability:expr, $test_fn:path) => {{
        let (_provider, model) = require_provider!($provider, $capability);
        $test_fn(&model).await
    }};
}

// ============================================================================
// Basic Text Generation Tests (All Providers)
// ============================================================================

#[tokio::test]
async fn test_basic_openai() -> Result<()> {
    provider_test!("openai", "text", suite::basic::run)
}

#[tokio::test]
async fn test_basic_anthropic() -> Result<()> {
    provider_test!("anthropic", "text", suite::basic::run)
}

#[tokio::test]
async fn test_basic_google() -> Result<()> {
    provider_test!("google", "text", suite::basic::run)
}

// ============================================================================
// Token Usage Tests (Providers with usage reporting)
// ============================================================================

#[tokio::test]
async fn test_token_usage_openai() -> Result<()> {
    provider_test!("openai", "text", suite::basic::run_token_usage)
}

#[tokio::test]
async fn test_token_usage_anthropic() -> Result<()> {
    provider_test!("anthropic", "text", suite::basic::run_token_usage)
}

#[tokio::test]
async fn test_token_usage_google() -> Result<()> {
    provider_test!("google", "text", suite::basic::run_token_usage)
}

// ============================================================================
// Multi-Turn Conversation Tests (Providers with context preservation)
// ============================================================================

#[tokio::test]
async fn test_multi_turn_openai() -> Result<()> {
    provider_test!("openai", "text", suite::basic::run_multi_turn)
}

#[tokio::test]
async fn test_multi_turn_anthropic() -> Result<()> {
    provider_test!("anthropic", "text", suite::basic::run_multi_turn)
}

#[tokio::test]
async fn test_multi_turn_google() -> Result<()> {
    provider_test!("google", "text", suite::basic::run_multi_turn)
}

// ============================================================================
// Tool Calling Tests (All Providers)
// ============================================================================

#[tokio::test]
async fn test_tools_openai() -> Result<()> {
    provider_test!("openai", "tools", suite::tools::run)
}

#[tokio::test]
async fn test_tools_anthropic() -> Result<()> {
    provider_test!("anthropic", "tools", suite::tools::run)
}

#[tokio::test]
async fn test_tools_google() -> Result<()> {
    provider_test!("google", "tools", suite::tools::run)
}

// ============================================================================
// Tool Complete Flow Tests (All Providers)
// ============================================================================

#[tokio::test]
async fn test_tool_flow_openai() -> Result<()> {
    provider_test!("openai", "tools", suite::tools::run_complete_flow)
}

#[tokio::test]
async fn test_tool_flow_anthropic() -> Result<()> {
    provider_test!("anthropic", "tools", suite::tools::run_complete_flow)
}

#[tokio::test]
async fn test_tool_flow_google() -> Result<()> {
    provider_test!("google", "tools", suite::tools::run_complete_flow)
}

// ============================================================================
// Streaming Tests (All Providers)
// ============================================================================

#[tokio::test]
async fn test_streaming_openai() -> Result<()> {
    provider_test!("openai", "streaming", suite::streaming::run)
}

#[tokio::test]
async fn test_streaming_anthropic() -> Result<()> {
    provider_test!("anthropic", "streaming", suite::streaming::run)
}

#[tokio::test]
async fn test_streaming_google() -> Result<()> {
    provider_test!("google", "streaming", suite::streaming::run)
}

// ============================================================================
// Streaming with Tools Tests
// ============================================================================

#[tokio::test]
async fn test_streaming_tools_openai() -> Result<()> {
    provider_test!("openai", "tools", suite::streaming::run_with_tools)
}

#[tokio::test]
async fn test_streaming_tools_anthropic() -> Result<()> {
    provider_test!("anthropic", "tools", suite::streaming::run_with_tools)
}

#[tokio::test]
async fn test_streaming_tools_google() -> Result<()> {
    provider_test!("google", "tools", suite::streaming::run_with_tools)
}

// ============================================================================
// StreamProcessor Tests (All Providers)
// ============================================================================

#[tokio::test]
async fn test_stream_processor_collect_openai() -> Result<()> {
    provider_test!("openai", "streaming", suite::stream_processor::run_collect)
}

#[tokio::test]
async fn test_stream_processor_collect_anthropic() -> Result<()> {
    provider_test!(
        "anthropic",
        "streaming",
        suite::stream_processor::run_collect
    )
}

#[tokio::test]
async fn test_stream_processor_collect_google() -> Result<()> {
    provider_test!("google", "streaming", suite::stream_processor::run_collect)
}

#[tokio::test]
async fn test_stream_processor_into_text_openai() -> Result<()> {
    provider_test!(
        "openai",
        "streaming",
        suite::stream_processor::run_into_text
    )
}

#[tokio::test]
async fn test_stream_processor_into_text_anthropic() -> Result<()> {
    provider_test!(
        "anthropic",
        "streaming",
        suite::stream_processor::run_into_text
    )
}

#[tokio::test]
async fn test_stream_processor_into_text_google() -> Result<()> {
    provider_test!(
        "google",
        "streaming",
        suite::stream_processor::run_into_text
    )
}

#[tokio::test]
async fn test_stream_processor_incremental_openai() -> Result<()> {
    provider_test!(
        "openai",
        "streaming",
        suite::stream_processor::run_next_incremental
    )
}

#[tokio::test]
async fn test_stream_processor_incremental_anthropic() -> Result<()> {
    provider_test!(
        "anthropic",
        "streaming",
        suite::stream_processor::run_next_incremental
    )
}

#[tokio::test]
async fn test_stream_processor_incremental_google() -> Result<()> {
    provider_test!(
        "google",
        "streaming",
        suite::stream_processor::run_next_incremental
    )
}

#[tokio::test]
async fn test_stream_processor_usage_openai() -> Result<()> {
    provider_test!("openai", "streaming", suite::stream_processor::run_usage)
}

#[tokio::test]
async fn test_stream_processor_usage_anthropic() -> Result<()> {
    provider_test!("anthropic", "streaming", suite::stream_processor::run_usage)
}

#[tokio::test]
async fn test_stream_processor_usage_google() -> Result<()> {
    provider_test!("google", "streaming", suite::stream_processor::run_usage)
}

#[tokio::test]
async fn test_stream_processor_tools_openai() -> Result<()> {
    provider_test!("openai", "tools", suite::stream_processor::run_tool_calls)
}

#[tokio::test]
async fn test_stream_processor_tools_anthropic() -> Result<()> {
    provider_test!(
        "anthropic",
        "tools",
        suite::stream_processor::run_tool_calls
    )
}

#[tokio::test]
async fn test_stream_processor_tools_google() -> Result<()> {
    provider_test!("google", "tools", suite::stream_processor::run_tool_calls)
}

#[tokio::test]
async fn test_stream_processor_multi_turn_openai() -> Result<()> {
    provider_test!(
        "openai",
        "streaming",
        suite::stream_processor::run_multi_turn
    )
}

#[tokio::test]
async fn test_stream_processor_multi_turn_anthropic() -> Result<()> {
    provider_test!(
        "anthropic",
        "streaming",
        suite::stream_processor::run_multi_turn
    )
}

#[tokio::test]
async fn test_stream_processor_multi_turn_google() -> Result<()> {
    provider_test!(
        "google",
        "streaming",
        suite::stream_processor::run_multi_turn
    )
}

// ============================================================================
// Vision / Image Understanding Tests (All Providers)
// ============================================================================

#[tokio::test]
async fn test_vision_openai() -> Result<()> {
    provider_test!("openai", "vision", suite::vision::run)
}

#[tokio::test]
async fn test_vision_anthropic() -> Result<()> {
    provider_test!("anthropic", "vision", suite::vision::run)
}

#[tokio::test]
async fn test_vision_google() -> Result<()> {
    provider_test!("google", "vision", suite::vision::run)
}

// ============================================================================
// Cross-Provider Conversation Tests
// ============================================================================

/// Macro to generate cross-provider test functions.
///
/// Checks that both providers are configured and have the `cross_provider` capability.
macro_rules! cross_provider_test {
    ($source:expr, $target:expr, $test_fn:path) => {{
        let source_cfg = match common::load_test_config($source) {
            Some(cfg) if cfg.enabled => cfg,
            _ => {
                eprintln!(
                    "Skipping cross-provider test: source provider '{}' not configured",
                    $source
                );
                return Ok(());
            }
        };
        if !source_cfg.has_capability("cross_provider") {
            eprintln!(
                "Skipping cross-provider test: capability 'cross_provider' not enabled for '{}'",
                $source
            );
            return Ok(());
        }
        let target_cfg = match common::load_test_config($target) {
            Some(cfg) if cfg.enabled => cfg,
            _ => {
                eprintln!(
                    "Skipping cross-provider test: target provider '{}' not configured",
                    $target
                );
                return Ok(());
            }
        };
        if !target_cfg.has_capability("cross_provider") {
            eprintln!(
                "Skipping cross-provider test: capability 'cross_provider' not enabled for '{}'",
                $target
            );
            return Ok(());
        }

        let (_, source_model) = match common::create_provider_and_model(&source_cfg) {
            Some(pair) => pair,
            None => {
                eprintln!("Skipping: failed to create source provider '{}'", $source);
                return Ok(());
            }
        };
        let (_, target_model) = match common::create_provider_and_model(&target_cfg) {
            Some(pair) => pair,
            None => {
                eprintln!("Skipping: failed to create target provider '{}'", $target);
                return Ok(());
            }
        };

        $test_fn(&source_model, &target_model).await
    }};
}

#[tokio::test]
async fn test_cross_provider_openai_to_anthropic() -> Result<()> {
    cross_provider_test!("openai", "anthropic", suite::cross_provider::run)
}

#[tokio::test]
async fn test_cross_provider_anthropic_to_openai() -> Result<()> {
    cross_provider_test!("anthropic", "openai", suite::cross_provider::run)
}

#[tokio::test]
async fn test_cross_provider_openai_to_google() -> Result<()> {
    cross_provider_test!("openai", "google", suite::cross_provider::run)
}

#[tokio::test]
async fn test_cross_provider_google_to_anthropic() -> Result<()> {
    cross_provider_test!("google", "anthropic", suite::cross_provider::run)
}

#[tokio::test]
async fn test_cross_provider_streaming_openai_to_anthropic() -> Result<()> {
    cross_provider_test!("openai", "anthropic", suite::cross_provider::run_streaming)
}

// ============================================================================
// Configuration Tests
// ============================================================================

#[test]
fn test_list_configured_providers() {
    let providers = common::config::list_configured_providers();
    eprintln!("Configured providers: {providers:?}");
}
