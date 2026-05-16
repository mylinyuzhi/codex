use pretty_assertions::assert_eq;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::text::Line;

use super::*;
use crate::display_settings::SyntaxHighlighting;
use crate::presentation::styles::UiStyles;
use crate::surface::history_lines::HistoryLineRenderOptions;
use crate::surface::history_lines::render_finalized_history_lines;
use crate::surface::terminal::SurfaceTerminal;
use crate::theme::Theme;

#[test]
fn plan_appends_all_messages_for_fresh_tracker() {
    let messages = messages(["m1", "m2"]);
    let tracker = HistoryEmissionTracker::new();

    assert_eq!(
        tracker.plan(&messages),
        HistoryEmissionPlan::Append { start: 0 }
    );
}

#[test]
fn plan_noops_when_emitted_prefix_matches_all_messages() {
    let messages = messages(["m1", "m2"]);
    let mut tracker = HistoryEmissionTracker::new();
    tracker.mark_emitted_through(&messages, messages.len());

    assert_eq!(tracker.plan(&messages), HistoryEmissionPlan::Noop);
    assert_eq!(tracker.emitted_count(), 2);
}

#[test]
fn plan_appends_only_new_tail_when_prefix_matches() {
    let initial = messages(["m1"]);
    let next = messages(["m1", "m2", "m3"]);
    let mut tracker = HistoryEmissionTracker::new();
    tracker.mark_emitted_through(&initial, initial.len());

    assert_eq!(
        tracker.plan(&next),
        HistoryEmissionPlan::Append { start: 1 }
    );
}

#[test]
fn plan_requires_replay_after_rewind_or_truncate() {
    let original = messages(["m1", "m2"]);
    let mut tracker = HistoryEmissionTracker::new();
    tracker.mark_emitted_through(&original, original.len());

    assert_eq!(
        tracker.plan(&messages(["m1"])),
        HistoryEmissionPlan::ReplayRequired
    );
}

#[test]
fn plan_requires_replay_after_prefix_divergence() {
    let original = messages(["m1", "m2"]);
    let mut tracker = HistoryEmissionTracker::new();
    tracker.mark_emitted_through(&original, original.len());

    assert_eq!(
        tracker.plan(&messages(["m1", "other"])),
        HistoryEmissionPlan::ReplayRequired
    );
}

#[test]
fn reset_returns_tracker_to_fresh_append_state() {
    let messages = messages(["m1"]);
    let mut tracker = HistoryEmissionTracker::new();
    tracker.mark_emitted_through(&messages, messages.len());
    tracker.reset();

    assert_eq!(
        tracker.plan(&messages),
        HistoryEmissionPlan::Append { start: 0 }
    );
}

#[test]
fn emit_append_only_writes_new_tail_and_marks_messages() {
    let backend = TestBackend::with_lines(["old   ", "view  "]);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 1, 6, 1));
    let messages = messages(["m1"]);
    let mut tracker = HistoryEmissionTracker::new();

    let outcome = tracker
        .emit_append_only(&mut terminal, &messages, render_message_ids)
        .expect("emit");

    assert_eq!(
        outcome,
        HistoryEmissionOutcome::Appended {
            start: 0,
            message_count: 1,
            rows: 1,
        }
    );
    assert_eq!(tracker.emitted_count(), 1);
    terminal.backend().assert_buffer_lines(["m1    ", "view  "]);
}

#[test]
fn emit_append_only_noops_when_already_emitted() {
    let backend = TestBackend::with_lines(["old   ", "view  "]);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 1, 6, 1));
    let messages = messages(["m1"]);
    let mut tracker = HistoryEmissionTracker::new();
    tracker.mark_emitted_through(&messages, messages.len());

    let outcome = tracker
        .emit_append_only(&mut terminal, &messages, render_message_ids)
        .expect("emit");

    assert_eq!(outcome, HistoryEmissionOutcome::Noop);
    terminal.backend().assert_buffer_lines(["old   ", "view  "]);
}

#[test]
fn emit_append_only_returns_replay_required_without_touching_terminal() {
    let backend = TestBackend::with_lines(["old   ", "view  "]);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 1, 6, 1));
    let original = messages(["m1"]);
    let mut tracker = HistoryEmissionTracker::new();
    tracker.mark_emitted_through(&original, original.len());

    let outcome = tracker
        .emit_append_only(&mut terminal, &messages(["other"]), render_message_ids)
        .expect("emit");

    assert_eq!(outcome, HistoryEmissionOutcome::ReplayRequired);
    terminal.backend().assert_buffer_lines(["old   ", "view  "]);
}

#[test]
fn replay_all_clears_surface_reinserts_all_rows_and_marks_messages() {
    let backend = TestBackend::with_lines(["old0  ", "old1  ", "view  "]);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 2, 6, 1));
    terminal.note_history_rows_inserted(2);
    let messages = messages(["m1", "m2"]);
    let mut tracker = HistoryEmissionTracker::new();

    let outcome = tracker
        .replay_all(&mut terminal, &messages, render_message_ids)
        .expect("replay");

    assert_eq!(
        outcome,
        HistoryEmissionOutcome::Replayed {
            message_count: 2,
            rows: 2,
        }
    );
    assert_eq!(tracker.emitted_count(), 2);
    assert_eq!(terminal.visible_history_rows(), 2);
    terminal
        .backend()
        .assert_buffer_lines(["m1    ", "m2    ", "      "]);
}

#[test]
fn emit_append_only_accepts_finalized_transcript_renderer() {
    let theme = Theme::default();
    let backend = TestBackend::with_lines(["old0    ", "old1    ", "old2    ", "view    "]);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 3, 8, 1));
    let messages = vec![ChatMessage::assistant_text("a1", "hello")];
    let mut tracker = HistoryEmissionTracker::new();

    let outcome = tracker
        .emit_append_only(&mut terminal, &messages, |tail| {
            render_finalized_history_lines(
                tail,
                HistoryLineRenderOptions {
                    styles: UiStyles::new(&theme),
                    width: 8,
                    syntax_highlighting: SyntaxHighlighting::Disabled,
                    show_system_reminders: false,
                },
            )
        })
        .expect("emit");

    assert_eq!(
        outcome,
        HistoryEmissionOutcome::Appended {
            start: 0,
            message_count: 1,
            rows: 2,
        }
    );
    assert_eq!(
        plain_buffer_lines(terminal.backend().buffer()),
        vec!["old2    ", "⏺ hello ", "        ", "view    "]
    );
}

fn messages<const N: usize>(ids: [&str; N]) -> Vec<ChatMessage> {
    ids.into_iter()
        .map(|id| ChatMessage::assistant_text(id, "text"))
        .collect()
}

fn render_message_ids(messages: &[ChatMessage]) -> Vec<Line<'static>> {
    messages
        .iter()
        .map(|message| Line::from(message.id.clone()))
        .collect()
}

fn plain_buffer_lines(buffer: &ratatui::buffer::Buffer) -> Vec<String> {
    buffer
        .content
        .chunks(buffer.area.width as usize)
        .map(|cells| cells.iter().map(ratatui::buffer::Cell::symbol).collect())
        .collect()
}
