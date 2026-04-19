use super::KeyChord;
use super::KeyCombo;
use super::parse_chord;
use super::parse_combo;

fn combo(key: &str) -> KeyCombo {
    KeyCombo {
        ctrl: false,
        shift: false,
        alt: false,
        meta: false,
        key: key.into(),
    }
}

#[test]
fn parses_plain_key() {
    assert_eq!(parse_combo("a").unwrap(), combo("a"));
    assert_eq!(parse_combo("enter").unwrap(), combo("enter"));
    assert_eq!(parse_combo("f1").unwrap(), combo("f1"));
}

#[test]
fn parses_ctrl_modifier() {
    let got = parse_combo("ctrl+a").unwrap();
    assert!(got.ctrl);
    assert!(!got.shift);
    assert_eq!(got.key, "a");
}

#[test]
fn parses_multi_modifier_case_insensitive() {
    let got = parse_combo("Ctrl+Shift+P").unwrap();
    assert!(got.ctrl && got.shift && !got.alt && !got.meta);
    assert_eq!(got.key, "p");
}

#[test]
fn parses_meta_aliases() {
    for token in ["cmd", "command", "meta", "super"] {
        let got = parse_combo(&format!("{token}+k")).unwrap();
        assert!(got.meta, "token `{token}` should set meta");
        assert_eq!(got.key, "k");
    }
}

#[test]
fn normalizes_named_keys() {
    assert_eq!(parse_combo("Return").unwrap().key, "enter");
    assert_eq!(parse_combo("Escape").unwrap().key, "esc");
    assert_eq!(parse_combo("delete").unwrap().key, "del");
    assert_eq!(parse_combo("pgup").unwrap().key, "pageup");
    assert_eq!(parse_combo("pgdn").unwrap().key, "pagedown");
}

#[test]
fn rejects_empty_and_multi_key() {
    assert!(parse_combo("").is_err());
    assert!(parse_combo("ctrl").is_err()); // no base key
    assert!(parse_combo("a+b").is_err()); // two base keys
    assert!(parse_chord("").is_err());
}

#[test]
fn parses_chord_with_two_combos() {
    let chord = parse_chord("ctrl+k, ctrl+s").unwrap();
    assert!(!chord.is_single());
    assert_eq!(chord.0.len(), 2);
    assert!(chord.0[0].ctrl && chord.0[0].key == "k");
    assert!(chord.0[1].ctrl && chord.0[1].key == "s");
}

#[test]
fn single_key_is_single_chord() {
    let chord = parse_chord("enter").unwrap();
    assert!(chord.is_single());
    assert_eq!(chord, KeyChord(vec![combo("enter")]));
}
