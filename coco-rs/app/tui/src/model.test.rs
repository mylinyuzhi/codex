use crate::model::AppModel;
use crate::model::DisplayRole;

#[test]
fn test_push_entries() {
    let mut model = AppModel::default();
    model.push_user_text("hello");
    model.push_assistant_text("hi there");
    model.push_system("welcome");
    assert_eq!(model.entries.len(), 3);
    assert_eq!(model.entries[0].role, DisplayRole::User);
    assert_eq!(model.entries[1].role, DisplayRole::Assistant);
    assert_eq!(model.entries[2].role, DisplayRole::System);
}

#[test]
fn test_input_editing() {
    let mut model = AppModel::default();
    model.insert_char('h');
    model.insert_char('i');
    assert_eq!(model.input, "hi");
    assert_eq!(model.cursor, 2);

    model.backspace();
    assert_eq!(model.input, "h");
    assert_eq!(model.cursor, 1);
}

#[test]
fn test_take_input() {
    let mut model = AppModel::default();
    model.insert_char('x');
    model.insert_char('y');
    let text = model.take_input();
    assert_eq!(text, "xy");
    assert!(model.input.is_empty());
    assert_eq!(model.cursor, 0);
}

#[test]
fn test_backspace_empty() {
    let mut model = AppModel::default();
    model.backspace(); // should not panic
    assert!(model.input.is_empty());
}
