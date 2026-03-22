use super::*;

#[test]
fn test_shorten_path_normal() {
    let short = HeaderBar::shorten_path("/home/user/projects/foo");
    assert!(!short.is_empty());
}
