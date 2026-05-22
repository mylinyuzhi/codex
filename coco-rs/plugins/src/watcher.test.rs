//! Tests for the plugin change detector.

use super::*;
use std::path::Path;

#[test]
fn is_interesting_accepts_plugin_toml() {
    assert!(is_interesting_plugin_path(Path::new("/x/PLUGIN.toml")));
    assert!(is_interesting_plugin_path(Path::new(
        "/x/installed_plugins.json"
    )));
    assert!(is_interesting_plugin_path(Path::new("/x/marketplace.json")));
    assert!(is_interesting_plugin_path(Path::new("/x/some-skill.md")));
}

#[test]
fn is_interesting_rejects_editor_temp_files() {
    assert!(!is_interesting_plugin_path(Path::new("/x/.#PLUGIN.toml")));
    assert!(!is_interesting_plugin_path(Path::new("/x/PLUGIN.toml~")));
    assert!(!is_interesting_plugin_path(Path::new(
        "/x/.PLUGIN.toml.swp"
    )));
}

#[test]
fn derive_reason_picks_known_filenames() {
    assert_eq!(
        derive_reason(&[PathBuf::from("/x/installed_plugins.json")]),
        "installed_plugins.json changed"
    );
    assert_eq!(
        derive_reason(&[PathBuf::from("/x/my-plugin/PLUGIN.toml")]),
        "PLUGIN.toml changed"
    );
    assert_eq!(
        derive_reason(&[PathBuf::from("/x/marketplaces/foo/marketplace.json")]),
        "marketplace.json changed"
    );
}

#[test]
fn derive_reason_falls_back_to_first_filename() {
    assert_eq!(
        derive_reason(&[PathBuf::from("/x/skills/my-skill.md")]),
        "my-skill.md changed"
    );
}

#[tokio::test]
async fn watcher_emits_on_file_change() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().to_path_buf();
    let detector = PluginChangeDetector::new(vec![dir.clone()]).expect("watcher");
    let mut rx = detector.subscribe();

    // Race-free signal: write a file inside the watched dir; the
    // watcher's debounce is 300ms, give it a 2s recv timeout.
    let target = dir.join("PLUGIN.toml");
    std::fs::write(&target, b"[plugin]\nname = \"x\"").expect("write");

    let event = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("watcher did not fire")
        .expect("recv error");
    assert!(!event.changed_paths.is_empty(), "no paths in event");
    assert_eq!(event.reason, "PLUGIN.toml changed");
}
