use super::*;

#[test]
fn test_new_cron_store_is_empty() {
    let store = new_cron_store();
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    rt.block_on(async {
        let guard = store.lock().await;
        assert!(guard.is_empty());
    });
}

#[test]
fn test_jobs_to_value_empty() {
    let jobs = BTreeMap::new();
    let value = jobs_to_value(&jobs);
    assert_eq!(value, serde_json::json!({}));
}

#[test]
fn test_format_cron_summary_empty() {
    let jobs: Vec<&CronJob> = vec![];
    let summary = format_cron_summary(jobs.into_iter());
    assert_eq!(summary, "No scheduled jobs.");
}

#[test]
fn test_format_cron_summary_with_jobs() {
    let job = CronJob {
        id: "cron_test".to_string(),
        cron: "*/5 * * * *".to_string(),
        prompt: "check status".to_string(),
        description: Some("A test".to_string()),
        recurring: true,
        durable: true,
        created_at: 0,
        execution_count: 2,
        last_executed_at: None,
        expires_at: None,
        status: CronJobStatus::Active,
        consecutive_failures: 0,
        next_fire_at: None,
    };
    let summary = format_cron_summary(std::iter::once(&job));
    assert!(summary.contains("cron_test"));
    assert!(summary.contains("[*/5 * * * *]"));
    assert!(summary.contains("[durable]"));
    assert!(summary.contains("executions: 2"));
    assert!(summary.contains("description: A test"));
}

#[test]
fn test_format_cron_summary_disabled_status() {
    let job = CronJob {
        id: "cron_dis".to_string(),
        cron: "0 * * * *".to_string(),
        prompt: "test".to_string(),
        description: None,
        recurring: false,
        durable: false,
        created_at: 0,
        execution_count: 0,
        last_executed_at: None,
        expires_at: None,
        status: CronJobStatus::Disabled,
        consecutive_failures: 3,
        next_fire_at: None,
    };
    let summary = format_cron_summary(std::iter::once(&job));
    assert!(summary.contains("[disabled]"));
    assert!(summary.contains("(one-shot)"));
}
