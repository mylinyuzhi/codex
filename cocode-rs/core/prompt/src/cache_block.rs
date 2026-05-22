//! System prompt blocks with cache scope annotations.

use cocode_protocol::CacheScope;

/// A system prompt block with an associated cache scope.
///
/// Used by `SystemPromptBuilder::build_for_cache()` to split the system
/// prompt into cacheable blocks with different scopes.
pub struct SystemPromptBlock {
    /// The text content of this block.
    pub text: String,
    /// Cache scope for this block.
    ///
    /// - `Some(Global)` → stable content shared across all users
    /// - `Some(Org)` → shared within organization
    /// - `None` → dynamic content, no explicit cache scope
    pub cache_scope: Option<CacheScope>,
}
