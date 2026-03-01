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
    env.add_template("explore_subagent", include_str!("templates/explore_subagent.md"))
        .expect("explore_subagent template must compile");
    env.add_template("plan_subagent", include_str!("templates/plan_subagent.md"))
        .expect("plan_subagent template must compile");
    env.add_template("tool_policy", include_str!("templates/tool_policy.md"))
        .expect("tool_policy template must compile");
    env
});

/// Build the tool names context value for minijinja templates.
fn tool_names_value() -> Value {
    minijinja::context! {
        READ => cocode_protocol::tools::READ,
        READ_MANY_FILES => cocode_protocol::tools::READ_MANY_FILES,
        GLOB => cocode_protocol::tools::GLOB,
        GREP => cocode_protocol::tools::GREP,
        EDIT => cocode_protocol::tools::EDIT,
        WRITE => cocode_protocol::tools::WRITE,
        BASH => cocode_protocol::tools::BASH,
        SHELL => cocode_protocol::tools::SHELL,
        TASK => cocode_protocol::tools::TASK,
        TASK_OUTPUT => cocode_protocol::tools::TASK_OUTPUT,
        TASK_STOP => cocode_protocol::tools::TASK_STOP,
        TODO_WRITE => cocode_protocol::tools::TODO_WRITE,
        ENTER_PLAN_MODE => cocode_protocol::tools::ENTER_PLAN_MODE,
        EXIT_PLAN_MODE => cocode_protocol::tools::EXIT_PLAN_MODE,
        ASK_USER_QUESTION => cocode_protocol::tools::ASK_USER_QUESTION,
        WEB_FETCH => cocode_protocol::tools::WEB_FETCH,
        WEB_SEARCH => cocode_protocol::tools::WEB_SEARCH,
        SKILL => cocode_protocol::tools::SKILL,
        LS => cocode_protocol::tools::LS,
        LSP => cocode_protocol::tools::LSP,
        NOTEBOOK_EDIT => cocode_protocol::tools::NOTEBOOK_EDIT,
        SMART_EDIT => cocode_protocol::tools::SMART_EDIT,
        APPLY_PATCH => cocode_protocol::tools::APPLY_PATCH,
        MCP_SEARCH => cocode_protocol::tools::MCP_SEARCH,
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
