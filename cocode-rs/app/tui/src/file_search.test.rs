use super::*;

#[tokio::test]
async fn test_file_index_cache_validity() {
    let cache = FileIndexCache::default();
    assert!(!cache.is_valid());
}

#[tokio::test]
async fn test_create_file_search_manager() {
    let (tx, _rx) = create_file_search_channel();
    let manager = FileSearchManager::new(PathBuf::from("/tmp"), tx);
    assert_eq!(manager.cwd(), &PathBuf::from("/tmp"));
}

#[tokio::test]
async fn test_cancel_pending_search() {
    let (tx, _rx) = create_file_search_channel();
    let mut manager = FileSearchManager::new(PathBuf::from("/tmp"), tx);

    // Schedule a search
    manager.on_query("test".to_string(), 0);
    assert!(manager.pending_search.is_some());

    // Cancel it
    manager.cancel();
    assert!(manager.pending_search.is_none());
}
