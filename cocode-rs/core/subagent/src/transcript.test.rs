use super::*;

#[test]
fn test_transcript_recorder_path() {
    let path = PathBuf::from("/tmp/test-transcript.jsonl");
    let recorder = TranscriptRecorder::new(path.clone());
    assert_eq!(recorder.path(), &path);
}

#[tokio::test]
async fn test_transcript_record_write() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("test.jsonl");

    let recorder = TranscriptRecorder::new(path.clone());
    let entry1 = serde_json::json!({"role": "user", "content": "hello"});
    let entry2 = serde_json::json!({"role": "assistant", "content": "hi"});

    recorder.record(&entry1).await.expect("record entry 1");
    recorder.record(&entry2).await.expect("record entry 2");

    let contents = std::fs::read_to_string(&path).expect("read transcript");
    let lines: Vec<&str> = contents.trim().split('\n').collect();
    assert_eq!(lines.len(), 2);

    let parsed1: serde_json::Value = serde_json::from_str(lines[0]).expect("parse line 1");
    assert_eq!(parsed1["role"], "user");
    let parsed2: serde_json::Value = serde_json::from_str(lines[1]).expect("parse line 2");
    assert_eq!(parsed2["role"], "assistant");
}

#[tokio::test]
async fn test_read_empty_file() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("empty.jsonl");

    std::fs::write(&path, "").expect("write empty file");

    let entries = TranscriptRecorder::read_transcript(&path)
        .await
        .expect("read transcript");
    assert!(entries.is_empty());
}

#[tokio::test]
async fn test_read_entries_roundtrip() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("roundtrip.jsonl");

    let recorder = TranscriptRecorder::new(path.clone());
    let entry1 = serde_json::json!({"role": "user", "content": "hello"});
    let entry2 = serde_json::json!({"role": "assistant", "content": "hi"});

    recorder.record(&entry1).await.expect("record entry 1");
    recorder.record(&entry2).await.expect("record entry 2");

    let entries = recorder.read_entries().await.expect("read entries");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["role"], "user");
    assert_eq!(entries[0]["content"], "hello");
    assert_eq!(entries[1]["role"], "assistant");
    assert_eq!(entries[1]["content"], "hi");
}

#[tokio::test]
async fn test_read_skips_invalid_lines() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("invalid.jsonl");

    let content = r#"{"role":"user","content":"hello"}
not valid json
{"role":"assistant","content":"hi"}

{broken
{"role":"system","content":"done"}
"#;
    std::fs::write(&path, content).expect("write test file");

    let entries = TranscriptRecorder::read_transcript(&path)
        .await
        .expect("read transcript");
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0]["role"], "user");
    assert_eq!(entries[1]["role"], "assistant");
    assert_eq!(entries[2]["role"], "system");
}

#[tokio::test]
async fn test_record_progress() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("progress.jsonl");

    let recorder = TranscriptRecorder::new(path.clone());
    recorder
        .record_progress("agent-1", "Reading files...")
        .await
        .expect("record progress");
    recorder
        .record_progress("agent-1", "Analyzing code...")
        .await
        .expect("record progress 2");

    let entries = recorder.read_entries().await.expect("read entries");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["type"], "progress");
    assert_eq!(entries[0]["agent_id"], "agent-1");
    assert_eq!(entries[0]["message"], "Reading files...");
    assert!(entries[0]["timestamp"].is_string());
    assert_eq!(entries[1]["message"], "Analyzing code...");
}

#[tokio::test]
async fn test_record_turn_result() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("turn.jsonl");

    let recorder = TranscriptRecorder::new(path.clone());
    recorder
        .record_turn_result("agent-1", 1, "Found 3 files")
        .await
        .expect("record turn");
    recorder
        .record_turn_result("agent-1", 2, "Applied changes")
        .await
        .expect("record turn 2");

    let entries = recorder.read_entries().await.expect("read entries");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["type"], "turn_result");
    assert_eq!(entries[0]["agent_id"], "agent-1");
    assert_eq!(entries[0]["turn"], 1);
    assert_eq!(entries[0]["text"], "Found 3 files");
    assert!(entries[0]["timestamp"].is_string());
    assert_eq!(entries[1]["turn"], 2);
}

#[tokio::test]
async fn test_mixed_entry_types() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("mixed.jsonl");

    let recorder = TranscriptRecorder::new(path.clone());

    recorder
        .record_progress("agent-1", "Starting")
        .await
        .expect("progress");
    recorder
        .record_turn_result("agent-1", 1, "Step 1 done")
        .await
        .expect("turn");
    let completion = serde_json::json!({
        "status": "completed",
        "agent_id": "agent-1",
        "output": "All done"
    });
    recorder.record(&completion).await.expect("completion");

    let entries = recorder.read_entries().await.expect("read entries");
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0]["type"], "progress");
    assert_eq!(entries[1]["type"], "turn_result");
    assert_eq!(entries[2]["status"], "completed");
}

// ── filter_empty_entries tests ──

#[test]
fn test_filter_empty_entries() {
    let entries = vec![
        serde_json::json!({"prompt": "do stuff", "output": "result here"}),
        serde_json::json!({"prompt": "do more", "output": ""}),
        serde_json::json!({"prompt": "another", "output": "   "}),
        serde_json::json!({"type": "progress", "message": "working"}),
        serde_json::json!({"prompt": "final", "output": "done"}),
        serde_json::json!({"prompt": "null output", "output": null}),
    ];
    let filtered = filter_empty_entries(&entries);
    // Keeps: first (has output), fourth (no "output" field), fifth (has output)
    // Removes: second (empty output), third (whitespace output), sixth (null output)
    assert_eq!(filtered.len(), 3);
    assert_eq!(filtered[0]["output"], "result here");
    assert_eq!(filtered[1]["type"], "progress");
    assert_eq!(filtered[2]["output"], "done");
}

#[test]
fn test_filter_empty_entries_preserves_non_string_output() {
    let entries = vec![
        serde_json::json!({"output": ["item1", "item2"]}),
        serde_json::json!({"output": 42}),
    ];
    let filtered = filter_empty_entries(&entries);
    assert_eq!(filtered.len(), 2);
}

#[tokio::test]
async fn test_resume_with_empty_entries_filtered() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("resume.jsonl");

    let recorder = TranscriptRecorder::new(path.clone());
    recorder
        .record(&serde_json::json!({"prompt": "task1", "output": "done"}))
        .await
        .expect("record");
    recorder
        .record(&serde_json::json!({"prompt": "task2", "output": ""}))
        .await
        .expect("record");
    recorder
        .record(&serde_json::json!({"prompt": "task3", "output": "completed"}))
        .await
        .expect("record");

    let entries = recorder.read_entries().await.expect("read");
    let filtered = filter_empty_entries(&entries);
    assert_eq!(filtered.len(), 2);
    assert_eq!(filtered[0]["prompt"], "task1");
    assert_eq!(filtered[1]["prompt"], "task3");
}

// ── read_from_offset tests ──

#[tokio::test]
async fn test_read_from_offset_basic() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("offset.jsonl");

    let recorder = TranscriptRecorder::new(path.clone());
    recorder
        .record(&serde_json::json!({"turn": 1, "text": "first"}))
        .await
        .expect("record");
    recorder
        .record(&serde_json::json!({"turn": 2, "text": "second"}))
        .await
        .expect("record");

    // Read from the beginning
    let (entries, new_offset) = read_from_offset(&path, 0).await.expect("read");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["turn"], 1);
    assert_eq!(entries[1]["turn"], 2);
    assert!(new_offset > 0);

    // Append another entry
    recorder
        .record(&serde_json::json!({"turn": 3, "text": "third"}))
        .await
        .expect("record");

    // Read from the previous offset — should only get the new entry
    let (entries, final_offset) = read_from_offset(&path, new_offset).await.expect("read");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["turn"], 3);
    assert!(final_offset > new_offset);
}

#[tokio::test]
async fn test_read_from_offset_at_end() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("at_end.jsonl");

    let recorder = TranscriptRecorder::new(path.clone());
    recorder
        .record(&serde_json::json!({"data": "entry"}))
        .await
        .expect("record");

    let (_, offset) = read_from_offset(&path, 0).await.expect("read");

    // Reading at the end should return empty
    let (entries, same_offset) = read_from_offset(&path, offset).await.expect("read");
    assert!(entries.is_empty());
    assert_eq!(same_offset, offset);
}
