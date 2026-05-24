use super::*;

#[tokio::test]
async fn test_multibyte_prompt_truncation_no_panic() {
    use cocode_cron::CronJob;
    use cocode_cron::CronJobStatus;
    use cocode_cron::new_cron_store;

    let store = new_cron_store();
    {
        let mut guard = store.lock().await;
        // Build a prompt that has a multi-byte codepoint straddling byte 80.
        // Each emoji is 4 bytes. 20 emojis = 80 bytes, so byte 80 is exactly
        // a boundary. Use 19 emojis (76 bytes) + a 4-byte char so byte 80
        // falls mid-codepoint.
        let prompt = "\u{1F600}".repeat(19) + "\u{1F600}extra"; // 76 + 4 + 5 = 85 bytes
        assert!(prompt.len() > 80);
        // Byte 80 is inside the 20th emoji — slicing [..80] would panic.
        guard.insert(
            "job-1".to_string(),
            CronJob {
                id: "job-1".to_string(),
                cron: "* * * * *".to_string(),
                prompt,
                description: None,
                recurring: true,
                durable: false,
                created_at: 0,
                execution_count: 0,
                last_executed_at: None,
                expires_at: None,
                status: CronJobStatus::Active,
                consecutive_failures: 0,
                next_fire_at: None,
            },
        );
    }

    let tool = CronListTool::new(store);
    let mut ctx = ToolContext::new("call-1", "session-1", std::path::PathBuf::from("/tmp"));

    // Should not panic
    let result = tool.execute(serde_json::json!({}), &mut ctx).await.unwrap();
    assert!(!result.is_error);
}
