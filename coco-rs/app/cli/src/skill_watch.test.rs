use super::default_watch_paths;
use std::path::Path;

#[test]
fn default_watch_paths_covers_user_and_project_scopes() {
    let cwd = Path::new("/proj");
    let config_home = Path::new("/home/.coco");
    let paths = default_watch_paths(cwd, config_home);
    assert_eq!(
        paths,
        vec![
            Path::new("/home/.coco/skills").to_path_buf(),
            Path::new("/proj/.coco/skills").to_path_buf(),
        ]
    );
}
