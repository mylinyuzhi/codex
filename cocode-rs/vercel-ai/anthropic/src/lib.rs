//! vercel-ai-anthropic — Anthropic provider for Vercel AI SDK in Rust.
//!
//! This crate implements the Anthropic provider for the Vercel AI SDK v4 specification,
//! providing access to Claude models through a unified interface.
//!
//! # Quick Start
//!
//! ```ignore
//! use vercel_ai_anthropic::{anthropic, AnthropicProviderSettings, create_anthropic};
//!
//! // Default provider (uses ANTHROPIC_API_KEY env var)
//! let provider = anthropic();
//!
//! // Messages API
//! let model = provider.messages("claude-sonnet-4-5");
//! ```

// Foundation
pub mod anthropic_config;
pub mod anthropic_error;
pub mod anthropic_metadata;
pub mod anthropic_provider;

// Model implementations
pub mod messages;

// Provider tools
pub mod tool;

// Cache control and utilities
pub mod cache_control;
pub mod forward_container_id;

// Re-exports
pub use anthropic_config::AnthropicConfig;
pub use anthropic_provider::AnthropicProvider;
pub use anthropic_provider::AnthropicProviderSettings;
pub use anthropic_provider::anthropic;
pub use anthropic_provider::create_anthropic;
pub use cache_control::CacheControlValidator;
pub use forward_container_id::forward_anthropic_container_id_from_last_step;
pub use messages::AnthropicMessagesLanguageModel;
