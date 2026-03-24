use super::*;

#[test]
fn test_create_noop_watcher() {
    let watcher = create_noop_watcher();
    // Should not panic when subscribing.
    let _rx = watcher.subscribe();
}
