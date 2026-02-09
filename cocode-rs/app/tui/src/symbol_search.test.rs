use super::*;

#[tokio::test]
async fn test_create_symbol_search_manager() {
    let (tx, _rx) = create_symbol_search_channel();
    let _manager = SymbolSearchManager::new(PathBuf::from("/tmp"), tx);
}

#[tokio::test]
async fn test_cancel_pending_search() {
    let (tx, _rx) = create_symbol_search_channel();
    let mut manager = SymbolSearchManager::new(PathBuf::from("/tmp"), tx);

    // Schedule a search
    manager.on_query("test".to_string(), 0);
    assert!(manager.pending_search.is_some());

    // Cancel it
    manager.cancel();
    assert!(manager.pending_search.is_none());
}
