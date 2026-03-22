use super::*;

#[test]
fn test_query_tracking_root() {
    let tracking = QueryTracking::new_root("chain-1");
    assert_eq!(tracking.chain_id, "chain-1");
    assert_eq!(tracking.depth, 0);
    assert!(tracking.is_root());
    assert!(tracking.parent_query_id.is_none());
}

#[test]
fn test_query_tracking_child() {
    let root = QueryTracking::new_root("chain-1");
    let child = root.child("query-1");

    assert_eq!(child.chain_id, "chain-1");
    assert_eq!(child.depth, 1);
    assert!(!child.is_root());
    assert_eq!(child.parent_query_id.as_deref(), Some("query-1"));

    let grandchild = child.child("query-2");
    assert_eq!(grandchild.depth, 2);
}

#[test]
fn test_auto_compact_tracking() {
    let mut tracking = AutoCompactTracking::new();
    assert!(!tracking.compacted);
    assert!(tracking.turn_id.is_none());

    tracking.mark_compacted("turn-1", 5);
    assert!(tracking.compacted);
    assert_eq!(tracking.turn_id.as_deref(), Some("turn-1"));
    assert_eq!(tracking.turn_counter, 5);

    tracking.reset();
    assert!(!tracking.compacted);
    assert!(tracking.turn_id.is_none());
    assert_eq!(tracking.turn_counter, 0);
}

#[test]
fn test_file_read_info() {
    let mtime = SystemTime::now();
    let info = FileReadInfo::new("content", mtime);

    assert_eq!(info.content, "content");
    assert_eq!(info.access_count, 1);
    assert!(info.is_complete_read);
    assert!(info.offset.is_none());
    assert!(info.limit.is_none());
}

#[test]
fn test_file_read_info_partial() {
    let mtime = SystemTime::now();
    let info = FileReadInfo::partial("partial", mtime, 10, 100);

    assert_eq!(info.offset, Some(10));
    assert_eq!(info.limit, Some(100));
    assert!(!info.is_complete_read);
}

#[test]
fn test_file_read_info_access() {
    let mtime = SystemTime::now();
    let mut info = FileReadInfo::new("content", mtime);
    assert_eq!(info.access_count, 1);

    info.record_access();
    assert_eq!(info.access_count, 2);
}

#[test]
fn test_file_change() {
    let change = FileChange::modified("/tmp/test.txt");
    assert_eq!(change.change_type, FileChangeType::Modified);

    let change = FileChange::deleted("/tmp/test.txt");
    assert_eq!(change.change_type, FileChangeType::Deleted);

    let change = FileChange::created("/tmp/test.txt");
    assert_eq!(change.change_type, FileChangeType::Created);
}

#[test]
fn test_file_change_type_display() {
    assert_eq!(FileChangeType::Modified.as_str(), "modified");
    assert_eq!(FileChangeType::Deleted.as_str(), "deleted");
    assert_eq!(FileChangeType::Created.as_str(), "created");
}

#[test]
fn test_serde_roundtrip() {
    let tracking = QueryTracking::new_root("chain-1");
    let json = serde_json::to_string(&tracking).unwrap();
    let parsed: QueryTracking = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tracking);

    let mut compact = AutoCompactTracking::new();
    compact.mark_compacted("turn-1", 5);
    let json = serde_json::to_string(&compact).unwrap();
    let parsed: AutoCompactTracking = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, compact);
}

#[test]
fn test_extraction_tracking() {
    let mut tracking = AutoCompactTracking::new();
    assert_eq!(tracking.extraction_count, 0);
    assert!(!tracking.extraction_in_progress);

    // Record some tool calls
    tracking.record_tool_call();
    tracking.record_tool_call();
    assert_eq!(tracking.tool_call_count, 2);

    // Start extraction
    tracking.mark_extraction_started();
    assert!(tracking.extraction_in_progress);

    // Complete extraction
    tracking.mark_extraction_completed(10000, "msg-123");
    assert!(!tracking.extraction_in_progress);
    assert_eq!(tracking.extraction_count, 1);
    assert_eq!(tracking.last_extraction_tokens, 10000);
    assert_eq!(tracking.last_extraction_tool_calls, 2);
    assert_eq!(tracking.last_extraction_id.as_deref(), Some("msg-123"));

    // Check tokens/calls since extraction
    assert_eq!(tracking.tokens_since_extraction(15000), 5000);
    assert_eq!(tracking.tool_calls_since_extraction(), 0);

    // Record more tool calls
    tracking.record_tool_call();
    assert_eq!(tracking.tool_calls_since_extraction(), 1);
}

#[test]
fn test_extraction_failure() {
    let mut tracking = AutoCompactTracking::new();
    tracking.mark_extraction_started();
    assert!(tracking.extraction_in_progress);

    tracking.mark_extraction_failed();
    assert!(!tracking.extraction_in_progress);
    assert_eq!(tracking.extraction_count, 0); // Count should not increase on failure
}
