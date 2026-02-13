use super::*;

#[test]
fn builtin_commands_not_empty() {
    let cmds = builtin_local_commands();
    assert!(!cmds.is_empty());
}

#[test]
fn find_by_name() {
    let cmd = find_local_command("help").expect("help should exist");
    assert_eq!(cmd.name, "help");
}

#[test]
fn find_skills_command() {
    let cmd = find_local_command("skills").expect("skills should exist");
    assert_eq!(cmd.name, "skills");
}

#[test]
fn find_todos_command() {
    let cmd = find_local_command("todos").expect("todos should exist");
    assert_eq!(cmd.name, "todos");
}

#[test]
fn find_todos_by_alias() {
    let cmd = find_local_command("tasks").expect("alias 'tasks' should resolve to todos");
    assert_eq!(cmd.name, "todos");
}

#[test]
fn find_compact_command() {
    let cmd = find_local_command("compact").expect("compact should exist");
    assert_eq!(cmd.name, "compact");
}

#[test]
fn find_by_alias() {
    let cmd = find_local_command("h").expect("alias 'h' should resolve to help");
    assert_eq!(cmd.name, "help");

    let cmd = find_local_command("q").expect("alias 'q' should resolve to exit");
    assert_eq!(cmd.name, "exit");
}

#[test]
fn find_nonexistent_returns_none() {
    assert!(find_local_command("nonexistent").is_none());
}

#[test]
fn to_slash_command_sets_local_type() {
    let cmd = find_local_command("clear").unwrap();
    let slash = cmd.to_slash_command();
    assert_eq!(slash.command_type, CommandType::Local);
    assert_eq!(slash.name, "clear");
}
