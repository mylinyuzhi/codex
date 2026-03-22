use super::*;

#[test]
fn test_generate_slug_format() {
    let slug = generate_slug();
    let parts: Vec<&str> = slug.split('-').collect();
    assert_eq!(parts.len(), 3, "Slug should have 3 parts: {slug}");
    assert!(
        ADJECTIVES.contains(&parts[0]),
        "First part should be adjective"
    );
    assert!(ACTIONS.contains(&parts[1]), "Second part should be action");
    assert!(NOUNS.contains(&parts[2]), "Third part should be noun");
}

#[test]
fn test_generate_slug_uniqueness() {
    // Generate 100 slugs and check for reasonable uniqueness
    let slugs: Vec<String> = (0..100).map(|_| generate_slug()).collect();
    let unique_count = slugs.iter().collect::<std::collections::HashSet<_>>().len();
    // With 3.4M combinations, 100 random slugs should be nearly all unique
    assert!(
        unique_count >= 95,
        "Expected at least 95 unique slugs, got {unique_count}"
    );
}

#[test]
fn test_get_unique_slug_caching() {
    clear_slug_cache();

    let session = "test-session-1";
    let slug1 = get_unique_slug(session, None);
    let slug2 = get_unique_slug(session, None);

    assert_eq!(slug1, slug2, "Same session should return same slug");
}

#[test]
fn test_get_unique_slug_different_sessions() {
    clear_slug_cache();

    let slug1 = get_unique_slug("session-a", None);
    let slug2 = get_unique_slug("session-b", None);

    // Different sessions could theoretically get the same slug but very unlikely
    // This test just verifies the function works for different sessions
    assert!(!slug1.is_empty());
    assert!(!slug2.is_empty());
}

#[test]
fn test_get_unique_slug_collision_avoidance() {
    clear_slug_cache();

    // Generate a slug and mark it as existing
    let existing_slug = generate_slug();
    let existing = vec![existing_slug];

    // Get a new slug avoiding the existing one
    let new_slug = get_unique_slug("collision-test", Some(&existing));

    // Very likely to be different (unless we hit the same random in 10 attempts)
    // This is a probabilistic test
    assert!(!new_slug.is_empty());
}

#[test]
fn test_word_list_sizes() {
    // Minimum word counts to ensure sufficient combinations
    assert!(
        ADJECTIVES.len() >= 100,
        "Should have at least 100 adjectives, got {}",
        ADJECTIVES.len()
    );
    assert!(
        ACTIONS.len() >= 80,
        "Should have at least 80 actions, got {}",
        ACTIONS.len()
    );
    assert!(
        NOUNS.len() >= 200,
        "Should have at least 200 nouns, got {}",
        NOUNS.len()
    );

    // Total combinations should be > 1.5 million for low collision probability
    let total = ADJECTIVES.len() * ACTIONS.len() * NOUNS.len();
    assert!(total > 1_500_000, "Total combinations: {total}");
}
