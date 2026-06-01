use super::*;
use coco_tui_ui::style::UiStyles;
use coco_tui_ui::theme::Theme;
use serde_json::json;

/// Flatten rendered lines to a single string for content assertions.
fn text_of(lines: &[Line<'static>]) -> String {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Render with an explicit `expanded` flag (the reader surface relaxes caps).
fn render_ex(
    tool_name: &str,
    input: Option<Value>,
    output: &str,
    is_error: bool,
    expanded: bool,
) -> Vec<Line<'static>> {
    render_ex_width(
        tool_name, input, output, is_error, expanded, /*width*/ 96,
    )
}

fn render_ex_width(
    tool_name: &str,
    input: Option<Value>,
    output: &str,
    is_error: bool,
    expanded: bool,
    width: u16,
) -> Vec<Line<'static>> {
    let theme = Theme::default();
    let cx = ToolResultRenderCtx {
        styles: UiStyles::new(&theme),
        width,
        syntax_highlighting: SyntaxHighlighting::Enabled,
        expand_hint: "(ctrl+o to expand)".to_string(),
        expanded,
    };
    let mut lines = Vec::new();
    render_tool_result_body(&cx, tool_name, input.as_ref(), output, is_error, &mut lines);
    lines
}

/// Inline-surface render (caps tight, hint points at the reader).
fn render(
    tool_name: &str,
    input: Option<Value>,
    output: &str,
    is_error: bool,
) -> Vec<Line<'static>> {
    render_ex(tool_name, input, output, is_error, /*expanded*/ false)
}

#[test]
fn unified_diff_text_emits_hunk_and_signed_lines() {
    let diff = unified_diff_text("let x = 1;\n", "let x = 2;\n");
    assert!(diff.contains("@@"), "expected a hunk header: {diff}");
    assert!(
        diff.contains("-let x = 1;"),
        "expected removed line: {diff}"
    );
    assert!(diff.contains("+let x = 2;"), "expected added line: {diff}");
}

#[test]
fn edit_renders_a_diff_from_old_new_input() {
    let input =
        json!({"file_path": "a.rs", "old_string": "let x = 1;", "new_string": "let x = 2;"});
    let out = text_of(&render("Edit", Some(input), "applied", false));
    assert!(out.contains("let x = 1;"), "old content must appear: {out}");
    assert!(out.contains("let x = 2;"), "new content must appear: {out}");
}

#[test]
fn edit_without_input_falls_back_to_output() {
    // Standalone path (input: None) must not panic and shows the tool output.
    let out = text_of(&render("Edit", None, "edited 1 file", false));
    assert!(out.contains("edited 1 file"), "{out}");
}

#[test]
fn bash_renders_output_without_echoing_the_command() {
    // The `🔧`/`●` header already names the command; the body shows only output,
    // never a redundant `$ command` line.
    let input = json!({"command": "ls -la /tmp"});
    let out = text_of(&render("Bash", Some(input), "file_a\nfile_b", false));
    assert!(out.contains("file_a"), "output expected: {out}");
    assert!(
        !out.contains("$ ls -la"),
        "command must not be echoed in the body: {out}"
    );
}

#[test]
fn todowrite_renders_status_glyphs() {
    let input = json!({"todos": [
        {"content": "done item", "status": "completed"},
        {"content": "active item", "status": "in_progress"},
        {"content": "todo item", "status": "pending"},
    ]});
    let out = text_of(&render("TodoWrite", Some(input), "ok", false));
    assert!(out.contains("✔ done item"), "{out}");
    assert!(out.contains("◐ active item"), "{out}");
    assert!(out.contains("☐ todo item"), "{out}");
}

#[test]
fn read_highlights_file_content() {
    // Routed through the markdown code highlighter — the content survives.
    let input = json!({"file_path": "main.rs"});
    let out = text_of(&render(
        "Read",
        Some(input),
        "fn main() { println!(\"hi\"); }",
        false,
    ));
    assert!(out.contains("fn main()"), "file content must render: {out}");
}

#[test]
fn read_code_truncates_by_wrapped_screen_rows() {
    let input = json!({"file_path": "main.rs"});
    let long = "let value = \"".to_string() + &"very_long_segment_".repeat(30) + "\";";
    let output = format!("{long}\n{long}");
    let out = text_of(&render_ex_width(
        "Read",
        Some(input),
        &output,
        false,
        /*expanded*/ false,
        /*width*/ 24,
    ));

    assert!(
        out.contains("… +"),
        "wrapped long code should collapse to an ellipsis: {out}"
    );
}

#[test]
fn structured_default_pretty_prints_json() {
    // A tool with no bespoke renderer + JSON output → multi-line pretty print.
    let out = text_of(&render(
        "Config",
        None,
        r#"{"theme":"dark","width":80}"#,
        false,
    ));
    assert!(
        out.contains("\"theme\": \"dark\""),
        "expected pretty JSON: {out}"
    );
    assert!(
        out.lines().count() > 1,
        "pretty JSON must be multi-line: {out}"
    );
}

#[test]
fn mcp_tool_name_routes_to_structured_default_without_panicking() {
    // `mcp__server__tool` does not parse to a ToolName → structured default.
    let out = text_of(&render("mcp__slack__send", None, r#"{"ok":true}"#, false));
    assert!(
        out.contains("\"ok\": true"),
        "MCP JSON pretty-printed: {out}"
    );
}

#[test]
fn error_output_renders_regardless_of_tool() {
    let out = text_of(&render(
        "Bash",
        Some(json!({"command": "boom"})),
        "command failed: boom",
        true,
    ));
    assert!(out.contains("command failed: boom"), "{out}");
}

#[test]
fn apply_patch_colors_signed_lines_from_input() {
    // The field is `patch` (see `ApplyPatchInput`), and the registered/wire name
    // is the serde-renamed `apply_patch`, not the PascalCase variant.
    let input = json!({"patch": "*** Update File: a.rs\n-old line\n+new line\n"});
    let out = text_of(&render("apply_patch", Some(input), "applied", false));
    assert!(out.contains("old line"), "{out}");
    assert!(out.contains("new line"), "{out}");
}

#[test]
fn expanded_surface_shows_more_rows_than_inline() {
    // The headline contract: a body the inline surface truncates ("… ctrl+o to
    // expand") resolves to its fuller form on the reader surface (`expanded`).
    let big = (0..200)
        .map(|i| format!("match line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let inline = render_ex("Grep", None, &big, false, /*expanded*/ false);
    let reader = render_ex("Grep", None, &big, false, /*expanded*/ true);
    assert!(
        reader.len() > inline.len(),
        "reader must show more rows than inline: inline={} reader={}",
        inline.len(),
        reader.len()
    );
}

#[test]
fn plain_output_truncates_by_wrapped_screen_rows() {
    let long = "https://example.test/".to_string() + &"very-long-segment/".repeat(30);
    let output = format!("{long}\n{long}");
    let out = text_of(&render_ex_width(
        "Bash",
        Some(json!({"command": "echo long"})),
        &output,
        false,
        /*expanded*/ false,
        /*width*/ 24,
    ));

    assert!(
        out.contains("… +"),
        "wrapped long output should collapse to an ellipsis: {out}"
    );
}
