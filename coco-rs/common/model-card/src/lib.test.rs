use super::*;
use pretty_assertions::assert_eq;

#[test]
fn canonical_id_lookup() {
    let card = lookup("claude-opus-4-7").expect("canonical id should resolve");
    assert_eq!(card.canonical_id, "claude-opus-4-7");
    assert_eq!(
        card.knowledge_cutoff.map(|c| c.display),
        Some("January 2026")
    );
}

#[test]
fn alias_resolves_to_same_card() {
    let canonical = lookup("claude-opus-4-7").unwrap();
    let alias_bracket = lookup("claude-opus-4-7[1m]").unwrap();
    let alias_dash = lookup("claude-opus-4-7-1m").unwrap();
    assert_eq!(canonical.canonical_id, alias_bracket.canonical_id);
    assert_eq!(canonical.canonical_id, alias_dash.canonical_id);
}

#[test]
fn unknown_model_returns_none() {
    assert!(lookup("claude-opus-4-8").is_none());
    assert!(lookup("gpt-9").is_none());
    assert!(lookup("").is_none());
}

#[test]
fn no_substring_matching() {
    // The bug the new crate fixes: `claude-haiku-4-5` must NOT silently
    // match `claude-haiku-4` (it's a different model).
    let card = lookup("claude-haiku-4-5").expect("haiku 4.5 should resolve");
    assert_eq!(card.canonical_id, "claude-haiku-4-5");
    // Plain "claude-haiku-4" is not in the catalog. It must return None,
    // not invent a cutoff date.
    assert!(lookup("claude-haiku-4").is_none());
}

#[test]
fn knowledge_cutoff_helper_omits_for_unknown() {
    assert_eq!(knowledge_cutoff("claude-opus-4-7"), Some("January 2026"));
    assert_eq!(knowledge_cutoff("claude-opus-4-8"), None);
}

#[test]
fn pricing_present_for_claude_models() {
    let opus = lookup("claude-opus-4-7").unwrap();
    let p = opus.pricing.expect("opus should have pricing");
    assert!(p.input_per_million_usd > 0.0);
    assert!(p.output_per_million_usd > p.input_per_million_usd);
}

#[test]
fn all_canonical_ids_unique() {
    let mut seen = std::collections::HashSet::new();
    for card in cards::ALL {
        assert!(
            seen.insert(card.canonical_id),
            "duplicate canonical_id: {}",
            card.canonical_id
        );
    }
}

#[test]
fn no_alias_collisions_across_cards() {
    let mut seen = std::collections::HashMap::new();
    for card in cards::ALL {
        for alias in card.aliases {
            if let Some(existing) = seen.insert(*alias, card.canonical_id) {
                panic!(
                    "alias `{}` claimed by both `{}` and `{}`",
                    alias, existing, card.canonical_id
                );
            }
        }
    }
}
