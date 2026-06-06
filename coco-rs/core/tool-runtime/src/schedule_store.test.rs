use super::*;

#[tokio::test]
async fn in_memory_schedule_store_creates_lists_and_deletes() {
    let store = InMemoryScheduleStore::new();

    let entry = store
        .create_schedule("standup", "0 9 * * *", "summarize work")
        .await
        .unwrap();
    assert_eq!(entry.name, "standup");

    let listed = store.list_schedules().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, entry.id);

    store.delete_schedule(&entry.id).await.unwrap();
    assert!(store.list_schedules().await.unwrap().is_empty());
}

#[tokio::test]
async fn in_memory_trigger_store_round_trips() {
    let store = InMemoryScheduleStore::new();

    let trigger = store
        .create_trigger("deploy", Some("deployment hook"))
        .await
        .unwrap();
    assert_eq!(store.get_trigger(&trigger.id).await.unwrap().name, "deploy");
    assert_eq!(
        store.run_trigger(&trigger.id).await.unwrap(),
        "Triggered deploy"
    );
}
