//! Domain types for configuration management.
//!
//! This module defines newtypes for API credentials and configuration values
//! that require special handling (e.g., redacted Debug output for security).

use serde::Deserialize;
use serde::Serialize;
use std::fmt::Debug;
use std::fmt::Formatter;
use std::fmt::{self};

/// Secure API key wrapper with redacted Debug output.
///
/// This newtype ensures API keys are never accidentally logged or printed
/// in full. The Debug implementation returns "[REDACTED]" instead of the
/// actual key value, making it safe to use in error messages and traces.
///
/// # Example
///
/// ```
/// use cocode_config::types::ApiKey;
///
/// let key = ApiKey::new("sk-test-key-12345".to_string());
/// assert_eq!(format!("{:?}", key), "ApiKey([REDACTED])");
///
/// // To access the actual key, use expose()
/// assert_eq!(key.expose(), "sk-test-key-12345");
/// ```
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    /// Create a new API key from a string.
    pub fn new(key: String) -> Self {
        Self(key)
    }

    /// Explicitly expose the key for actual use.
    ///
    /// This method makes key usage auditable by requiring an explicit
    /// call to access the actual value. Use sparingly and only where needed.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Convert into the inner string, consuming the wrapper.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl Debug for ApiKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "ApiKey([REDACTED])")
    }
}

impl From<String> for ApiKey {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl AsRef<str> for ApiKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_key_debug_redacted() {
        let key = ApiKey::new("secret-key".to_string());
        let debug_output = format!("{key:?}");
        assert_eq!(debug_output, "ApiKey([REDACTED])");
        assert!(!debug_output.contains("secret-key"));
    }

    #[test]
    fn test_api_key_expose() {
        let key = ApiKey::new("my-secret-key".to_string());
        assert_eq!(key.expose(), "my-secret-key");
    }

    #[test]
    fn test_api_key_from_string() {
        let key = ApiKey::from("test-key".to_string());
        assert_eq!(key.expose(), "test-key");
    }

    #[test]
    fn test_api_key_into_inner() {
        let key = ApiKey::new("inner-key".to_string());
        let inner = key.into_inner();
        assert_eq!(inner, "inner-key");
    }

    #[test]
    fn test_api_key_equality() {
        let key1 = ApiKey::new("same-key".to_string());
        let key2 = ApiKey::new("same-key".to_string());
        let key3 = ApiKey::new("different-key".to_string());

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_api_key_as_ref() {
        let key = ApiKey::new("test-key".to_string());
        assert_eq!(<ApiKey as AsRef<str>>::as_ref(&key), "test-key");
    }
}
