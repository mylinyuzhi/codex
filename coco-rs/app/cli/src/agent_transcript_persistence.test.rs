use super::*;

fn temp_store() -> SessionAgentTranscriptStore {
    let dir = std::env::temp_dir().join(format!("coco-trans-{}", uuid::Uuid::new_v4().simple()));
    let store = Arc::new(TranscriptStore::new(dir));
    SessionAgentTranscriptStore::new(store)
}

#[tokio::test]
async fn write_then_read_metadata_roundtrip() {
    let st = temp_store();
    let meta = AgentSpawnMetadata {
        agent_type: "Explore".into(),
        worktree_path: Some("/tmp/wt".into()),
        description: Some("look around".into()),
    };
    st.write_agent_metadata("sess-1", "agent-7af2", &meta)
        .await
        .unwrap();
    let got = st
        .read_agent_metadata("sess-1", "agent-7af2")
        .await
        .unwrap();
    assert_eq!(got, Some(meta));
}

#[tokio::test]
async fn read_metadata_returns_none_for_unknown() {
    let st = temp_store();
    let got = st.read_agent_metadata("sess-1", "ghost").await.unwrap();
    assert!(got.is_none());
}

#[tokio::test]
async fn append_then_load_messages_roundtrip() {
    let st = temp_store();
    let msgs = vec![
        serde_json::json!({"role": "user", "content": "hello"}),
        serde_json::json!({"role": "assistant", "content": "hi"}),
    ];
    st.append_agent_messages("sess-1", "agent-1", msgs.clone())
        .await
        .unwrap();
    let got = st
        .load_agent_messages("sess-1", "agent-1")
        .await
        .unwrap()
        .expect("should have messages");
    assert_eq!(got, msgs);
}

#[tokio::test]
async fn load_messages_returns_none_for_unknown() {
    let st = temp_store();
    let got = st.load_agent_messages("sess-1", "ghost").await.unwrap();
    assert!(got.is_none());
}

#[tokio::test]
async fn append_is_additive_across_calls() {
    let st = temp_store();
    let first = vec![serde_json::json!({"step": 1})];
    let second = vec![serde_json::json!({"step": 2})];
    st.append_agent_messages("sess-1", "agent-1", first)
        .await
        .unwrap();
    st.append_agent_messages("sess-1", "agent-1", second)
        .await
        .unwrap();
    let got = st
        .load_agent_messages("sess-1", "agent-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.len(), 2);
    assert_eq!(got[0], serde_json::json!({"step": 1}));
    assert_eq!(got[1], serde_json::json!({"step": 2}));
}
