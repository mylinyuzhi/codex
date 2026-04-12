use super::*;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

#[test]
fn create_with_absolute_path_ignores_base_path() {
    let base_dir = tempdir().expect("base dir");
    let absolute_dir = tempdir().expect("absolute dir");
    let base_path = base_dir.path();
    let absolute_path = absolute_dir.path().join("file.txt");
    let abs_path_buf = AbsolutePathBuf::resolve_path_against_base(absolute_path.clone(), base_path)
        .expect("failed to create");
    assert_eq!(abs_path_buf.as_path(), absolute_path.as_path());
}

#[test]
fn relative_path_is_resolved_against_base_path() {
    let temp_dir = tempdir().expect("base dir");
    let base_dir = temp_dir.path();
    let abs_path_buf =
        AbsolutePathBuf::resolve_path_against_base("file.txt", base_dir).expect("failed to create");
    assert_eq!(abs_path_buf.as_path(), base_dir.join("file.txt").as_path());
}

#[test]
fn guard_used_in_deserialization() {
    let temp_dir = tempdir().expect("base dir");
    let base_dir = temp_dir.path();
    let relative_path = "subdir/file.txt";
    let abs_path_buf = {
        let _guard = AbsolutePathBufGuard::new(base_dir);
        serde_json::from_str::<AbsolutePathBuf>(&format!(r#""{relative_path}""#))
            .expect("failed to deserialize")
    };
    assert_eq!(
        abs_path_buf.as_path(),
        base_dir.join(relative_path).as_path()
    );
}

#[cfg(not(target_os = "windows"))]
#[test]
fn home_directory_root_on_non_windows_is_expanded_in_deserialization() {
    let Some(home) = home_dir() else {
        return;
    };
    let temp_dir = tempdir().expect("base dir");
    let abs_path_buf = {
        let _guard = AbsolutePathBufGuard::new(temp_dir.path());
        serde_json::from_str::<AbsolutePathBuf>("\"~\"").expect("failed to deserialize")
    };
    assert_eq!(abs_path_buf.as_path(), home.as_path());
}

#[cfg(not(target_os = "windows"))]
#[test]
fn home_directory_subpath_on_non_windows_is_expanded_in_deserialization() {
    let Some(home) = home_dir() else {
        return;
    };
    let temp_dir = tempdir().expect("base dir");
    let abs_path_buf = {
        let _guard = AbsolutePathBufGuard::new(temp_dir.path());
        serde_json::from_str::<AbsolutePathBuf>("\"~/code\"").expect("failed to deserialize")
    };
    assert_eq!(abs_path_buf.as_path(), home.join("code").as_path());
}

#[cfg(not(target_os = "windows"))]
#[test]
fn home_directory_double_slash_on_non_windows_is_expanded_in_deserialization() {
    let Some(home) = home_dir() else {
        return;
    };
    let temp_dir = tempdir().expect("base dir");
    let abs_path_buf = {
        let _guard = AbsolutePathBufGuard::new(temp_dir.path());
        serde_json::from_str::<AbsolutePathBuf>("\"~//code\"").expect("failed to deserialize")
    };
    assert_eq!(abs_path_buf.as_path(), home.join("code").as_path());
}

#[cfg(target_os = "windows")]
#[test]
fn home_directory_on_windows_is_not_expanded_in_deserialization() {
    let temp_dir = tempdir().expect("base dir");
    let base_dir = temp_dir.path();
    let abs_path_buf = {
        let _guard = AbsolutePathBufGuard::new(base_dir);
        serde_json::from_str::<AbsolutePathBuf>("\"~/code\"").expect("failed to deserialize")
    };
    assert_eq!(
        abs_path_buf.as_path(),
        base_dir.join("~").join("code").as_path()
    );
}
