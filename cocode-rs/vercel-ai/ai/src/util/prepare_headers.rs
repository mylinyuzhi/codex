//! Prepare HTTP headers for requests.
//!
//! This module provides utilities for preparing and merging HTTP headers
//! for API requests.

use std::collections::HashMap;

/// Prepare headers for an API request.
///
/// This function combines base headers with additional headers,
/// with additional headers taking precedence.
///
/// # Arguments
///
/// * `base_headers` - The base headers to start with.
/// * `additional_headers` - Additional headers to add/override.
///
/// # Returns
///
/// A combined `HashMap` of headers.
pub fn prepare_headers(
    base_headers: Option<&HashMap<String, String>>,
    additional_headers: Option<&HashMap<String, String>>,
) -> HashMap<String, String> {
    let mut headers = base_headers.cloned().unwrap_or_default();

    if let Some(additional) = additional_headers {
        for (key, value) in additional {
            headers.insert(key.clone(), value.clone());
        }
    }

    headers
}

/// Prepare headers with authentication.
///
/// # Arguments
///
/// * `api_key` - The API key for authentication.
/// * `additional_headers` - Additional headers to include.
///
/// # Returns
///
/// A `HashMap` with authorization and additional headers.
pub fn prepare_headers_with_auth(
    api_key: &str,
    additional_headers: Option<&HashMap<String, String>>,
) -> HashMap<String, String> {
    let mut headers = HashMap::new();
    headers.insert("Authorization".to_string(), format!("Bearer {api_key}"));

    if let Some(additional) = additional_headers {
        for (key, value) in additional {
            headers.insert(key.clone(), value.clone());
        }
    }

    headers
}

/// Prepare headers for a specific provider.
///
/// Different providers may require different header formats.
///
/// # Arguments
///
/// * `provider` - The provider name (e.g., "openai", "anthropic").
/// * `api_key` - The API key.
/// * `additional_headers` - Additional headers.
///
/// # Returns
///
/// Provider-specific headers.
pub fn prepare_provider_headers(
    provider: &str,
    api_key: &str,
    additional_headers: Option<&HashMap<String, String>>,
) -> HashMap<String, String> {
    let mut headers = HashMap::new();

    match provider.to_lowercase().as_str() {
        "anthropic" => {
            headers.insert("x-api-key".to_string(), api_key.to_string());
            headers.insert("anthropic-version".to_string(), "2023-06-01".to_string());
        }
        "openai" => {
            headers.insert("Authorization".to_string(), format!("Bearer {api_key}"));
        }
        "google" | "google-genai" => {
            // Google uses query param for API key, but we can set it here too
            headers.insert("x-goog-api-key".to_string(), api_key.to_string());
        }
        _ => {
            // Default to Bearer auth
            headers.insert("Authorization".to_string(), format!("Bearer {api_key}"));
        }
    }

    // Add content type
    headers.insert("Content-Type".to_string(), "application/json".to_string());

    // Merge additional headers
    if let Some(additional) = additional_headers {
        for (key, value) in additional {
            headers.insert(key.clone(), value.clone());
        }
    }

    headers
}

/// Merge multiple header maps.
///
/// Later maps take precedence over earlier ones.
///
/// # Arguments
///
/// * `header_maps` - Multiple header maps to merge.
///
/// # Returns
///
/// A merged `HashMap`.
pub fn merge_headers(header_maps: &[&HashMap<String, String>]) -> HashMap<String, String> {
    let mut result = HashMap::new();

    for map in header_maps {
        for (key, value) in *map {
            result.insert(key.clone(), value.clone());
        }
    }

    result
}

/// Check if headers contain a specific header (case-insensitive).
///
/// # Arguments
///
/// * `headers` - The headers to search.
/// * `name` - The header name to look for.
///
/// # Returns
///
/// `Some(&String)` if found, `None` otherwise.
pub fn get_header<'a>(headers: &'a HashMap<String, String>, name: &str) -> Option<&'a String> {
    // Try exact match first
    if let Some(value) = headers.get(name) {
        return Some(value);
    }

    // Try case-insensitive match
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prepare_headers_empty() {
        let result = prepare_headers(None, None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_prepare_headers_base_only() {
        let mut base = HashMap::new();
        base.insert("Content-Type".to_string(), "application/json".to_string());

        let result = prepare_headers(Some(&base), None);

        assert_eq!(result.len(), 1);
        assert_eq!(
            result.get("Content-Type"),
            Some(&"application/json".to_string())
        );
    }

    #[test]
    fn test_prepare_headers_combined() {
        let mut base = HashMap::new();
        base.insert("Content-Type".to_string(), "application/json".to_string());

        let mut additional = HashMap::new();
        additional.insert("X-Custom".to_string(), "value".to_string());

        let result = prepare_headers(Some(&base), Some(&additional));

        assert_eq!(result.len(), 2);
        assert_eq!(
            result.get("Content-Type"),
            Some(&"application/json".to_string())
        );
        assert_eq!(result.get("X-Custom"), Some(&"value".to_string()));
    }

    #[test]
    fn test_prepare_headers_override() {
        let mut base = HashMap::new();
        base.insert("X-Key".to_string(), "base-value".to_string());

        let mut additional = HashMap::new();
        additional.insert("X-Key".to_string(), "override-value".to_string());

        let result = prepare_headers(Some(&base), Some(&additional));

        assert_eq!(result.get("X-Key"), Some(&"override-value".to_string()));
    }

    #[test]
    fn test_prepare_headers_with_auth() {
        let result = prepare_headers_with_auth("my-api-key", None);

        assert_eq!(
            result.get("Authorization"),
            Some(&"Bearer my-api-key".to_string())
        );
    }

    #[test]
    fn test_prepare_provider_headers_anthropic() {
        let result = prepare_provider_headers("anthropic", "my-key", None);

        assert_eq!(result.get("x-api-key"), Some(&"my-key".to_string()));
        assert_eq!(
            result.get("anthropic-version"),
            Some(&"2023-06-01".to_string())
        );
        assert_eq!(
            result.get("Content-Type"),
            Some(&"application/json".to_string())
        );
    }

    #[test]
    fn test_prepare_provider_headers_openai() {
        let result = prepare_provider_headers("openai", "my-key", None);

        assert_eq!(
            result.get("Authorization"),
            Some(&"Bearer my-key".to_string())
        );
        assert_eq!(
            result.get("Content-Type"),
            Some(&"application/json".to_string())
        );
    }

    #[test]
    fn test_merge_headers() {
        let mut h1 = HashMap::new();
        h1.insert("A".to_string(), "1".to_string());

        let mut h2 = HashMap::new();
        h2.insert("B".to_string(), "2".to_string());

        let result = merge_headers(&[&h1, &h2]);

        assert_eq!(result.len(), 2);
        assert_eq!(result.get("A"), Some(&"1".to_string()));
        assert_eq!(result.get("B"), Some(&"2".to_string()));
    }

    #[test]
    fn test_get_header_case_insensitive() {
        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        assert_eq!(
            get_header(&headers, "Content-Type"),
            Some(&"application/json".to_string())
        );
        assert_eq!(
            get_header(&headers, "content-type"),
            Some(&"application/json".to_string())
        );
        assert_eq!(
            get_header(&headers, "CONTENT-TYPE"),
            Some(&"application/json".to_string())
        );
    }
}
