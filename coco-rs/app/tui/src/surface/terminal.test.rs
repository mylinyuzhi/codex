use crossterm::cursor::SetCursorStyle;
use pretty_assertions::assert_eq;
use ratatui::backend::TestBackend;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use super::*;

#[test]
fn surface_terminal_draws_inside_configured_viewport() {
    let backend = TestBackend::new(12, 5);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    assert_eq!(terminal.last_known_screen_size(), Size::new(12, 5));
    terminal.set_viewport_area(Rect::new(0, 3, 12, 2));
    assert_eq!(terminal.viewport_area(), Rect::new(0, 3, 12, 2));

    terminal
        .draw_viewport(|frame| {
            frame.render_widget(Paragraph::new("hello"), frame.area());
        })
        .expect("draw");

    terminal.backend().assert_buffer_lines([
        "            ",
        "            ",
        "            ",
        "hello       ",
        "            ",
    ]);
}

#[test]
fn surface_terminal_applies_cursor_claim() {
    let backend = TestBackend::new(8, 4);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 2, 8, 2));

    terminal
        .draw_viewport(|frame| {
            frame.set_cursor_claim(CursorClaim {
                position: Position { x: 3, y: 3 },
                style: SetCursorStyle::SteadyBar,
            });
        })
        .expect("draw");

    terminal
        .backend_mut()
        .assert_cursor_position(Position { x: 3, y: 3 });
}

#[test]
fn surface_terminal_hides_cursor_without_claim_and_homes_position() {
    let backend = TestBackend::new(8, 4);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");

    terminal.draw_viewport(|_frame| {}).expect("draw");

    terminal
        .backend_mut()
        .assert_cursor_position(Position { x: 0, y: 0 });
}

#[test]
fn visible_history_rows_are_clamped_to_rows_above_viewport() {
    let backend = TestBackend::new(10, 6);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 4, 10, 2));

    terminal.note_history_rows_inserted(3);
    assert_eq!(terminal.visible_history_rows(), 3);

    terminal.note_history_rows_inserted(10);
    assert_eq!(terminal.visible_history_rows(), 4);
}

#[test]
fn set_viewport_area_reclamps_visible_history_rows() {
    let backend = TestBackend::new(10, 8);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 5, 10, 3));
    terminal.note_history_rows_inserted(5);

    terminal.set_viewport_area(Rect::new(0, 2, 10, 3));

    assert_eq!(terminal.visible_history_rows(), 2);
}

#[test]
fn clear_owned_scrollback_resets_history_accounting_and_repaints() {
    let backend = TestBackend::new(8, 3);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 1, 8, 2));
    terminal.note_history_rows_inserted(1);
    terminal
        .draw_viewport(|frame| {
            frame.render_widget(
                Paragraph::new("stale").style(Style::default()),
                frame.area(),
            );
        })
        .expect("initial draw");

    terminal.clear_owned_scrollback().expect("clear");
    terminal
        .draw_viewport(|frame| {
            frame.render_widget(Paragraph::new("fresh"), frame.area());
        })
        .expect("redraw");

    assert_eq!(terminal.visible_history_rows(), 0);
    terminal
        .backend()
        .assert_buffer_lines(["        ", "fresh   ", "        "]);
}

#[test]
fn clear_viewport_to_end_preserves_history_above_viewport() {
    let backend = TestBackend::with_lines(["history ", "stale 1 ", "stale 2 "]);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 1, 8, 2));

    terminal.clear_viewport_to_end().expect("clear viewport");

    terminal
        .backend()
        .assert_buffer_lines(["history ", "        ", "        "]);
}

#[test]
fn prepare_shell_prompt_after_exit_clears_viewport_and_places_cursor_after_history() {
    let backend = TestBackend::with_lines(["history ", "stale 1 ", "stale 2 "]);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 1, 8, 2));

    terminal
        .prepare_shell_prompt_after_exit()
        .expect("prepare prompt");

    terminal
        .backend()
        .assert_buffer_lines(["history ", "        ", "        "]);
    terminal
        .backend_mut()
        .assert_cursor_position(Position { x: 0, y: 1 });
}

#[test]
fn apply_viewport_area_shrink_clears_old_live_tail() {
    let backend = TestBackend::with_lines(["history", "live 1 ", "live 2 ", "input ", "status"]);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 1, 7, 4));

    terminal
        .apply_viewport_area(Rect::new(0, 3, 7, 2), true)
        .expect("apply viewport");

    terminal
        .backend()
        .assert_buffer_lines(["history", "       ", "       ", "       ", "       "]);
}

#[test]
fn apply_viewport_area_growth_scrolls_history_before_clearing_viewport() {
    let backend = TestBackend::with_lines(["hist 1", "hist 2", "hist 3", "live 1", "input "]);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 3, 6, 2));

    terminal
        .apply_viewport_area(Rect::new(0, 1, 6, 4), true)
        .expect("apply viewport");

    terminal
        .backend()
        .assert_buffer_lines(["hist 3", "      ", "      ", "      ", "      "]);
}

#[test]
fn insert_history_lines_after_viewport_shrink_closes_live_tail_gap() {
    let backend = TestBackend::new(8, 10);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 8, 8, 2));
    terminal
        .insert_history_lines([
            Line::from("header"),
            Line::default(),
            Line::from("❯ hello"),
            Line::default(),
        ])
        .expect("insert first history");
    terminal
        .apply_viewport_area(Rect::new(0, 5, 8, 5), true)
        .expect("grow viewport");
    terminal
        .apply_viewport_area(Rect::new(0, 8, 8, 2), true)
        .expect("shrink viewport");

    terminal
        .insert_history_lines([Line::from("⏺ hi"), Line::default()])
        .expect("insert assistant history");

    terminal.backend().assert_buffer_lines([
        "        ",
        "header  ",
        "        ",
        "❯ hello",
        "        ",
        "⏺ hi    ",
        "        ",
        "        ",
        "        ",
        "        ",
    ]);
}

#[test]
fn insert_history_lines_writes_above_viewport_and_preserves_viewport() {
    let backend = TestBackend::with_lines(["old0  ", "old1  ", "old2  ", "view0 ", "view1 "]);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 3, 6, 2));

    let inserted = terminal
        .insert_history_lines([Line::from("hist0"), Line::from("hist1")])
        .expect("insert history");

    assert_eq!(inserted, 2);
    assert_eq!(terminal.visible_history_rows(), 2);
    terminal
        .backend()
        .assert_buffer_lines(["old2  ", "hist0 ", "hist1 ", "view0 ", "view1 "]);
    terminal
        .backend()
        .assert_scrollback_lines(["old0  ", "old1  "]);
}

#[test]
fn insert_history_lines_clamps_to_rows_above_viewport() {
    let backend = TestBackend::with_lines(["old0 ", "view "]);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 1, 5, 1));

    let inserted = terminal
        .insert_history_lines([Line::from("hist0"), Line::from("hist1")])
        .expect("insert history");

    assert_eq!(inserted, 1);
    assert_eq!(terminal.visible_history_rows(), 1);
    terminal.backend().assert_buffer_lines(["hist0", "view "]);
}
