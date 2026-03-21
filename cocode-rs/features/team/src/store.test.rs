use crate::types::MemberStatus;
use crate::types::Team;
use crate::types::TeamMember;

use super::*;

fn make_team(name: &str) -> Team {
    Team {
        name: name.to_string(),
        description: Some("test".to_string()),
        agent_type: None,
        leader_agent_id: None,
        members: Vec::new(),
        created_at: 0,
    }
}

fn make_member(id: &str) -> TeamMember {
    TeamMember {
        agent_id: id.to_string(),
        name: Some(id.to_string()),
        agent_type: None,
        model: None,
        joined_at: 0,
        cwd: None,
        status: MemberStatus::Active,
        background: false,
    }
}

#[tokio::test]
async fn create_and_get_team_in_memory() {
    let store = TeamStore::new(PathBuf::from("/tmp/unused"), false);
    store.create_team(make_team("t1")).await.unwrap();

    let team = store.get_team("t1").await;
    assert!(team.is_some());
    assert_eq!(team.unwrap().name, "t1");
}

#[tokio::test]
async fn create_duplicate_team_errors() {
    let store = TeamStore::new(PathBuf::from("/tmp/unused"), false);
    store.create_team(make_team("t1")).await.unwrap();
    let err = store.create_team(make_team("t1")).await;
    assert!(err.is_err());
}

#[tokio::test]
async fn delete_team() {
    let store = TeamStore::new(PathBuf::from("/tmp/unused"), false);
    store.create_team(make_team("t1")).await.unwrap();
    store.delete_team("t1").await.unwrap();
    assert!(store.get_team("t1").await.is_none());
}

#[tokio::test]
async fn delete_nonexistent_team_errors() {
    let store = TeamStore::new(PathBuf::from("/tmp/unused"), false);
    let err = store.delete_team("ghost").await;
    assert!(err.is_err());
}

#[tokio::test]
async fn add_and_remove_member() {
    let store = TeamStore::new(PathBuf::from("/tmp/unused"), false);
    store.create_team(make_team("t1")).await.unwrap();

    store.add_member("t1", make_member("a1"), 10).await.unwrap();
    let team = store.get_team("t1").await.unwrap();
    assert_eq!(team.members.len(), 1);

    store.remove_member("t1", "a1").await.unwrap();
    let team = store.get_team("t1").await.unwrap();
    assert!(team.members.is_empty());
}

#[tokio::test]
async fn add_member_max_reached() {
    let store = TeamStore::new(PathBuf::from("/tmp/unused"), false);
    store.create_team(make_team("t1")).await.unwrap();
    store.add_member("t1", make_member("a1"), 1).await.unwrap();
    let err = store.add_member("t1", make_member("a2"), 1).await;
    assert!(err.is_err());
}

#[tokio::test]
async fn update_member_status() {
    let store = TeamStore::new(PathBuf::from("/tmp/unused"), false);
    store.create_team(make_team("t1")).await.unwrap();
    store.add_member("t1", make_member("a1"), 10).await.unwrap();

    store
        .update_member_status("t1", "a1", MemberStatus::Idle)
        .await
        .unwrap();

    let team = store.get_team("t1").await.unwrap();
    assert_eq!(team.members[0].status, MemberStatus::Idle);
}

#[tokio::test]
async fn list_teams() {
    let store = TeamStore::new(PathBuf::from("/tmp/unused"), false);
    store.create_team(make_team("alpha")).await.unwrap();
    store.create_team(make_team("beta")).await.unwrap();
    let teams = store.list_teams().await;
    assert_eq!(teams.len(), 2);
    assert!(teams.contains_key("alpha"));
    assert!(teams.contains_key("beta"));
}

#[tokio::test]
async fn snapshot_returns_valid_json() {
    let store = TeamStore::new(PathBuf::from("/tmp/unused"), false);
    store.create_team(make_team("t1")).await.unwrap();
    let snap = store.snapshot().await;
    assert!(snap.is_object());
    assert!(snap.get("t1").is_some());
}

#[tokio::test]
async fn persist_and_load_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().to_path_buf();

    // Create and persist
    let store = TeamStore::new(base.clone(), true);
    let mut team = make_team("persist-test");
    team.description = Some("persisted".into());
    store.create_team(team).await.unwrap();
    store
        .add_member("persist-test", make_member("a1"), 10)
        .await
        .unwrap();

    // Load in new store
    let store2 = TeamStore::new(base, true);
    store2.load_from_disk().await.unwrap();
    let loaded = store2.get_team("persist-test").await.unwrap();
    assert_eq!(loaded.description.as_deref(), Some("persisted"));
    assert_eq!(loaded.members.len(), 1);
    assert_eq!(loaded.members[0].agent_id, "a1");
}
