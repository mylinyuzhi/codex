use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

use super::*;

#[test]
fn write_user_setting_rejects_invalid_json_without_overwriting_file() {
    let tmp = TempDir::new().expect("temp dir");
    let path = tmp.path().join("settings.json");
    let original = "{ invalid json";
    std::fs::write(&path, original).expect("write invalid settings");

    let err = write_user_setting_to_path(&path, "theme", json!("dark"))
        .expect_err("invalid settings must fail");

    assert!(
        err.to_string().contains("failed to parse JSON"),
        "unexpected error: {err}"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn write_user_setting_preserves_siblings_for_dotted_keys() {
    let tmp = TempDir::new().expect("temp dir");
    let path = tmp.path().join("settings.json");
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "theme": "dark",
            "sandbox": {
                "mode": "workspace-write",
                "network": "enabled"
            }
        }))
        .unwrap(),
    )
    .expect("write settings");

    let written =
        write_user_setting_to_path(&path, "sandbox.mode", json!("read-only")).expect("write");

    assert_eq!(written, path);
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        value,
        json!({
            "theme": "dark",
            "sandbox": {
                "mode": "read-only",
                "network": "enabled"
            }
        })
    );
}
