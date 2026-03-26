use std::path::Path;

use super::*;

#[test]
fn test_generate_tab_name_format() {
    let name = generate_tab_name(Path::new("/src/main.rs"));
    assert!(name.starts_with("\u{273B} [Claude Code] main.rs ("));
    assert!(name.ends_with(") \u{29C9}"));
    // 6-char random ID between parens
    let id_start = name.find('(').expect("should have (") + 1;
    let id_end = name.find(')').expect("should have )");
    assert_eq!(id_end - id_start, 6);
}

#[test]
fn test_generate_tab_name_unique() {
    let name1 = generate_tab_name(Path::new("/src/main.rs"));
    let name2 = generate_tab_name(Path::new("/src/main.rs"));
    // Random IDs should differ (extremely unlikely to collide)
    assert_ne!(name1, name2);
}
