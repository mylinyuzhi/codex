//! Plan slug generation for unique plan file naming.
//!
//! Generates slugs in the format `{adjective}-{action}-{noun}`.
//! Total combinations: 168 x 87 x 235 = 3,436,980

use std::collections::HashMap;
use std::sync::Mutex;

use crate::plan_slug_words::ACTIONS;
use crate::plan_slug_words::ADJECTIVES;
use crate::plan_slug_words::NOUNS;
use once_cell::sync::Lazy;
use rand::Rng;

/// Maximum retry attempts for collision detection.
const MAX_SLUG_RETRIES: i32 = 10;

/// Session-based slug cache to prevent regeneration.
static SLUG_CACHE: Lazy<Mutex<HashMap<String, String>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// Generate a random slug in the format `{adjective}-{action}-{noun}`.
pub fn generate_slug() -> String {
    let mut rng = rand::rng();
    let adj = ADJECTIVES[rng.random_range(0..ADJECTIVES.len())];
    let action = ACTIONS[rng.random_range(0..ACTIONS.len())];
    let noun = NOUNS[rng.random_range(0..NOUNS.len())];
    format!("{adj}-{action}-{noun}")
}

/// Get or generate a unique slug for a session.
///
/// Uses session-based caching to ensure the same slug is returned
/// for the same session ID. Performs collision detection with up to
/// `MAX_SLUG_RETRIES` attempts.
///
/// # Arguments
///
/// * `session_id` - The session identifier for caching
/// * `existing_slugs` - Optional set of existing slugs to avoid collisions
///
/// # Returns
///
/// The cached slug if one exists for this session, otherwise a new unique slug.
pub fn get_unique_slug(session_id: &str, existing_slugs: Option<&[String]>) -> String {
    let mut cache = SLUG_CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    // Check cache first
    if let Some(slug) = cache.get(session_id) {
        return slug.clone();
    }

    // Generate new slug with collision detection
    let existing = existing_slugs.unwrap_or(&[]);
    let mut attempts = 0;
    let slug = loop {
        let candidate = generate_slug();
        if !existing.contains(&candidate) || attempts >= MAX_SLUG_RETRIES {
            break candidate;
        }
        attempts += 1;
    };

    // Cache and return
    cache.insert(session_id.to_string(), slug.clone());
    slug
}

/// Clear the slug cache for testing purposes.
#[doc(hidden)]
pub fn clear_slug_cache() {
    let mut cache = SLUG_CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    cache.clear();
}

#[cfg(test)]
#[path = "plan_slug.test.rs"]
mod tests;
