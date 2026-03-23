use super::*;

#[test]
fn test_task_file_watcher_new_with_nonexistent_dir() {
    // Should handle gracefully when directory doesn't exist
    let result = TaskFileWatcher::new(std::path::Path::new("/nonexistent/path"));
    // May or may not succeed depending on OS — just verify no panic
    let _ = result;
}

#[tokio::test]
async fn test_task_file_watcher_subscribe() {
    let dir = tempfile::tempdir().expect("tempdir");
    if let Some(watcher) = TaskFileWatcher::new(dir.path()) {
        let _rx = watcher.subscribe();
        // Subscription should succeed without panic
    }
}
