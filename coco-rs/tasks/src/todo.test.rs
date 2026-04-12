use super::*;

#[test]
fn test_create_and_list() {
    clear_todos();
    create_todo("Fix bug", "The login bug", None);
    create_todo("Add tests", "Unit tests for auth", None);
    let todos = list_todos();
    assert_eq!(todos.len(), 2);
    clear_todos();
}

#[test]
fn test_update_status() {
    clear_todos();
    let item = create_todo("Task 1", "desc", Some("Working on task 1"));
    update_todo_status(&item.id, TodoStatus::Completed);
    let updated = get_todo(&item.id).unwrap();
    assert_eq!(updated.status, TodoStatus::Completed);
    assert!(updated.completed_at.is_some());
    clear_todos();
}

#[test]
fn test_blocks_relationship() {
    clear_todos();
    let a = create_todo("A", "first", None);
    let b = create_todo("B", "second", None);
    add_blocks(&a.id, &b.id);
    let a_updated = get_todo(&a.id).unwrap();
    let b_updated = get_todo(&b.id).unwrap();
    assert!(a_updated.blocks.contains(&b.id));
    assert!(b_updated.blocked_by.contains(&a.id));
    clear_todos();
}

#[test]
fn test_format_markdown() {
    clear_todos();
    create_todo("Test task", "description", None);
    let todos = list_todos();
    let md = format_todos_markdown(&todos);
    assert!(md.contains("Test task"));
    assert!(md.contains("pending"));
    clear_todos();
}
