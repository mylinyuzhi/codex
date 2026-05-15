//! Tests for the `/help` slash renderer. These guard the i18n key
//! invariant: every entry in the static tables must resolve to a real
//! YAML key in both locales — `rust-i18n` silently returns the key
//! itself on a miss, so a typo would surface as raw `help.slash.cmd.foo`
//! in the user's terminal.

use super::*;
use rust_i18n::set_locale;

fn render_in(locale: &str) -> String {
    set_locale(locale);
    render_overview()
}

#[test]
fn overview_contains_translated_section_titles_en() {
    let out = render_in("en");
    assert!(out.contains("Commands"));
    assert!(out.contains("Keyboard shortcuts"));
    // Vim group is rendered by keymap::export_markdown under its own title.
    assert!(out.contains("Vim Normal mode"));
}

#[test]
fn overview_contains_translated_section_titles_zh() {
    let out = render_in("zh-CN");
    assert!(out.contains("命令"));
    assert!(out.contains("键盘快捷键"));
    assert!(out.contains("Vim Normal 模式"));
}

#[test]
fn overview_lists_every_command_in_categories() {
    set_locale("en");
    let out = render_overview();
    for category in CATEGORIES {
        for cmd in category.commands {
            assert!(
                out.contains(&format!("/{}", cmd.name)),
                "command `/{}` missing from overview",
                cmd.name
            );
        }
    }
}

#[test]
fn every_i18n_key_resolves_en() {
    set_locale("en");
    assert_all_keys_resolve();
}

#[test]
fn every_i18n_key_resolves_zh() {
    set_locale("zh-CN");
    assert_all_keys_resolve();
}

fn assert_all_keys_resolve() {
    fn check(key: &str) {
        let value = crate::i18n::t!(key).to_string();
        let locale = rust_i18n::locale().to_string();
        assert_ne!(
            value, key,
            "i18n key `{key}` does not resolve in locale `{locale}`"
        );
    }
    // Slash-command catalog keys (categories + per-command descriptions).
    for category in CATEGORIES {
        let cat_key = format!("help.slash.cat.{}", category.key);
        check(&cat_key);
        for cmd in category.commands {
            let cmd_key = format!("help.slash.cmd.{}", cmd.name);
            check(&cmd_key);
        }
    }
    // Section title keys used directly by `render_overview`.
    check("help.slash.title");
    check("help.slash.tagline");
    check("help.slash.section.commands");
    check("help.slash.section.shortcuts");
    check("help.slash.footer.hint");
    check("help.slash.field.usage");
    check("help.slash.field.aliases");
    // Keymap entries are covered by `keymap::tests::every_description_key_resolves_*`.
}

#[test]
fn command_detail_lookup_by_alias() {
    set_locale("en");
    let out = render_command_detail("st").expect("status alias should resolve");
    assert!(out.contains("/status"));
    assert!(out.contains("Aliases:"));
}

#[test]
fn command_detail_unknown_returns_none() {
    set_locale("en");
    assert!(render_command_detail("nonexistent-cmd").is_none());
}

#[test]
fn not_found_message_includes_query() {
    set_locale("en");
    let msg = render_not_found("foo");
    assert!(msg.contains("foo"));
}
