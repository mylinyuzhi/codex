//! Vim state transitions — process key input and update state.

use super::CommandState;
use super::FindType;
use super::Operator;
use super::PersistentState;
use super::TextObjScope;
use super::motions;
use super::operators;
use super::text_objects;
use crate::state::ui::InputState;

/// Result of processing a key in vim mode.
pub enum VimAction {
    /// Key was handled, no further action needed.
    Handled,
    /// Should enter insert mode at current position.
    EnterInsert,
    /// Should enter insert mode at end of line.
    EnterInsertEnd,
    /// Should enter insert mode after cursor.
    EnterInsertAfter,
    /// Should enter insert mode at line start.
    EnterInsertHome,
    /// Open line below and enter insert.
    OpenBelow,
    /// Open line above and enter insert.
    OpenAbove,
    /// Key not handled — pass to default handler.
    Unhandled,
    /// Submit input (Enter in normal mode).
    Submit,
}

/// Process a key in normal mode.
///
/// Returns what action the caller should take.
pub fn process_normal_key(
    ch: char,
    input: &mut InputState,
    command: &mut CommandState,
    persistent: &mut PersistentState,
) -> VimAction {
    match command {
        CommandState::Idle => process_idle_key(ch, input, command, persistent),
        CommandState::Count { digits } => {
            if ch.is_ascii_digit() {
                digits.push(ch);
                VimAction::Handled
            } else {
                let count = digits.parse::<i32>().unwrap_or(1);
                *command = CommandState::Idle;
                process_motion_with_count(ch, input, count, command, persistent)
            }
        }
        CommandState::OperatorPending { op, count } => {
            let op = *op;
            let count = *count;
            process_operator_motion(ch, input, op, count, command, persistent)
        }
        CommandState::OperatorCount { op, count, digits } => {
            if ch.is_ascii_digit() {
                digits.push(ch);
                VimAction::Handled
            } else {
                let total_count = *count * digits.parse::<i32>().unwrap_or(1);
                let op = *op;
                *command = CommandState::Idle;
                process_operator_motion(ch, input, op, total_count, command, persistent)
            }
        }
        CommandState::Find { find, count } => {
            let find = *find;
            let count = *count;
            execute_find(ch, input, find, count, persistent);
            command.reset();
            VimAction::Handled
        }
        CommandState::OperatorFind { op, count, find } => {
            let op = *op;
            let _count = *count;
            let find = *find;
            if let Some(target) = find_target(input, find, ch) {
                let start = input.cursor;
                operators::apply_operator(input, op, start, target, persistent);
            }
            command.reset();
            if op == Operator::Change {
                VimAction::EnterInsert
            } else {
                VimAction::Handled
            }
        }
        CommandState::OperatorTextObj { op, count, scope } => {
            let op = *op;
            let _count = *count;
            let scope = *scope;
            if let Some((start, end)) = resolve_text_object(ch, &input.text, input.cursor, scope) {
                operators::apply_operator(input, op, start, end, persistent);
            }
            command.reset();
            if op == Operator::Change {
                VimAction::EnterInsert
            } else {
                VimAction::Handled
            }
        }
        CommandState::G { count } => {
            let _count = *count;
            if ch == 'g' {
                input.cursor = motions::go_to_top(&input.text, input.cursor);
            }
            command.reset();
            VimAction::Handled
        }
        CommandState::OperatorG { op, count } => {
            let op = *op;
            let _count = *count;
            if ch == 'g' {
                let target = motions::go_to_top(&input.text, input.cursor);
                operators::apply_operator(input, op, input.cursor, target, persistent);
            }
            command.reset();
            if op == Operator::Change {
                VimAction::EnterInsert
            } else {
                VimAction::Handled
            }
        }
        CommandState::Replace { count: _ } => {
            operators::replace_char(input, ch);
            command.reset();
            VimAction::Handled
        }
        CommandState::Indent { dir: _, count: _ } => {
            // Indent not applicable in single-line mode
            command.reset();
            VimAction::Handled
        }
    }
}

/// Process a key when command state is idle.
fn process_idle_key(
    ch: char,
    input: &mut InputState,
    command: &mut CommandState,
    persistent: &mut PersistentState,
) -> VimAction {
    match ch {
        // Mode switches
        'i' => VimAction::EnterInsert,
        'I' => VimAction::EnterInsertHome,
        'a' => VimAction::EnterInsertAfter,
        'A' => VimAction::EnterInsertEnd,
        'o' => VimAction::OpenBelow,
        'O' => VimAction::OpenAbove,

        // Motions
        'h' => {
            input.cursor_left();
            VimAction::Handled
        }
        'l' => {
            input.cursor_right();
            VimAction::Handled
        }
        'w' => {
            input.cursor = motions::word_forward(&input.text, input.cursor);
            VimAction::Handled
        }
        'b' => {
            input.cursor = motions::word_backward(&input.text, input.cursor);
            VimAction::Handled
        }
        'e' => {
            input.cursor = motions::word_end(&input.text, input.cursor);
            VimAction::Handled
        }
        '0' => {
            input.cursor_home();
            VimAction::Handled
        }
        '^' => {
            input.cursor = motions::first_non_blank(&input.text, input.cursor);
            VimAction::Handled
        }
        '$' => {
            input.cursor_end();
            VimAction::Handled
        }
        'G' => {
            input.cursor = motions::go_to_bottom(&input.text, input.cursor);
            VimAction::Handled
        }

        // Operators
        'd' => {
            *command = CommandState::OperatorPending {
                op: Operator::Delete,
                count: 1,
            };
            VimAction::Handled
        }
        'c' => {
            *command = CommandState::OperatorPending {
                op: Operator::Change,
                count: 1,
            };
            VimAction::Handled
        }
        'y' => {
            *command = CommandState::OperatorPending {
                op: Operator::Yank,
                count: 1,
            };
            VimAction::Handled
        }

        // Single-key operations
        'x' => {
            // Delete char under cursor
            input.delete_forward();
            VimAction::Handled
        }
        'r' => {
            *command = CommandState::Replace { count: 1 };
            VimAction::Handled
        }
        'p' => {
            operators::put_after(input, persistent);
            VimAction::Handled
        }
        'P' => {
            operators::put_before(input, persistent);
            VimAction::Handled
        }
        'u' => {
            // Undo not implemented — would need undo stack
            VimAction::Handled
        }

        // Find
        'f' => {
            *command = CommandState::Find {
                find: FindType::F,
                count: 1,
            };
            VimAction::Handled
        }
        'F' => {
            *command = CommandState::Find {
                find: FindType::BigF,
                count: 1,
            };
            VimAction::Handled
        }
        't' => {
            *command = CommandState::Find {
                find: FindType::T,
                count: 1,
            };
            VimAction::Handled
        }
        'T' => {
            *command = CommandState::Find {
                find: FindType::BigT,
                count: 1,
            };
            VimAction::Handled
        }

        // Repeat last find
        ';' => {
            if let Some((find, ch)) = persistent.last_find {
                execute_find(ch, input, find, 1, persistent);
            }
            VimAction::Handled
        }
        ',' => {
            if let Some((find, ch)) = persistent.last_find {
                let reverse = match find {
                    FindType::F => FindType::BigF,
                    FindType::BigF => FindType::F,
                    FindType::T => FindType::BigT,
                    FindType::BigT => FindType::T,
                };
                execute_find(ch, input, reverse, 1, persistent);
            }
            VimAction::Handled
        }

        // G prefix
        'g' => {
            *command = CommandState::G { count: 1 };
            VimAction::Handled
        }

        // Count
        '1'..='9' => {
            *command = CommandState::Count {
                digits: ch.to_string(),
            };
            VimAction::Handled
        }

        // Enter submits in normal mode
        '\n' | '\r' => VimAction::Submit,

        _ => VimAction::Unhandled,
    }
}

fn process_motion_with_count(
    ch: char,
    input: &mut InputState,
    count: i32,
    command: &mut CommandState,
    persistent: &mut PersistentState,
) -> VimAction {
    // Apply count to motions
    for _ in 0..count {
        match process_idle_key(ch, input, command, persistent) {
            VimAction::Handled => {}
            other => return other,
        }
    }
    VimAction::Handled
}

fn process_operator_motion(
    ch: char,
    input: &mut InputState,
    op: Operator,
    count: i32,
    command: &mut CommandState,
    persistent: &mut PersistentState,
) -> VimAction {
    match ch {
        // Motions
        'w' | 'e' | 'b' | 'h' | 'l' | '0' | '$' | '^' | 'G' => {
            let start = input.cursor;
            let mut target = start;
            // 'w' and 'b' are exclusive motions (delete up to, not including target)
            let is_exclusive = matches!(ch, 'w' | 'b');
            for _ in 0..count {
                target = match ch {
                    'w' => motions::word_forward(&input.text, target),
                    'e' => motions::word_end(&input.text, target),
                    'b' => motions::word_backward(&input.text, target),
                    'h' => (target - 1).max(0),
                    'l' => (target + 1).min(input.text.chars().count() as i32 - 1),
                    '0' => 0,
                    '$' => motions::line_end(&input.text, target),
                    '^' => motions::first_non_blank(&input.text, target),
                    'G' => motions::go_to_bottom(&input.text, target),
                    _ => target,
                };
            }
            // Exclusive motions: adjust target to not include the target char
            let adjusted = if is_exclusive && target > start {
                target - 1
            } else if is_exclusive && target < start {
                target + 1
            } else {
                target
            };
            operators::apply_operator(input, op, start, adjusted, persistent);
            command.reset();
            if op == Operator::Change {
                VimAction::EnterInsert
            } else {
                VimAction::Handled
            }
        }
        // Double operator = line operation (dd, cc, yy)
        'd' if op == Operator::Delete => {
            input.text.clear();
            input.cursor = 0;
            command.reset();
            VimAction::Handled
        }
        'c' if op == Operator::Change => {
            input.text.clear();
            input.cursor = 0;
            command.reset();
            VimAction::EnterInsert
        }
        'y' if op == Operator::Yank => {
            persistent.register = input.text.clone();
            persistent.register_is_linewise = true;
            command.reset();
            VimAction::Handled
        }
        // Text object scope
        'i' => {
            *command = CommandState::OperatorTextObj {
                op,
                count,
                scope: TextObjScope::Inner,
            };
            VimAction::Handled
        }
        'a' => {
            *command = CommandState::OperatorTextObj {
                op,
                count,
                scope: TextObjScope::Around,
            };
            VimAction::Handled
        }
        // Find within operator
        'f' => {
            *command = CommandState::OperatorFind {
                op,
                count,
                find: FindType::F,
            };
            VimAction::Handled
        }
        'F' => {
            *command = CommandState::OperatorFind {
                op,
                count,
                find: FindType::BigF,
            };
            VimAction::Handled
        }
        't' => {
            *command = CommandState::OperatorFind {
                op,
                count,
                find: FindType::T,
            };
            VimAction::Handled
        }
        'T' => {
            *command = CommandState::OperatorFind {
                op,
                count,
                find: FindType::BigT,
            };
            VimAction::Handled
        }
        // Count within operator
        '0'..='9' => {
            *command = CommandState::OperatorCount {
                op,
                count,
                digits: ch.to_string(),
            };
            VimAction::Handled
        }
        // g within operator
        'g' => {
            *command = CommandState::OperatorG { op, count };
            VimAction::Handled
        }
        _ => {
            command.reset();
            VimAction::Unhandled
        }
    }
}

fn execute_find(
    ch: char,
    input: &mut InputState,
    find: FindType,
    _count: i32,
    persistent: &mut PersistentState,
) {
    if let Some(target) = find_target(input, find, ch) {
        input.cursor = target;
    }
    persistent.last_find = Some((find, ch));
}

fn find_target(input: &InputState, find: FindType, ch: char) -> Option<i32> {
    match find {
        FindType::F => motions::find_char_forward(&input.text, input.cursor, ch),
        FindType::BigF => motions::find_char_backward(&input.text, input.cursor, ch),
        FindType::T => motions::till_char_forward(&input.text, input.cursor, ch),
        FindType::BigT => motions::till_char_backward(&input.text, input.cursor, ch),
    }
}

fn resolve_text_object(ch: char, text: &str, pos: i32, scope: TextObjScope) -> Option<(i32, i32)> {
    match ch {
        'w' => text_objects::word(text, pos, scope),
        '"' => text_objects::quoted(text, pos, '"', scope),
        '\'' => text_objects::quoted(text, pos, '\'', scope),
        '(' | ')' | 'b' => text_objects::bracket(text, pos, '(', ')', scope),
        '{' | '}' | 'B' => text_objects::bracket(text, pos, '{', '}', scope),
        '[' | ']' => text_objects::bracket(text, pos, '[', ']', scope),
        _ => None,
    }
}
