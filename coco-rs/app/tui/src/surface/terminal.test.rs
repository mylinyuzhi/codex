use crossterm::cursor::SetCursorStyle;
use pretty_assertions::assert_eq;
use ratatui::backend::Backend;
use ratatui::backend::CrosstermBackend;
use ratatui::backend::TestBackend;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use std::cell::RefCell;
use std::io;
use std::io::Write;
use std::rc::Rc;

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
fn surface_terminal_skips_hidden_cells_after_wide_chars() {
    let backend = TestBackend::new(20, 2);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 0, 20, 2));
    terminal
        .current_buffer_mut()
        .set_string(0, 0, "❯ 你是什么模型", Style::default());

    let updates = terminal.buffer_updates();

    assert!(updates.iter().all(|(_, _, cell)| !cell.skip));
    let symbols = updates
        .iter()
        .map(|(_, _, cell)| cell.symbol())
        .collect::<String>();
    assert!(symbols.contains("你是"), "got {symbols:?}");
    assert!(!symbols.contains("你 "), "got {symbols:?}");
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
    assert_eq!(terminal.history_bottom_y(), 0);
    assert_eq!(terminal.viewport_area(), Rect::new(0, 0, 8, 2));
    terminal
        .backend()
        .assert_buffer_lines(["fresh   ", "        ", "        "]);
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
fn apply_viewport_area_closes_gap_without_scrolling_history() {
    let backend =
        TestBackend::with_lines(["hist 1", "hist 2", "      ", "      ", "live  ", "input "]);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 4, 6, 2));
    terminal.note_history_rows_inserted(2);

    terminal
        .apply_viewport_area(Rect::new(0, 2, 6, 2), true)
        .expect("apply viewport");

    assert_eq!(terminal.history_bottom_y(), 2);
    terminal
        .backend()
        .assert_buffer_lines(["hist 1", "hist 2", "      ", "      ", "      ", "      "]);
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
fn insert_history_lines_pushes_viewport_down_when_screen_has_room() {
    let backend = TestBackend::with_lines(["view0 ", "view1 ", "      ", "      ", "      "]);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 0, 6, 2));

    let inserted = terminal
        .insert_history_lines([Line::from("hist0"), Line::from("hist1")])
        .expect("insert history");

    assert_eq!(inserted, 2);
    assert_eq!(terminal.viewport_area(), Rect::new(0, 2, 6, 2));
    assert_eq!(terminal.visible_history_rows(), 2);
    terminal
        .backend()
        .assert_buffer_lines(["hist0 ", "hist1 ", "view0 ", "view1 ", "      "]);
}

#[test]
fn insert_history_lines_scrolls_overflow_into_scrollback() {
    let backend = TestBackend::with_lines(["old0 ", "view "]);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 1, 5, 1));

    let inserted = terminal
        .insert_history_lines([Line::from("hist0"), Line::from("hist1")])
        .expect("insert history");

    assert_eq!(inserted, 2);
    assert_eq!(terminal.visible_history_rows(), 1);
    terminal
        .backend()
        .assert_scrollback_lines(["old0 ", "hist0"]);
    terminal.backend().assert_buffer_lines(["hist1", "view "]);
}

#[test]
fn crossterm_surface_backend_purges_scrollback_and_screen_bytes() {
    let capture = CapturedWriter::default();
    let mut backend = CrosstermBackend::new(capture.clone());

    backend.clear_scrollback_and_screen().expect("clear bytes");

    let bytes = capture.ansi_bytes();
    parse_with_vt100(&bytes);
    assert!(
        bytes.starts_with("\x1b[r\x1b[0m\x1b[H"),
        "expected scroll-region/style reset and cursor home in {bytes:?}"
    );
    assert!(
        bytes.contains("\x1b[3J"),
        "expected scrollback purge in {bytes:?}"
    );
    assert!(
        bytes.contains("\x1b[2J"),
        "expected screen clear in {bytes:?}"
    );
}

#[test]
fn crossterm_surface_backend_emits_scroll_region_bytes() {
    let capture = CapturedWriter::default();
    let mut backend = CrosstermBackend::new(capture.clone());

    backend.scroll_region_up(0..3, 2).expect("scroll bytes");

    let bytes = capture.ansi_bytes();
    parse_with_vt100(&bytes);
    assert!(
        bytes.contains("\x1b[1;3r"),
        "expected DECSTBM scroll region in {bytes:?}"
    );
    assert!(
        bytes.contains("\x1b[2S"),
        "expected scroll-up command in {bytes:?}"
    );
    assert!(
        bytes.contains("\x1b[r"),
        "expected scroll region reset in {bytes:?}"
    );
}

#[test]
fn crossterm_surface_backend_emits_cursor_style_and_sync_update_bytes() {
    let capture = CapturedWriter::default();
    let mut backend = CrosstermBackend::new(capture.clone());

    backend
        .begin_synchronized_update()
        .expect("begin sync bytes");
    backend
        .set_cursor_style(SetCursorStyle::SteadyBar)
        .expect("cursor style bytes");
    backend.end_synchronized_update().expect("end sync bytes");

    let bytes = capture.ansi_bytes();
    parse_with_vt100(&bytes);
    assert!(
        bytes.contains("\x1b[?2026h"),
        "expected begin synchronized update in {bytes:?}"
    );
    assert!(
        bytes.contains("\x1b[6 q"),
        "expected steady bar cursor style in {bytes:?}"
    );
    assert!(
        bytes.contains("\x1b[?2026l"),
        "expected end synchronized update in {bytes:?}"
    );
}

fn parse_with_vt100(bytes: &str) {
    let mut parser = vt100::Parser::new(8, 16, 16);
    parser.process(bytes.as_bytes());
}

#[derive(Debug, Default, Clone)]
struct CapturedWriter {
    bytes: Rc<RefCell<Vec<u8>>>,
}

impl CapturedWriter {
    fn ansi_bytes(&self) -> String {
        String::from_utf8(self.bytes.borrow().clone()).expect("crossterm bytes are utf8")
    }
}

impl Write for CapturedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.bytes.borrow_mut().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
