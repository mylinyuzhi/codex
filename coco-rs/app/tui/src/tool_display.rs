//! Tool input display helpers shared by permission prompts and chat previews.

use coco_types::MCP_TOOL_SEPARATOR;
use coco_types::PermissionDisplayInput;
use coco_types::ToolName;
use serde_json::Value;
use std::str::FromStr;

const TOOL_INPUT_PREVIEW_MAX_CHARS: usize = 512;
const PERMISSION_DISPLAY_MAX_CHARS: usize = 1_200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolNameTone {
    ReadOnly,
    Shell,
    Write,
    Agent,
    Plan,
    Utility,
}

pub fn permission_display_input(tool_name: &str, input: &Value) -> PermissionDisplayInput {
    if is_shell_tool(tool_name)
        && let Some(command) = input.get("command").and_then(Value::as_str)
    {
        return PermissionDisplayInput::Command(single_line_capped(
            command,
            PERMISSION_DISPLAY_MAX_CHARS,
        ));
    }

    let display = multi_line_tool_input(tool_name, input, PERMISSION_DISPLAY_MAX_CHARS);
    if display.is_empty() {
        PermissionDisplayInput::Empty
    } else {
        PermissionDisplayInput::Text(display)
    }
}

pub fn tool_input_preview(tool_name: &str, input: &Value) -> String {
    single_line_capped(
        &single_line_tool_input(tool_name, input),
        TOOL_INPUT_PREVIEW_MAX_CHARS,
    )
}

pub fn tool_name_tone(tool_name: &str) -> ToolNameTone {
    let Some(tool) = normalized_builtin_tool(tool_name) else {
        return ToolNameTone::Utility;
    };

    match tool {
        ToolName::Read
        | ToolName::Glob
        | ToolName::Grep
        | ToolName::WebFetch
        | ToolName::WebSearch
        | ToolName::TaskGet
        | ToolName::TaskList
        | ToolName::TaskOutput
        | ToolName::ToolSearch
        | ToolName::Lsp
        | ToolName::ListMcpResources
        | ToolName::ReadMcpResource
        | ToolName::CronList => ToolNameTone::ReadOnly,
        ToolName::Bash | ToolName::PowerShell | ToolName::Repl => ToolNameTone::Shell,
        ToolName::Write
        | ToolName::Edit
        | ToolName::NotebookEdit
        | ToolName::ApplyPatch
        | ToolName::TodoWrite
        | ToolName::TaskCreate
        | ToolName::TaskUpdate
        | ToolName::TaskStop
        | ToolName::SendMessage
        | ToolName::TeamCreate
        | ToolName::TeamDelete
        | ToolName::Config
        | ToolName::CronCreate
        | ToolName::CronDelete
        | ToolName::RemoteTrigger => ToolNameTone::Write,
        ToolName::Agent | ToolName::Skill => ToolNameTone::Agent,
        ToolName::EnterPlanMode
        | ToolName::ExitPlanMode
        | ToolName::VerifyPlanExecution
        | ToolName::EnterWorktree
        | ToolName::ExitWorktree => ToolNameTone::Plan,
        ToolName::AskUserQuestion
        | ToolName::McpAuth
        | ToolName::Brief
        | ToolName::Sleep
        | ToolName::StructuredOutput => ToolNameTone::Utility,
    }
}

fn is_shell_tool(tool_name: &str) -> bool {
    matches!(
        normalized_builtin_tool(tool_name),
        Some(ToolName::Bash | ToolName::PowerShell)
    )
}

fn normalized_builtin_tool(tool_name: &str) -> Option<ToolName> {
    let normalized = tool_name
        .rsplit(MCP_TOOL_SEPARATOR)
        .next()
        .unwrap_or(tool_name);
    ToolName::from_str(normalized).ok()
}

pub(crate) fn single_line_tool_input(tool_name: &str, input: &Value) -> String {
    let Some(tool) = normalized_builtin_tool(tool_name) else {
        return object_summary(input);
    };
    if matches!(tool, ToolName::Bash | ToolName::PowerShell)
        && let Some(command) = input.get("command").and_then(Value::as_str)
    {
        return command.to_string();
    }

    match tool {
        ToolName::Glob => join_existing(input, &["pattern", "path"], " in "),
        ToolName::Grep => join_existing(input, &["pattern", "path"], " in "),
        ToolName::Read => scalar_value(input, "file_path")
            .or_else(|| scalar_value(input, "path"))
            .unwrap_or_default(),
        ToolName::Edit | ToolName::Write | ToolName::NotebookEdit => {
            scalar_value(input, "file_path")
                .or_else(|| scalar_value(input, "path"))
                .unwrap_or_default()
        }
        ToolName::WebFetch => scalar_value(input, "url").unwrap_or_default(),
        ToolName::WebSearch => scalar_value(input, "query").unwrap_or_default(),
        ToolName::Agent => scalar_value(input, "description")
            .or_else(|| scalar_value(input, "prompt"))
            .unwrap_or_default(),
        _ => object_summary(input),
    }
}

fn multi_line_tool_input(tool_name: &str, input: &Value, max_chars: usize) -> String {
    let Some(tool) = normalized_builtin_tool(tool_name) else {
        return capped_lines(object_lines(input), max_chars);
    };
    if matches!(tool, ToolName::Bash | ToolName::PowerShell)
        && let Some(command) = input.get("command").and_then(Value::as_str)
    {
        return single_line_capped(command, max_chars);
    }

    let keys: &[&str] = match tool {
        ToolName::Glob => &["path", "pattern"],
        ToolName::Grep => &["path", "pattern", "output_mode"],
        ToolName::Read => &["file_path", "offset", "limit"],
        ToolName::Edit => &["file_path", "old_string", "new_string"],
        ToolName::Write => &["file_path"],
        ToolName::NotebookEdit => &["file_path", "cell_id"],
        ToolName::WebFetch => &["url", "prompt"],
        ToolName::WebSearch => &["query"],
        ToolName::Agent => &["description", "subagent_type", "prompt"],
        _ => &[],
    };

    let mut lines = Vec::new();
    for key in keys {
        if let Some(value) = scalar_value(input, key) {
            lines.push(format!("{key}: {value}"));
        }
    }
    if lines.is_empty() {
        lines = object_lines(input);
    }

    capped_lines(lines, max_chars)
}

fn join_existing(input: &Value, keys: &[&str], separator: &str) -> String {
    keys.iter()
        .filter_map(|key| scalar_value(input, key))
        .collect::<Vec<_>>()
        .join(separator)
}

fn scalar_value(input: &Value, key: &str) -> Option<String> {
    value_to_display(input.get(key)?)
}

fn object_summary(input: &Value) -> String {
    let lines = object_lines(input);
    if !lines.is_empty() {
        return lines.join(", ");
    }
    match input {
        Value::Null => String::new(),
        other => value_to_display(other).unwrap_or_default(),
    }
}

fn object_lines(input: &Value) -> Vec<String> {
    let Some(obj) = input.as_object() else {
        return Vec::new();
    };

    obj.iter()
        .filter_map(|(key, value)| value_to_display(value).map(|value| format!("{key}: {value}")))
        .collect()
}

fn value_to_display(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::Array(values) => {
            let parts = values
                .iter()
                .filter_map(value_to_display)
                .collect::<Vec<_>>();
            (!parts.is_empty()).then(|| parts.join(", "))
        }
        Value::Object(_) => None,
    }
}

fn capped_lines(lines: Vec<String>, max_chars: usize) -> String {
    let mut out = String::new();
    let mut count = 0usize;

    for line in lines {
        let line = single_line_capped(&line, max_chars);
        let separator = usize::from(!out.is_empty());
        let line_len = line.chars().count();
        if count + separator + line_len > max_chars {
            if max_chars > 3 {
                while count + 3 > max_chars {
                    out.pop();
                    count = count.saturating_sub(1);
                }
                out.push_str("...");
            }
            return out;
        }
        if separator == 1 {
            out.push('\n');
            count += 1;
        }
        out.push_str(&line);
        count += line_len;
    }

    out
}

fn single_line_capped(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let mut out = String::new();
    let mut pending_space = false;
    let mut count = 0usize;
    for chunk in text.split_whitespace() {
        let space = if out.is_empty() || !pending_space {
            0
        } else {
            1
        };
        let chunk_len = chunk.chars().count();
        if count + space + chunk_len > max_chars {
            if max_chars > 3 {
                while count + 3 > max_chars {
                    out.pop();
                    count = count.saturating_sub(1);
                }
                out.push_str("...");
            }
            return out;
        }
        if space == 1 {
            out.push(' ');
            count += 1;
        }
        out.push_str(chunk);
        count += chunk_len;
        pending_space = true;
    }

    out
}

#[cfg(test)]
#[path = "tool_display.test.rs"]
mod tests;
