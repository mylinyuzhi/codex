use super::*;

#[test]
fn test_concurrency_safety() {
    use cocode_cron::new_cron_store;
    let tool = CronDeleteTool::new(new_cron_store());
    assert!(tool.is_concurrent_safe());
}
