//! API key rotation for handling rate-limit errors.
//!
//! When a provider returns a rate-limit error (429), the rotator cycles to the
//! next available API key. This distributes load across multiple keys and helps
//! avoid hitting per-key rate limits.
//!
// TODO: Wire into retry loop when Model trait supports runtime key swapping.
// Currently, the caller must re-create the model via Provider::model() to use
// a different key, since Model has no with_api_key() method.

use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

/// Thread-safe API key rotator.
///
/// Rotates through a list of API keys on each call to [`rotate()`](Self::rotate).
/// Useful for distributing requests across multiple API keys to avoid rate limits.
///
/// # Example
///
/// ```
/// use hyper_sdk::ApiKeyRotator;
///
/// let rotator = ApiKeyRotator::new(vec![
///     "key-1".to_string(),
///     "key-2".to_string(),
///     "key-3".to_string(),
/// ]);
///
/// assert_eq!(rotator.current(), "key-1");
/// assert_eq!(rotator.rotate(), "key-2");
/// assert_eq!(rotator.rotate(), "key-3");
/// assert_eq!(rotator.rotate(), "key-1"); // wraps around
/// ```
#[derive(Debug)]
pub struct ApiKeyRotator {
    keys: Vec<String>,
    index: AtomicUsize,
}

impl ApiKeyRotator {
    /// Create a new rotator with the given keys.
    ///
    /// # Panics
    ///
    /// Panics if `keys` is empty.
    pub fn new(keys: Vec<String>) -> Self {
        assert!(!keys.is_empty(), "ApiKeyRotator requires at least one key");
        Self {
            keys,
            index: AtomicUsize::new(0),
        }
    }

    /// Get the current API key without rotating.
    pub fn current(&self) -> &str {
        let idx = self.index.load(Ordering::Relaxed) % self.keys.len();
        &self.keys[idx]
    }

    /// Rotate to the next API key and return it.
    pub fn rotate(&self) -> &str {
        let new_idx = self.index.fetch_add(1, Ordering::Relaxed) + 1;
        &self.keys[new_idx % self.keys.len()]
    }

    /// Get the number of available keys.
    pub fn key_count(&self) -> usize {
        self.keys.len()
    }

    /// Check if rotation is useful (more than one key).
    pub fn has_alternatives(&self) -> bool {
        self.keys.len() > 1
    }
}

#[cfg(test)]
#[path = "key_rotator.test.rs"]
mod tests;
