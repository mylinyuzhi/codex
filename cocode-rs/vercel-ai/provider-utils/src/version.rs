//! Version constant for the SDK.

/// The version of the Vercel AI SDK Rust implementation.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_exists() {
        // Just verify we can access the version
        assert!(!VERSION.is_empty());
    }
}
