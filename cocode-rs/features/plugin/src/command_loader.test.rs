use super::*;
use crate::command::CommandHandler;
use std::fs;

#[test]
fn test_load_commands_from_empty_dir() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let results = load_commands_from_dir(tmp.path(), "test-plugin");
    assert!(results.is_empty());
}

#[test]
fn test_load_commands_from_nonexistent_dir() {
    let results = load_commands_from_dir(Path::new("/nonexistent"), "test-plugin");
    assert!(results.is_empty());
}

#[test]
fn test_load_command_shell() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let cmd_dir = tmp.path().join("build");
    fs::create_dir_all(&cmd_dir).expect("mkdir");

    fs::write(
        cmd_dir.join("command.json"),
        r#"{
  "name": "build",
  "description": "Build the project",
  "handler": {
    "type": "shell",
    "command": "cargo build",
    "timeout_sec": 300
  }
}"#,
    )
    .expect("write");

    let results = load_commands_from_dir(tmp.path(), "test-plugin");
    assert_eq!(results.len(), 1);

    if let PluginContribution::Command {
        command,
        plugin_name,
    } = &results[0]
    {
        assert_eq!(command.name, "build");
        assert_eq!(command.description, "Build the project");
        assert!(command.visible);
        assert_eq!(plugin_name, "test-plugin");

        if let CommandHandler::Shell {
            command: cmd,
            timeout_sec,
        } = &command.handler
        {
            assert_eq!(cmd, "cargo build");
            assert_eq!(*timeout_sec, Some(300));
        } else {
            panic!("Expected Shell handler");
        }
    } else {
        panic!("Expected Command contribution");
    }
}

#[test]
fn test_load_command_skill() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let cmd_dir = tmp.path().join("review");
    fs::create_dir_all(&cmd_dir).expect("mkdir");

    fs::write(
        cmd_dir.join("command.json"),
        r#"{
  "name": "review",
  "description": "Review code changes",
  "handler": {
    "type": "skill",
    "skill_name": "code-review"
  }
}"#,
    )
    .expect("write");

    let results = load_commands_from_dir(tmp.path(), "test-plugin");
    assert_eq!(results.len(), 1);

    if let PluginContribution::Command { command, .. } = &results[0] {
        if let CommandHandler::Skill { skill_name } = &command.handler {
            assert_eq!(skill_name, "code-review");
        } else {
            panic!("Expected Skill handler");
        }
    } else {
        panic!("Expected Command contribution");
    }
}

#[test]
fn test_load_command_agent() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let cmd_dir = tmp.path().join("explore");
    fs::create_dir_all(&cmd_dir).expect("mkdir");

    fs::write(
        cmd_dir.join("command.json"),
        r#"{
  "name": "explore",
  "description": "Explore codebase",
  "handler": {
    "type": "agent",
    "agent_type": "explore"
  }
}"#,
    )
    .expect("write");

    let results = load_commands_from_dir(tmp.path(), "test-plugin");
    assert_eq!(results.len(), 1);

    if let PluginContribution::Command { command, .. } = &results[0] {
        if let CommandHandler::Agent { agent_type } = &command.handler {
            assert_eq!(agent_type, "explore");
        } else {
            panic!("Expected Agent handler");
        }
    } else {
        panic!("Expected Command contribution");
    }
}

#[test]
fn test_load_command_invalid_json() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let cmd_dir = tmp.path().join("invalid");
    fs::create_dir_all(&cmd_dir).expect("mkdir");

    fs::write(cmd_dir.join("command.json"), "invalid { json").expect("write");

    let results = load_commands_from_dir(tmp.path(), "test-plugin");
    assert!(results.is_empty()); // Invalid JSON should be skipped
}

#[test]
fn test_load_multiple_commands() {
    let tmp = tempfile::tempdir().expect("create temp dir");

    // Command 1
    let cmd1 = tmp.path().join("cmd1");
    fs::create_dir_all(&cmd1).expect("mkdir");
    fs::write(
        cmd1.join("command.json"),
        r#"{
  "name": "cmd1",
  "description": "First command",
  "handler": {
    "type": "shell",
    "command": "echo 1"
  }
}"#,
    )
    .expect("write");

    // Command 2
    let cmd2 = tmp.path().join("cmd2");
    fs::create_dir_all(&cmd2).expect("mkdir");
    fs::write(
        cmd2.join("command.json"),
        r#"{
  "name": "cmd2",
  "description": "Second command",
  "handler": {
    "type": "shell",
    "command": "echo 2"
  }
}"#,
    )
    .expect("write");

    let results = load_commands_from_dir(tmp.path(), "test-plugin");
    assert_eq!(results.len(), 2);

    let names: Vec<&str> = results
        .iter()
        .filter_map(|c| {
            if let PluginContribution::Command { command, .. } = c {
                Some(command.name.as_str())
            } else {
                None
            }
        })
        .collect();
    assert!(names.contains(&"cmd1"));
    assert!(names.contains(&"cmd2"));
}
