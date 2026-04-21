use super::PlanPanel;
use crate::theme::Theme;
use crate::widgets::task_list::TaskDisplayStatus;
use crate::widgets::task_list::TaskDisplayType;
use crate::widgets::task_list::TaskEntry;
use coco_types::TaskListStatus;
use coco_types::TaskRecord;
use coco_types::TodoRecord;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use std::collections::HashMap;

fn task(id: &str, subject: &str, status: TaskListStatus, owner: Option<&str>) -> TaskRecord {
    TaskRecord {
        id: id.into(),
        subject: subject.into(),
        description: String::new(),
        active_form: None,
        owner: owner.map(String::from),
        status,
        blocks: Vec::new(),
        blocked_by: Vec::new(),
        metadata: None,
    }
}

fn todo(content: &str, status: &str) -> TodoRecord {
    TodoRecord {
        content: content.into(),
        status: status.into(),
        active_form: format!("Doing {content}"),
    }
}

fn running(id: &str, name: &str, status: TaskDisplayStatus) -> TaskEntry {
    TaskEntry {
        id: id.into(),
        name: name.into(),
        status,
        task_type: TaskDisplayType::Shell,
        progress: None,
        elapsed_ms: 1500,
    }
}

fn render(panel: PlanPanel<'_>, w: u16, h: u16) -> String {
    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| frame.render_widget(panel, Rect::new(0, 0, w, h)))
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..h {
        for x in 0..w {
            out.push(buf[(x, y)].symbol().chars().next().unwrap_or(' '));
        }
        out.push('\n');
    }
    out
}

#[test]
fn has_content_is_false_when_all_sections_empty() {
    let todos: HashMap<String, Vec<TodoRecord>> = HashMap::new();
    let theme = Theme::default();
    let panel = PlanPanel::new(&[], &todos, &[], &theme);
    assert!(!panel.has_content());
}

#[test]
fn has_content_true_when_any_section_populated() {
    let theme = Theme::default();
    let plan = vec![task("1", "a", TaskListStatus::Pending, None)];
    let todos: HashMap<String, Vec<TodoRecord>> = HashMap::new();
    let running_vec: Vec<TaskEntry> = Vec::new();
    assert!(PlanPanel::new(&plan, &todos, &running_vec, &theme).has_content());
}

#[test]
fn renders_all_three_sections() {
    let plan = vec![
        task(
            "1",
            "Design schema",
            TaskListStatus::Completed,
            Some("alice"),
        ),
        task("2", "Wire tool", TaskListStatus::InProgress, Some("bob")),
        task("3", "Add tests", TaskListStatus::Pending, None),
    ];
    let mut todos: HashMap<String, Vec<TodoRecord>> = HashMap::new();
    todos.insert(
        "main-session".into(),
        vec![
            todo("check CLAUDE.md", "pending"),
            todo("run just fmt", "completed"),
        ],
    );
    let running_tasks = vec![running("bg-1", "npm test", TaskDisplayStatus::Running)];
    let theme = Theme::default();
    let panel = PlanPanel::new(&plan, &todos, &running_tasks, &theme);
    let output = render(panel, 60, 15);

    assert!(output.contains("Plan items"), "section header missing");
    assert!(output.contains("Design schema"), "plan task missing");
    assert!(output.contains("alice"), "owner missing");
    assert!(output.contains("Todos"), "todos section missing");
    assert!(output.contains("main-session"), "agent key missing");
    assert!(output.contains("check CLAUDE.md"), "todo content missing");
    assert!(output.contains("Running"), "running section missing");
    assert!(output.contains("npm test"), "running task name missing");
}

#[test]
fn renders_empty_state_when_nothing_to_show() {
    let todos: HashMap<String, Vec<TodoRecord>> = HashMap::new();
    let theme = Theme::default();
    let panel = PlanPanel::new(&[], &todos, &[], &theme);
    let output = render(panel, 40, 4);
    assert!(
        output.contains("No tasks or todos"),
        "empty marker missing: {output:?}"
    );
}

#[test]
fn renders_blocked_by_marker_when_task_is_blocked() {
    let mut blocked = task("2", "Second", TaskListStatus::Pending, None);
    blocked.blocked_by.push("1".into());
    let plan = vec![task("1", "First", TaskListStatus::Pending, None), blocked];
    let todos: HashMap<String, Vec<TodoRecord>> = HashMap::new();
    let theme = Theme::default();
    let panel = PlanPanel::new(&plan, &todos, &[], &theme);
    let output = render(panel, 60, 8);
    assert!(
        output.contains("blocked by"),
        "blocked marker should appear: {output:?}"
    );
}
