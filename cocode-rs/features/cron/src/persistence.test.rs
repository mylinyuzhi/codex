use std::collections::BTreeMap;

use super::*;
use crate::types::CronJob;
use crate::types::CronJobStatus;

fn make_job(id: &str, recurring: bool, created_at: i64) -> CronJob {
    CronJob {
        id: id.to_string(),
        cron: "*/5 * * * *".to_string(),
        prompt: "check status".to_string(),
        description: None,
        recurring,
        durable: true,
        created_at,
        execution_count: 0,
        last_executed_at: None,
        expires_at: None,
        status: CronJobStatus::Active,
        consecutive_failures: 0,
        next_fire_at: None,
    }
}

#[test]
fn test_detect_missed_oneshots_finds_non_recurring() {
    let now = 1711234567;
    let mut jobs = BTreeMap::new();
    jobs.insert("a".to_string(), make_job("a", false, now - 100)); // one-shot, created before now
    jobs.insert("b".to_string(), make_job("b", true, now - 100)); // recurring, should be skipped

    let missed = detect_missed_oneshots(&jobs, now);
    assert_eq!(missed.len(), 1);
    assert_eq!(missed[0].id, "a");
}

#[test]
fn test_detect_missed_oneshots_empty_when_none() {
    let now = 1711234567;
    let mut jobs = BTreeMap::new();
    jobs.insert("a".to_string(), make_job("a", true, now - 100));

    let missed = detect_missed_oneshots(&jobs, now);
    assert!(missed.is_empty());
}

#[test]
fn test_detect_missed_oneshots_skips_completed() {
    let now = 1711234567;
    let mut job = make_job("a", false, now - 100);
    job.status = CronJobStatus::Completed;
    let mut jobs = BTreeMap::new();
    jobs.insert("a".to_string(), job);

    let missed = detect_missed_oneshots(&jobs, now);
    assert!(missed.is_empty());
}

#[test]
fn test_format_missed_tasks_message_empty() {
    assert!(format_missed_tasks_message(&[]).is_empty());
}

#[test]
fn test_format_missed_tasks_message_content() {
    let tasks = vec![MissedTask {
        id: "cron_abc".to_string(),
        prompt: "deploy".to_string(),
        cron: "0 9 * * *".to_string(),
        created_at: 1711234567,
    }];
    let msg = format_missed_tasks_message(&tasks);
    assert!(msg.contains("cron_abc"));
    assert!(msg.contains("deploy"));
    assert!(msg.contains("Do NOT execute"));
    assert!(msg.contains("AskUserQuestion"));
}

#[tokio::test]
async fn test_save_and_load_durable_jobs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = crate::store::new_cron_store();

    // Insert a durable job
    {
        let mut guard = store.lock().await;
        let mut job = make_job("cron_d1", true, now_unix_secs());
        job.durable = true;
        job.expires_at = Some(now_unix_secs() + 86400);
        guard.insert("cron_d1".to_string(), job);
    }

    // Save
    save_durable_jobs(&store, dir.path()).await.expect("save");

    // Load
    let loaded = load_durable_jobs(dir.path()).await.expect("load");
    assert_eq!(loaded.len(), 1);
    assert!(loaded.contains_key("cron_d1"));
}

#[tokio::test]
async fn test_load_durable_jobs_filters_expired() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = crate::store::new_cron_store();
    let now = now_unix_secs();

    {
        let mut guard = store.lock().await;
        // Job created 4 days ago (past 3-day expiry)
        let mut job = make_job("cron_old", true, now - 4 * 86400);
        job.durable = true;
        guard.insert("cron_old".to_string(), job);

        // Job created 1 day ago (still valid)
        let mut job2 = make_job("cron_new", true, now - 86400);
        job2.durable = true;
        job2.expires_at = Some(now + 2 * 86400);
        guard.insert("cron_new".to_string(), job2);
    }

    save_durable_jobs(&store, dir.path()).await.expect("save");
    let loaded = load_durable_jobs(dir.path()).await.expect("load");

    assert!(
        !loaded.contains_key("cron_old"),
        "Expired job should be filtered"
    );
    assert!(loaded.contains_key("cron_new"), "Valid job should remain");
}
