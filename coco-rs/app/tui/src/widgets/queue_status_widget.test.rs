use std::collections::VecDeque;

use super::MAX_ROWS;
use super::QueueStatusWidget;
use crate::state::session::QueuedCommandDisplay;

fn queued(n: usize) -> VecDeque<QueuedCommandDisplay> {
    (0..n)
        .map(|i| QueuedCommandDisplay {
            id: format!("id-{i}"),
            preview: format!("msg {i}"),
            editable: true,
        })
        .collect()
}

#[test]
fn height_is_zero_when_empty() {
    assert_eq!(QueueStatusWidget::height(&queued(0)), 0);
}

#[test]
fn height_is_one_row_per_command() {
    assert_eq!(QueueStatusWidget::height(&queued(1)), 1);
    assert_eq!(QueueStatusWidget::height(&queued(3)), 3);
}

#[test]
fn height_caps_at_max_rows() {
    assert_eq!(
        QueueStatusWidget::height(&queued(MAX_ROWS + 5)),
        MAX_ROWS as u16
    );
}
