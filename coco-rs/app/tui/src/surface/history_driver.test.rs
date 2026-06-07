use std::sync::Arc;
use std::time::Instant;

use pretty_assertions::assert_eq;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use uuid::Uuid;

use super::*;
use crate::state::AppState;
use crate::state::derive::test_helpers;
use crate::state::ui::StreamingState;
use crate::surface::history_emitter::HistoryEmissionOutcome;
use crate::surface::history_lines::HistoryReplayCachePolicy;
use crate::surface::modal::HistorySurfaceMode;
use crate::surface::modal::SurfaceFramePlan;
use crate::surface::stream::CommittedStablePrefix;
use crate::surface::stream::SurfaceStreamDriver;
use crate::theme::Theme;
use coco_tui_ui::display::SyntaxHighlighting;
use coco_tui_ui::engine::history_reflow::HistoryViewportChange;
use coco_tui_ui::style::UiStyles;

#[test]
fn driver_emit_append_only_uses_finalized_transcript_renderer() {
    let theme = Theme::default();
    let backend = TestBackend::with_lines([
        "old0    ", "old1    ", "old2    ", "old3    ", "old4    ", "old5    ", "view    ",
    ]);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 6, 8, 1));
    let cells = vec![test_helpers::assistant_text_cell("hello")];
    let mut driver = SurfaceHistoryDriver::default();

    let outcome = driver
        .emit_append_only(&mut terminal, header(), &cells, 1, options(&theme, 8))
        .expect("emit");

    assert_eq!(
        outcome,
        HistoryEmissionOutcome::Appended {
            start: 0,
            message_count: 1,
            rows: 3,
        }
    );
    assert_eq!(
        plain_buffer_lines(terminal.backend().buffer()),
        vec![
            "old3    ",
            "old4    ",
            "old5    ",
            "header  ",
            "⏺ hello ",
            "        ",
            "view    "
        ]
    );
}

#[test]
fn driver_tail_cache_tracks_current_width_only() {
    let theme = Theme::default();
    let backend = TestBackend::new(16, 8);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 6, 16, 2));
    let cells = vec![test_helpers::assistant_text_cell("hello")];
    let mut driver = SurfaceHistoryDriver::default();

    let outcome = driver
        .emit_append_only(&mut terminal, header(), &cells, 1, options(&theme, 16))
        .expect("emit");

    let HistoryEmissionOutcome::Appended { rows, .. } = outcome else {
        panic!("expected append outcome: {outcome:?}");
    };
    assert_eq!(rows, driver.tail_reveal_rows(16));
    assert_eq!(driver.tail_reveal_rows(12), 0);
    driver.note_viewport(/*width*/ 12, /*stream_active*/ false);
    assert_eq!(driver.tail_reveal_rows(16), 0);
    assert_eq!(driver.tail_reveal_rows(12), 0);
}

#[test]
fn history_tail_cache_caches_wrapped_rows_and_trims_suffix() {
    let mut cache = HistoryTailCache::default();
    let initial_rows = render_history_rows(
        (0..140)
            .map(|index| Line::from(format!("line {index:03}")))
            .collect(),
        /*width*/ 16,
    );
    cache.replace_from_rows(/*width*/ 16, &initial_rows);

    assert_eq!(cache.available_rows(/*width*/ 16), 128);
    assert_eq!(cache.rows.as_ref().expect("cached rows").height(), 128);
    assert_eq!(cache.available_rows(/*width*/ 12), 0);

    let appended_rows = render_history_rows(
        (140..150)
            .map(|index| Line::from(format!("line {index:03}")))
            .collect(),
        /*width*/ 16,
    );
    cache.extend_from_rows(/*width*/ 16, &appended_rows);

    assert_eq!(cache.available_rows(/*width*/ 16), 128);
    assert_eq!(cache.rows.as_ref().expect("cached rows").height(), 128);
}

#[test]
fn driver_consolidates_provisional_stream_with_finalized_tail_by_line_count() {
    let theme = Theme::default();
    let width = 32;
    let backend = TestBackend::new(width, 12);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 8, width, 4));
    let cells = vec![test_helpers::assistant_text_cell("alpha\n\nbeta")];
    let mut driver = initialized_driver(&mut terminal, &theme, width);
    let final_lines = render_finalized_history_lines(&cells, options(&theme, width));
    let provisional_lines = final_lines[..1].to_vec();

    let provisional = driver
        .emit_provisional_stream(
            &mut terminal,
            &provisional_append("alpha\n\n", provisional_lines, options(&theme, width)),
        )
        .expect("provisional append");
    assert!(matches!(
        provisional,
        ProvisionalAppendOutcome::Written { .. }
    ));

    let outcome = driver
        .emit_append_only(&mut terminal, header(), &cells, 2, options(&theme, width))
        .expect("final append");

    assert!(matches!(outcome, HistoryEmissionOutcome::Appended { .. }));
    let text = plain_buffer_lines(terminal.backend().buffer()).join("\n");
    assert_eq!(text.matches("alpha").count(), 1, "{text}");
    assert_eq!(text.matches("beta").count(), 1, "{text}");
}

#[test]
fn driver_consolidates_multiple_provisional_appends_with_cumulative_line_count() {
    let theme = Theme::default();
    let width = 32;
    let backend = TestBackend::new(width, 12);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 8, width, 4));
    let cells = vec![test_helpers::assistant_text_cell("alpha\n\nbeta")];
    let mut driver = initialized_driver(&mut terminal, &theme, width);
    let final_lines = render_finalized_history_lines(&cells, options(&theme, width));

    driver
        .emit_provisional_stream(
            &mut terminal,
            &provisional_append(
                "alpha\n\n",
                final_lines[..1].to_vec(),
                options(&theme, width),
            ),
        )
        .expect("first provisional append");
    driver
        .emit_provisional_stream(
            &mut terminal,
            &provisional_append_after(
                "alpha\n\n",
                "beta",
                final_lines[1..].to_vec(),
                options(&theme, width),
            ),
        )
        .expect("second provisional append");

    let outcome = driver
        .emit_append_only(&mut terminal, header(), &cells, 2, options(&theme, width))
        .expect("final append");

    assert_eq!(
        outcome,
        HistoryEmissionOutcome::Appended {
            start: 0,
            message_count: 1,
            rows: 0,
        }
    );
}

#[test]
fn driver_finalizes_real_stream_driver_appends_without_replay_or_duplication() {
    let theme = Theme::default();
    let width = 32;
    let backend = TestBackend::new(width, 14);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 10, width, 4));
    let mut history = initialized_driver(&mut terminal, &theme, width);
    let mut stream = SurfaceStreamDriver::default();
    let mut state = AppState::new();
    state.ui.theme = theme.clone();
    state.ui.display_settings.syntax_highlighting = SyntaxHighlighting::Disabled;
    let mut streaming = StreamingState::new();
    streaming.append_text("alpha\n\n");
    streaming.reveal_all();
    state.ui.streaming = Some(streaming);

    let first = stream.prepare(&state, width, native_plan());
    let first_append = first.stable_append.expect("first append");
    assert!(matches!(
        history
            .emit_provisional_stream(&mut terminal, &first_append)
            .expect("first provisional"),
        ProvisionalAppendOutcome::Written { .. }
    ));
    stream.mark_stable_appended();

    let streaming = state.ui.streaming.as_mut().expect("streaming");
    streaming.append_text("beta\n\n");
    streaming.reveal_all();
    let second = stream.prepare(&state, width, native_plan());
    let second_append = second.stable_append.expect("second append");
    assert_eq!(plain_rows(&second_append.rows), vec!["", "  beta"]);
    assert!(matches!(
        history
            .emit_provisional_stream(&mut terminal, &second_append)
            .expect("second provisional"),
        ProvisionalAppendOutcome::Written { .. }
    ));
    stream.mark_stable_appended();

    let cells = vec![test_helpers::assistant_text_cell("alpha\n\nbeta\n\n")];
    let outcome = history
        .emit_append_only(&mut terminal, header(), &cells, 2, options(&theme, width))
        .expect("final append");

    assert_eq!(
        outcome,
        HistoryEmissionOutcome::Appended {
            start: 0,
            message_count: 1,
            rows: 1,
        }
    );
    let text = plain_buffer_lines(terminal.backend().buffer()).join("\n");
    assert_eq!(text.matches("alpha").count(), 1, "{text}");
    assert_eq!(text.matches("beta").count(), 1, "{text}");
}

#[test]
fn driver_consolidates_reasoning_text_message_without_replay() {
    let theme = Theme::default();
    let width = 32;
    let backend = TestBackend::new(width, 16);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 12, width, 4));
    let mut history = initialized_driver(&mut terminal, &theme, width);
    let final_cells = assistant_reasoning_text_cells("Need to inspect files.", "alpha\n\nbeta");
    let provisional_cells = vec![test_helpers::assistant_text_cell("alpha\n\n")];
    let provisional_lines =
        render_finalized_history_lines(&provisional_cells, options(&theme, width));

    history
        .emit_provisional_stream(
            &mut terminal,
            &provisional_append("alpha\n\n", provisional_lines, options(&theme, width)),
        )
        .expect("provisional append");

    let outcome = history
        .emit_append_only(
            &mut terminal,
            header(),
            &final_cells,
            2,
            options(&theme, width),
        )
        .expect("final append");

    assert!(
        matches!(outcome, HistoryEmissionOutcome::Appended { .. }),
        "reasoning+text finalize must append residual lines, got {outcome:?}"
    );
    let text = plain_buffer_lines(terminal.backend().buffer()).join("\n");
    assert_eq!(text.matches("alpha").count(), 1, "{text}");
    assert_eq!(text.matches("beta").count(), 1, "{text}");
    assert!(text.contains("Thinking"), "{text}");

    let alpha = text.find("alpha").expect("alpha rendered");
    let thinking = text.find("Thinking").expect("thinking rendered");
    assert!(
        alpha < thinking,
        "native history must remain prefix-compatible with streamed text:\n{text}"
    );
}

#[test]
fn finalized_native_history_renders_text_before_leading_thinking() {
    let theme = Theme::default();
    let width = 32;
    let cells = assistant_reasoning_text_cells("Need to inspect files.", "answer");

    let lines = render_finalized_history_lines(&cells, options(&theme, width));
    let plain = plain_lines(&lines);

    let answer = plain
        .iter()
        .position(|line| line.contains("answer"))
        .expect("answer rendered");
    let thinking = plain
        .iter()
        .position(|line| line.contains("Thinking"))
        .expect("thinking rendered");
    assert!(
        answer < thinking,
        "native finalized presentation must be text-first: {plain:#?}"
    );
}

#[test]
fn driver_replays_when_provisional_render_key_mismatches() {
    let theme = Theme::default();
    let backend = TestBackend::new(32, 12);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 8, 32, 4));
    let cells = vec![test_helpers::assistant_text_cell("alpha")];
    let mut driver = initialized_driver(&mut terminal, &theme, 32);
    let final_lines = render_finalized_history_lines(&cells, options(&theme, 32));

    driver
        .emit_provisional_stream(
            &mut terminal,
            &provisional_append("alpha", final_lines, options(&theme, 32)),
        )
        .expect("provisional append");

    let outcome = driver
        .emit_append_only(&mut terminal, header(), &cells, 2, options(&theme, 24))
        .expect("final append");

    assert_eq!(outcome, HistoryEmissionOutcome::ReplayRequired);
}

#[test]
fn driver_replays_when_provisional_source_prefix_mismatches() {
    let theme = Theme::default();
    let width = 32;
    let backend = TestBackend::new(width, 12);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 8, width, 4));
    let mut driver = initialized_driver(&mut terminal, &theme, width);
    let provisional_cells = vec![test_helpers::assistant_text_cell("alpha")];
    let final_cells = vec![test_helpers::assistant_text_cell("beta")];
    let final_lines = render_finalized_history_lines(&provisional_cells, options(&theme, width));

    driver
        .emit_provisional_stream(
            &mut terminal,
            &provisional_append("alpha", final_lines, options(&theme, width)),
        )
        .expect("provisional append");

    let outcome = driver
        .emit_append_only(
            &mut terminal,
            header(),
            &final_cells,
            2,
            options(&theme, width),
        )
        .expect("final append");

    assert_eq!(outcome, HistoryEmissionOutcome::ReplayRequired);
}

#[test]
fn driver_replays_when_provisional_line_count_exceeds_finalized_tail() {
    let theme = Theme::default();
    let width = 32;
    let backend = TestBackend::new(width, 12);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 8, width, 4));
    let cells = vec![test_helpers::assistant_text_cell("alpha")];
    let mut driver = initialized_driver(&mut terminal, &theme, width);
    let final_lines = render_finalized_history_lines(&cells, options(&theme, width));

    driver
        .emit_provisional_stream(
            &mut terminal,
            &provisional_append("alpha", final_lines, options(&theme, width)),
        )
        .expect("provisional append");
    driver.provisional.as_mut().expect("ledger").line_count = usize::MAX;

    let outcome = driver
        .emit_append_only(&mut terminal, header(), &cells, 2, options(&theme, width))
        .expect("final append");

    assert_eq!(outcome, HistoryEmissionOutcome::ReplayRequired);
}

#[test]
fn driver_uses_logical_line_count_when_provisional_line_wraps() {
    let theme = Theme::default();
    let width = 12;
    let backend = TestBackend::new(width, 16);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 12, width, 4));
    let cells = vec![test_helpers::assistant_text_cell(
        "abcdefghijklmnopqrstuvwxyz",
    )];
    let mut driver = initialized_driver(&mut terminal, &theme, width);
    let final_lines = render_finalized_history_lines(&cells, options(&theme, width));
    let logical_lines = final_lines.len();

    let provisional = driver
        .emit_provisional_stream(
            &mut terminal,
            &provisional_append(
                "abcdefghijklmnopqrstuvwxyz",
                final_lines,
                options(&theme, width),
            ),
        )
        .expect("provisional append");

    let ProvisionalAppendOutcome::Written { rows } = provisional else {
        panic!("expected written provisional append, got {provisional:?}");
    };
    assert!(usize::from(rows) > logical_lines);

    let outcome = driver
        .emit_append_only(&mut terminal, header(), &cells, 2, options(&theme, width))
        .expect("final append");

    assert_eq!(
        outcome,
        HistoryEmissionOutcome::Appended {
            start: 0,
            message_count: 1,
            rows: 0,
        }
    );
}

#[test]
fn driver_note_viewport_schedules_stream_replay_after_resize() {
    let mut driver = SurfaceHistoryDriver::default();

    assert_eq!(
        driver.note_viewport(80, false),
        HistoryViewportChange {
            initialized: true,
            changed: false,
        }
    );
    assert_eq!(
        driver.note_viewport(100, true),
        HistoryViewportChange {
            initialized: false,
            changed: true,
        }
    );
    assert!(!driver.replay_due(Instant::now()));

    driver.reflow.force_due_for_test();

    assert!(driver.replay_due(Instant::now()));
    assert_eq!(driver.reflow.pending_viewport(), Some(100));
}

#[test]
fn driver_replay_all_replaces_owned_history_and_marks_stream_replay() {
    let theme = Theme::default();
    let backend = TestBackend::with_lines([
        "old0    ", "old1    ", "old2    ", "old3    ", "old4    ", "old5    ", "view    ",
    ]);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 6, 8, 1));
    terminal.note_history_rows_inserted(6);
    let cells = vec![test_helpers::assistant_text_cell("world")];
    let mut driver = SurfaceHistoryDriver::default();

    let outcome = driver
        .replay_all(&mut terminal, header(), &cells, 1, options(&theme, 8), true)
        .expect("replay");

    assert_eq!(
        outcome,
        HistoryEmissionOutcome::Replayed {
            message_count: 1,
            rows: 3,
        }
    );
    assert_eq!(terminal.visible_history_rows(), 3);
    assert_eq!(terminal.viewport_area(), Rect::new(0, 6, 8, 1));
    assert_eq!(
        plain_buffer_lines(terminal.backend().buffer()),
        vec![
            "header  ",
            "⏺ world ",
            "        ",
            "        ",
            "        ",
            "        ",
            "        "
        ]
    );
}

#[test]
fn driver_replay_all_preserves_bottom_pinned_viewport_area() {
    let theme = Theme::default();
    let backend = TestBackend::new(48, 30);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 26, 48, 4));
    terminal.note_history_rows_inserted(26);
    let cells = vec![
        test_helpers::user_text_cell(Uuid::new_v4(), "hello"),
        test_helpers::assistant_text_cell("short reply"),
    ];
    let mut driver = SurfaceHistoryDriver::default();

    let outcome = driver
        .replay_all(
            &mut terminal,
            header(),
            &cells,
            1,
            options(&theme, 48),
            false,
        )
        .expect("replay");

    let HistoryEmissionOutcome::Replayed { rows, .. } = outcome else {
        panic!("expected replay outcome, got {outcome:?}");
    };
    assert!(rows > 0);
    assert_eq!(terminal.viewport_area(), Rect::new(0, 26, 48, 4));

    let lines = plain_buffer_lines(terminal.backend().buffer());
    let assistant = line_index(&lines, "⏺ short reply");
    let input_top = terminal.viewport_area().top() as usize;
    let gap = input_top.saturating_sub(assistant + 1);
    assert!(
        gap > 3,
        "replay should not move the bottom-pinned viewport to history:\n{}",
        lines.join("\n")
    );
}

fn header() -> Vec<Line<'static>> {
    vec![Line::from("header")]
}

fn native_plan() -> SurfaceFramePlan {
    SurfaceFramePlan {
        modal_placement: None,
        history_surface: HistorySurfaceMode::NativeScrollback,
        attention_requested: false,
    }
}

fn initialized_driver(
    terminal: &mut SurfaceTerminal<TestBackend>,
    theme: &Theme,
    width: u16,
) -> SurfaceHistoryDriver {
    let mut driver = SurfaceHistoryDriver::default();
    driver
        .emit_append_only(terminal, header(), &[], 1, options(theme, width))
        .expect("emit header");
    driver
}

fn provisional_append(
    source: &str,
    append_lines: Vec<Line<'static>>,
    options: HistoryLineRenderOptions<'_>,
) -> PreparedProvisionalAppend {
    provisional_append_after("", source, append_lines, options)
}

fn provisional_append_after(
    prior_source: &str,
    source: &str,
    append_lines: Vec<Line<'static>>,
    options: HistoryLineRenderOptions<'_>,
) -> PreparedProvisionalAppend {
    let mut prefix_source = prior_source.to_string();
    prefix_source.push_str(source);
    let prefix_cells = vec![test_helpers::assistant_text_cell(&prefix_source)];
    let line_count = render_finalized_history_lines(&prefix_cells, options).len();
    PreparedProvisionalAppend {
        committed_prefix: CommittedStablePrefix {
            source: prefix_source,
            line_count,
            render_key: finalized_render_key(options),
        },
        line_count: append_lines.len(),
        rows: render_history_rows(append_lines, options.width),
        render_elapsed: std::time::Duration::default(),
    }
}

fn assistant_reasoning_text_cells(reasoning: &str, text: &str) -> Vec<RenderedCell> {
    let message = coco_messages::create_assistant_message(
        vec![
            coco_messages::AssistantContent::Reasoning(coco_messages::ReasoningContent::new(
                reasoning,
            )),
            coco_messages::AssistantContent::Text(coco_messages::TextContent::new(text)),
        ],
        "test-model",
        coco_types::TokenUsage {
            output_tokens: coco_types::OutputTokens {
                reasoning: 7,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    crate::state::derive::message_to_cells(Arc::new(message))
}

fn options(theme: &Theme, width: u16) -> HistoryLineRenderOptions<'_> {
    HistoryLineRenderOptions {
        styles: UiStyles::new(theme),
        width,
        syntax_highlighting: SyntaxHighlighting::Disabled,
        show_system_reminders: false,
        show_thinking: false,
        cwd: None,
        kb_handle: None,
        replay_cache_policy: HistoryReplayCachePolicy::default(),
        reasoning_metadata: None,
    }
}

fn plain_buffer_lines(buffer: &Buffer) -> Vec<String> {
    buffer
        .content
        .chunks(buffer.area.width as usize)
        .map(|cells| cells.iter().map(ratatui::buffer::Cell::symbol).collect())
        .collect()
}

fn plain_lines(lines: &[Line<'_>]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        })
        .collect()
}

fn plain_rows(rows: &HistoryRows) -> Vec<String> {
    let buffer = rows.buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

fn line_index(lines: &[String], needle: &str) -> usize {
    lines
        .iter()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("missing {needle:?} in {lines:#?}"))
}
