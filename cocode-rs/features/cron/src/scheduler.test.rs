use std::sync::Arc;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering;

use super::*;
use crate::config::CronConfig;
use crate::store::new_cron_store;
use crate::types::CronJob;
use crate::types::CronJobStatus;

fn make_active_job(id: &str, cron: &str, recurring: bool) -> CronJob {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    CronJob {
        id: id.to_string(),
        cron: cron.to_string(),
        prompt: "test prompt".to_string(),
        description: None,
        recurring,
        durable: false,
        created_at: now,
        execution_count: 0,
        last_executed_at: None,
        expires_at: if recurring { Some(now + 86400) } else { None },
        status: CronJobStatus::Active,
        consecutive_failures: 0,
        next_fire_at: None,
    }
}

#[tokio::test]
async fn test_scheduler_start_stop() {
    let store = new_cron_store();
    let config = CronConfig {
        tick_interval_secs: 1,
        ..CronConfig::default()
    };
    let fire_count = Arc::new(AtomicI32::new(0));
    let fc = fire_count.clone();
    let on_fire = Arc::new(move |_: CronFireEvent| {
        fc.fetch_add(1, Ordering::SeqCst);
    });

    let scheduler = CronScheduler::new(store, config, on_fire);
    scheduler.start();

    // Let it tick a couple times
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    scheduler.stop();
    // Should not panic
}

#[tokio::test]
async fn test_report_execution_result_success_resets_failures() {
    let store = new_cron_store();
    let config = CronConfig::default();
    let on_fire = Arc::new(|_: CronFireEvent| {});

    let mut job = make_active_job("cron_test", "*/5 * * * *", true);
    job.consecutive_failures = 2;
    store.lock().await.insert("cron_test".to_string(), job);

    let scheduler = CronScheduler::new(store.clone(), config, on_fire);
    scheduler.report_execution_result("cron_test", true).await;

    let guard = store.lock().await;
    assert_eq!(guard["cron_test"].consecutive_failures, 0);
}

#[tokio::test]
async fn test_report_execution_result_failure_increments() {
    let store = new_cron_store();
    let config = CronConfig::default();
    let on_fire = Arc::new(|_: CronFireEvent| {});

    let job = make_active_job("cron_test", "*/5 * * * *", true);
    store.lock().await.insert("cron_test".to_string(), job);

    let scheduler = CronScheduler::new(store.clone(), config, on_fire);
    scheduler.report_execution_result("cron_test", false).await;

    let guard = store.lock().await;
    assert_eq!(guard["cron_test"].consecutive_failures, 1);
    assert_eq!(guard["cron_test"].status, CronJobStatus::Active);
}

#[tokio::test]
async fn test_circuit_breaker_disables_after_threshold() {
    let store = new_cron_store();
    let config = CronConfig {
        circuit_breaker_threshold: 3,
        ..CronConfig::default()
    };
    let disabled_count = Arc::new(AtomicI32::new(0));
    let dc = disabled_count.clone();
    let on_fire = Arc::new(|_: CronFireEvent| {});
    let on_disabled = Arc::new(move |_id: String, _failures: i32| {
        dc.fetch_add(1, Ordering::SeqCst);
    });

    let mut job = make_active_job("cron_test", "*/5 * * * *", true);
    job.consecutive_failures = 2; // One more failure will trigger
    store.lock().await.insert("cron_test".to_string(), job);

    let scheduler =
        CronScheduler::new(store.clone(), config, on_fire).with_on_disabled(on_disabled);
    scheduler.report_execution_result("cron_test", false).await;

    let guard = store.lock().await;
    assert_eq!(guard["cron_test"].status, CronJobStatus::Disabled);
    assert_eq!(guard["cron_test"].consecutive_failures, 3);
    assert_eq!(disabled_count.load(Ordering::SeqCst), 1);
}

#[test]
fn test_compute_next_fire_time_every_5_min() {
    // Start from a known timestamp (2026-03-23 14:30:00 UTC)
    let after_ts = 1774375800;
    let next = compute_next_fire_time("*/5 * * * *", after_ts, 0);
    assert!(next.is_some());
    let next = next.unwrap();
    assert!(next > after_ts);
    // Should be within 5 minutes (300 seconds)
    assert!(
        next - after_ts <= 300,
        "Next fire at {next} too far from {after_ts}"
    );
}

#[test]
fn test_compute_next_fire_time_with_jitter() {
    let after_ts = 1774375800;
    let next_no_jitter = compute_next_fire_time("*/5 * * * *", after_ts, 0).unwrap();
    let next_with_jitter = compute_next_fire_time("*/5 * * * *", after_ts, 30).unwrap();
    assert_eq!(next_with_jitter, next_no_jitter + 30);
}

#[tokio::test]
async fn test_check_and_fire_expired_job() {
    let store = new_cron_store();
    let now = Local::now();
    let now_ts = now.timestamp();

    let mut job = make_active_job("cron_exp", "*/5 * * * *", true);
    job.expires_at = Some(now_ts - 100); // already expired
    store.lock().await.insert("cron_exp".to_string(), job);

    let on_fire: Arc<dyn Fn(CronFireEvent) + Send + Sync> = Arc::new(|_: CronFireEvent| {});
    let firing = Arc::new(Mutex::new(std::collections::HashSet::new()));
    let config = CronConfig::default();

    let changed = check_and_fire(&store, &on_fire, None, &firing, &config, now, now_ts).await;
    assert!(changed, "Should report changes for expired job");

    let guard = store.lock().await;
    assert!(guard.is_empty(), "Expired job should be removed");
}
