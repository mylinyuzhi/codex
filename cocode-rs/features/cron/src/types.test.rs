use super::*;

#[test]
fn test_generate_cron_id_format() {
    let id = generate_cron_id();
    assert!(
        id.starts_with("cron_"),
        "Expected prefix 'cron_', got: {id}"
    );
    assert_eq!(
        id.len(),
        13,
        "Expected 13 chars (cron_ + 8), got: {}",
        id.len()
    );
}

#[test]
fn test_generate_cron_id_uniqueness() {
    let ids: Vec<String> = (0..100).map(|_| generate_cron_id()).collect();
    let unique: std::collections::HashSet<&str> = ids.iter().map(String::as_str).collect();
    assert_eq!(unique.len(), ids.len(), "Generated duplicate IDs");
}

#[test]
fn test_cron_job_default_status() {
    let status = CronJobStatus::default();
    assert_eq!(status, CronJobStatus::Active);
}

#[test]
fn test_cron_job_serde_roundtrip() {
    let job = CronJob {
        id: "cron_test1234".to_string(),
        cron: "*/5 * * * *".to_string(),
        prompt: "check status".to_string(),
        description: Some("A test job".to_string()),
        recurring: true,
        durable: false,
        created_at: 1711234567,
        execution_count: 3,
        last_executed_at: Some(1711234800),
        expires_at: Some(1711493767),
        status: CronJobStatus::Active,
        consecutive_failures: 0,
        next_fire_at: None,
    };
    let json = serde_json::to_string(&job).expect("serialize");
    let deserialized: CronJob = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(deserialized.id, job.id);
    assert_eq!(deserialized.cron, job.cron);
    assert_eq!(deserialized.status, job.status);
    assert_eq!(deserialized.consecutive_failures, 0);
}

#[test]
fn test_cron_job_status_disabled_serde() {
    let json = r#""disabled""#;
    let status: CronJobStatus = serde_json::from_str(json).expect("deserialize");
    assert_eq!(status, CronJobStatus::Disabled);
}

#[test]
fn test_cron_job_defaults_on_missing_fields() {
    let json = r#"{
        "id": "cron_abc",
        "cron": "* * * * *",
        "prompt": "test",
        "created_at": 0
    }"#;
    let job: CronJob = serde_json::from_str(json).expect("deserialize");
    assert!(job.recurring, "recurring should default to true");
    assert!(!job.durable, "durable should default to false");
    assert_eq!(job.execution_count, 0);
    assert_eq!(job.consecutive_failures, 0);
    assert_eq!(job.status, CronJobStatus::Active);
    assert!(job.next_fire_at.is_none());
}
