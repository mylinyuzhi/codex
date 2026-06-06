use std::time::Instant;

use pretty_assertions::assert_eq;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::text::Line;
use uuid::Uuid;

use super::*;
use crate::state::derive::test_helpers;
use crate::surface::history_emitter::HistoryEmissionOutcome;
use crate::surface::history_lines::HistoryReplayCachePolicy;
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
fn driver_consolidates_provisional_stream_with_finalized_tail_prefix_parity() {
    let theme = Theme::default();
    let width = 32;
    let backend = TestBackend::new(width, 12);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 8, width, 4));
    let cells = vec![test_helpers::assistant_text_cell("alpha\n\nbeta")];
    let mut driver = initialized_driver(&mut terminal, &theme, width);
    let final_lines = render_finalized_history_lines(&cells, options(&theme, width));
    let provisional_lines = final_lines[..final_lines.len() - 1].to_vec();

    let provisional = driver
        .emit_provisional_stream(
            &mut terminal,
            provisional_append("alpha\n\nbeta", provisional_lines, options(&theme, width)),
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
fn driver_consolidates_multiple_provisional_appends_with_cumulative_line_parity() {
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
            provisional_append(
                "alpha\n\n",
                final_lines[..1].to_vec(),
                options(&theme, width),
            ),
        )
        .expect("first provisional append");
    driver
        .emit_provisional_stream(
            &mut terminal,
            provisional_append_after(
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
fn driver_replays_when_provisional_digest_mismatches() {
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
            provisional_append("alpha", final_lines, options(&theme, width)),
        )
        .expect("provisional append");
    driver.provisional.as_mut().expect("ledger").prefix_digest = digest_str("different");

    let outcome = driver
        .emit_append_only(&mut terminal, header(), &cells, 2, options(&theme, width))
        .expect("final append");

    assert_eq!(outcome, HistoryEmissionOutcome::ReplayRequired);
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
            provisional_append("alpha", final_lines, options(&theme, 32)),
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
            provisional_append("alpha", final_lines, options(&theme, width)),
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
fn driver_replays_when_provisional_line_style_differs_from_finalized_tail() {
    let theme = Theme::default();
    let width = 32;
    let backend = TestBackend::new(width, 12);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 8, width, 4));
    let cells = vec![test_helpers::assistant_text_cell("alpha")];
    let mut driver = initialized_driver(&mut terminal, &theme, width);
    let mut final_lines = render_finalized_history_lines(&cells, options(&theme, width));
    final_lines[0].spans[0].style = Style::default().fg(Color::Red);

    driver
        .emit_provisional_stream(
            &mut terminal,
            provisional_append("alpha", final_lines, options(&theme, width)),
        )
        .expect("provisional append");

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
            provisional_append(
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
    assert!(driver.stream_finish_replay_needed());
    assert!(!driver.stream_finish_replay_needed());
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
    assert_eq!(terminal.viewport_area(), Rect::new(0, 3, 8, 1));
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
    assert!(driver.stream_finish_replay_needed());
}

#[test]
fn driver_replay_all_reanchors_viewport_to_replayed_history_bottom() {
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
    assert_eq!(terminal.viewport_area().top(), rows);

    let lines = plain_buffer_lines(terminal.backend().buffer());
    let assistant = line_index(&lines, "⏺ short reply");
    let input_top = terminal.viewport_area().top() as usize;
    let gap = input_top.saturating_sub(assistant + 1);
    assert!(
        gap <= 3,
        "replay left {gap} rows between assistant and viewport:\n{}",
        lines.join("\n")
    );
}

fn header() -> Vec<Line<'static>> {
    vec![Line::from("header")]
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
) -> ProvisionalStableAppend {
    provisional_append_after("", source, append_lines, options)
}

fn provisional_append_after(
    prior_source: &str,
    source: &str,
    append_lines: Vec<Line<'static>>,
    options: HistoryLineRenderOptions<'_>,
) -> ProvisionalStableAppend {
    ProvisionalStableAppend {
        prior_prefix_digest: digest_str(prior_source),
        prefix_digest: digest_str(&format!("{prior_source}{source}")),
        append_source: source.to_string(),
        append_line_fingerprints: fingerprint_lines(&append_lines),
        append_lines,
        render_key: finalized_render_key(options),
    }
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

fn line_index(lines: &[String], needle: &str) -> usize {
    lines
        .iter()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("missing {needle:?} in {lines:#?}"))
}
