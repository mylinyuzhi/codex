//! Template engine backed by minijinja.
//!
//! Provides a lazily-initialized `Environment` with compile-time embedded
//! templates. All templates produce Markdown output (auto-escaping disabled).

use std::sync::LazyLock;

use minijinja::Environment;

#[allow(clippy::expect_used)]
static ENGINE: LazyLock<Environment<'static>> = LazyLock::new(|| {
    let mut env = Environment::new();
    // Markdown output — disable HTML auto-escaping.
    env.set_auto_escape_callback(|_| minijinja::AutoEscape::None);
    env.add_template("environment", include_str!("templates/environment.md"))
        .expect("environment template must compile");
    env.add_template(
        "tool_policy_lines",
        include_str!("templates/tool_policy_lines.md"),
    )
    .expect("tool_policy_lines template must compile");
    env.add_template("memory_files", include_str!("templates/memory_files.md"))
        .expect("memory_files template must compile");
    env
});

/// Render a named template with the given context.
///
/// Panics if the template or context is invalid (developer bug).
#[allow(clippy::expect_used)]
pub fn render(name: &str, ctx: minijinja::Value) -> String {
    let tmpl = ENGINE.get_template(name).expect("template must exist");
    tmpl.render(ctx).expect("template render must succeed")
}
