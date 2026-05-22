//! Shared types module (V4).
//!
//! Types that are shared across different model types.

mod provider_metadata;
mod provider_options;
mod warning;

pub use provider_metadata::ProviderMetadata;
pub use provider_options::ProviderOptions;
pub use warning::Warning;
