use super::ChordResolver;
use super::ResolveOutcome;
use crate::Keybinding;
use crate::parser::parse_combo;

fn binding(key: &str, action: &str, context: Option<&str>) -> Keybinding {
    Keybinding {
        key: key.into(),
        action: action.into(),
        context: context.map(str::to_string),
        when: None,
    }
}

#[test]
fn single_combo_fires_immediately() {
    let bindings = vec![binding("ctrl+a", "select_all", Some("input"))];
    let mut resolver = ChordResolver::new(&bindings);
    let combo = parse_combo("ctrl+a").unwrap();
    assert_eq!(
        resolver.feed(&combo, &["input"]),
        ResolveOutcome::Fire("select_all".into())
    );
    assert!(!resolver.has_pending());
}

#[test]
fn unknown_combo_returns_no_match() {
    let bindings = vec![binding("ctrl+a", "select_all", Some("input"))];
    let mut resolver = ChordResolver::new(&bindings);
    let combo = parse_combo("ctrl+b").unwrap();
    assert_eq!(resolver.feed(&combo, &["input"]), ResolveOutcome::NoMatch);
}

#[test]
fn chord_returns_pending_then_fire() {
    let bindings = vec![binding("ctrl+k, ctrl+s", "save", Some("input"))];
    let mut resolver = ChordResolver::new(&bindings);
    let first = parse_combo("ctrl+k").unwrap();
    let second = parse_combo("ctrl+s").unwrap();
    assert_eq!(resolver.feed(&first, &["input"]), ResolveOutcome::Pending);
    assert!(resolver.has_pending());
    assert_eq!(
        resolver.feed(&second, &["input"]),
        ResolveOutcome::Fire("save".into())
    );
    assert!(!resolver.has_pending());
}

#[test]
fn chord_breaks_on_unmatched_second_combo() {
    let bindings = vec![binding("ctrl+k, ctrl+s", "save", Some("input"))];
    let mut resolver = ChordResolver::new(&bindings);
    let first = parse_combo("ctrl+k").unwrap();
    let unrelated = parse_combo("ctrl+z").unwrap();
    assert_eq!(resolver.feed(&first, &["input"]), ResolveOutcome::Pending);
    assert_eq!(
        resolver.feed(&unrelated, &["input"]),
        ResolveOutcome::NoMatch
    );
    assert!(!resolver.has_pending());
}

#[test]
fn context_specificity_wins() {
    let bindings = vec![
        binding("enter", "submit_global", None),
        binding("enter", "submit_input", Some("input")),
    ];
    let mut resolver = ChordResolver::new(&bindings);
    let combo = parse_combo("enter").unwrap();

    assert_eq!(
        resolver.feed(&combo, &["input"]),
        ResolveOutcome::Fire("submit_input".into())
    );

    assert_eq!(
        resolver.feed(&combo, &[]),
        ResolveOutcome::Fire("submit_global".into())
    );
}

#[test]
fn reset_clears_pending_state() {
    let bindings = vec![binding("ctrl+k, ctrl+s", "save", Some("input"))];
    let mut resolver = ChordResolver::new(&bindings);
    let first = parse_combo("ctrl+k").unwrap();
    let second = parse_combo("ctrl+s").unwrap();
    resolver.feed(&first, &["input"]);
    resolver.reset();
    // After reset, ctrl+s alone should not fire the chord action.
    assert_eq!(resolver.feed(&second, &["input"]), ResolveOutcome::NoMatch);
}
