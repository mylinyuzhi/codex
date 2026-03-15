//! Provider-specific tool definitions for the OpenAI Responses API.
//!
//! Each tool creates a `LanguageModelV4ProviderTool` that can be passed
//! to the Responses API.

pub mod web_search;

pub use web_search::openai_web_search_tool;
