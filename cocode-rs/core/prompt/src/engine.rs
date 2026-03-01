//! Template engine backed by minijinja.
//!
//! Provides a lazily-initialized `Environment` with compile-time embedded
//! templates. All templates produce Markdown output (auto-escaping disabled).

use std::sync::LazyLock;

use minijinja::Environment;
use minijinja::Value;

#[allow(clippy::expect_used)]
static ENGINE: LazyLock<Environment<'static>> = LazyLock::new(|| {
    let mut env = Environment::new();
    // Markdown output — disable HTML auto-escaping.
    env.set_auto_escape_callback(|_| minijinja::AutoEscape::None);

    // Add tool names as global context for all templates
    env.add_global("tools", tool_names_value());

    env.add_template("environment", include_str!("templates/environment.md"))
        .expect("environment template must compile");
    env.add_template(
        "tool_policy_lines",
        include_str!("templates/tool_policy_lines.md"),
    )
    .expect("tool_policy_lines template must compile");
    env.add_template("memory_files", include_str!("templates/memory_files.md"))
        .expect("memory_files template must compile");
    env.add_template(
        "explore_subagent",
        include_str!("templates/explore_subagent.md"),
    )
    .expect("explore_subagent template must compile");
    env.add_template("plan_subagent", include_str!("templates/plan_subagent.md"))
        .expect("plan_subagent template must compile");
    env.add_template("tool_policy", include_str!("templates/tool_policy.md"))
        .expect("tool_policy template must compile");
    env
});

/// Build the tool names context value for minijinja templates.
fn tool_names_value() -> Value {
    use cocode_protocol::ToolName;
    minijinja::context! {
        READ => ToolName::Read.as_str(),
        READ_MANY_FILES => ToolName::ReadManyFiles.as_str(),
        GLOB => ToolName::Glob.as_str(),
        GREP => ToolName::Grep.as_str(),
        EDIT => ToolName::Edit.as_str(),
        WRITE => ToolName::Write.as_str(),
        BASH => ToolName::Bash.as_str(),
        SHELL => ToolName::Shell.as_str(),
        TASK => ToolName::Task.as_str(),
        TASK_OUTPUT => ToolName::TaskOutput.as_str(),
        TASK_STOP => ToolName::TaskStop.as_str(),
        TODO_WRITE => ToolName::TodoWrite.as_str(),
        ENTER_PLAN_MODE => ToolName::EnterPlanMode.as_str(),
        EXIT_PLAN_MODE => ToolName::ExitPlanMode.as_str(),
        ASK_USER_QUESTION => ToolName::AskUserQuestion.as_str(),
        WEB_FETCH => ToolName::WebFetch.as_str(),
        WEB_SEARCH => ToolName::WebSearch.as_str(),
        SKILL => ToolName::Skill.as_str(),
        LS => ToolName::LS.as_str(),
        LSP => ToolName::Lsp.as_str(),
        NOTEBOOK_EDIT => ToolName::NotebookEdit.as_str(),
        SMART_EDIT => ToolName::SmartEdit.as_str(),
        APPLY_PATCH => ToolName::ApplyPatch.as_str(),
        MCP_SEARCH => ToolName::McpSearch.as_str(),
    }
}

/// Render a named template with the given context.
///
/// Panics if the template or context is invalid (developer bug).
#[allow(clippy::expect_used)]
pub fn render(name: &str, ctx: minijinja::Value) -> String {
    let tmpl = ENGINE.get_template(name).expect("template must exist");
    tmpl.render(ctx).expect("template render must succeed")
}
