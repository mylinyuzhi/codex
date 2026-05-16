//! Surface terminal substrate for the native-scrollback migration.
// S1 substrate lands before production `Tui` switches over; keep the
// unused-surface warnings scoped to this migration module.
#![allow(dead_code)]

use ratatui::backend::Backend;
use ratatui::backend::ClearType;
use ratatui::buffer::Buffer;
use ratatui::buffer::Cell;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::layout::Size;
use ratatui::text::Line;
use ratatui::widgets::Widget;

use crate::cursor::CursorClaim;
use crate::surface::history_insert::render_history_lines;

/// A retained bottom viewport with explicit cursor and history accounting.
///
/// This is intentionally smaller than the final native terminal contract. It
/// proves the central ownership model on crates.io `ratatui 0.30`: coco owns
/// viewport geometry, draw buffers, cursor policy application, and visible
/// history row accounting instead of relying on stock `Viewport::Inline`.
#[derive(Debug)]
pub(crate) struct SurfaceTerminal<B: Backend> {
    backend: B,
    buffers: [Buffer; 2],
    current: usize,
    viewport_area: Rect,
    last_known_screen_size: Size,
    visible_history_rows: u16,
    history_bottom_y: u16,
    invalidated: bool,
}

/// Frame passed to surface renderers.
pub(crate) struct SurfaceFrame<'a> {
    viewport_area: Rect,
    buffer: &'a mut Buffer,
    cursor_claim: Option<CursorClaim>,
}

impl<'a> SurfaceFrame<'a> {
    /// Area of the retained viewport.
    pub(crate) fn area(&self) -> Rect {
        self.viewport_area
    }

    /// Render a ratatui widget into the frame buffer.
    pub(crate) fn render_widget<W: Widget>(&mut self, widget: W, area: Rect) {
        widget.render(area, self.buffer);
    }

    /// Claim the terminal cursor for this frame.
    pub(crate) fn set_cursor_claim(&mut self, claim: CursorClaim) {
        self.cursor_claim = Some(claim);
    }
}

impl<B> SurfaceTerminal<B>
where
    B: Backend,
{
    /// Create a surface terminal using the backend's current size as the
    /// initial viewport.
    pub(crate) fn new(backend: B) -> Result<Self, B::Error> {
        let screen_size = backend.size()?;
        let viewport_area = Rect::new(0, 0, screen_size.width, screen_size.height);
        Ok(Self {
            backend,
            buffers: [Buffer::empty(viewport_area), Buffer::empty(viewport_area)],
            current: 0,
            viewport_area,
            last_known_screen_size: screen_size,
            visible_history_rows: 0,
            history_bottom_y: viewport_area.top(),
            invalidated: true,
        })
    }

    /// Immutable backend access for tests and future surface adapters.
    pub(crate) fn backend(&self) -> &B {
        &self.backend
    }

    /// Mutable backend access for integration glue.
    pub(crate) fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    /// Current retained viewport area.
    pub(crate) fn viewport_area(&self) -> Rect {
        self.viewport_area
    }

    /// Last backend screen size observed by the surface terminal.
    pub(crate) fn last_known_screen_size(&self) -> Size {
        self.last_known_screen_size
    }

    /// Current backend-reported terminal size.
    pub(crate) fn size(&self) -> Result<Size, B::Error> {
        self.backend.size()
    }

    /// Rows of finalized history known to be visible above the viewport.
    pub(crate) fn visible_history_rows(&self) -> u16 {
        self.visible_history_rows
    }

    /// Set the retained viewport area and resize both diff buffers.
    pub(crate) fn set_viewport_area(&mut self, area: Rect) {
        let had_history = self.visible_history_rows > 0;
        self.viewport_area = area;
        self.buffers[0].resize(area);
        self.buffers[1].resize(area);
        self.visible_history_rows = self.visible_history_rows.min(area.top());
        self.history_bottom_y = if had_history {
            self.history_bottom_y
                .min(area.top())
                .max(self.visible_history_rows)
        } else {
            area.top()
        };
        self.invalidate_viewport();
    }

    /// Move the retained viewport and clear stale cells from the old
    /// interactive region. When the inline viewport grows upward, callers can
    /// ask the terminal to scroll finalized history up before clearing the old
    /// viewport, matching the native-scrollback draw path.
    pub(crate) fn apply_viewport_area(
        &mut self,
        area: Rect,
        scroll_history_on_growth: bool,
    ) -> Result<(), B::Error> {
        let previous = self.viewport_area;
        if previous == area {
            return Ok(());
        }

        let size = self.size()?;
        let initial_fullscreen = previous.x == 0
            && previous.y == 0
            && previous.width == size.width
            && previous.height == size.height;

        if scroll_history_on_growth && !initial_fullscreen && area.y < previous.y {
            let scroll_by = previous.y - area.y;
            if scroll_by > 0 && previous.y > 0 {
                self.backend.scroll_region_up(0..previous.y, scroll_by)?;
                self.history_bottom_y = self.history_bottom_y.saturating_sub(scroll_by);
            }
            self.clear_after_position(Position {
                x: 0,
                y: previous.y,
            })?;
        } else {
            let clear_y = if initial_fullscreen {
                area.y
            } else {
                previous.y.min(area.y)
            };
            self.clear_after_position(Position { x: 0, y: clear_y })?;
        }

        self.set_viewport_area(area);
        Ok(())
    }

    /// Mark the next draw as a full repaint.
    pub(crate) fn invalidate_viewport(&mut self) {
        self.invalidated = true;
        self.previous_buffer_mut().reset();
    }

    /// Record history rows inserted above the retained viewport.
    pub(crate) fn note_history_rows_inserted(&mut self, rows: u16) {
        self.visible_history_rows = self
            .visible_history_rows
            .saturating_add(rows)
            .min(self.history_bottom_y);
    }

    /// Clear visible terminal content owned by the surface and reset history
    /// accounting. Full scrollback purge is added by the native history
    /// insertion slice; this method already gives callers a single surface
    /// boundary for clear/replay decisions.
    pub(crate) fn clear_owned_scrollback(&mut self) -> Result<(), B::Error> {
        self.backend.clear_region(ClearType::All)?;
        self.visible_history_rows = 0;
        self.history_bottom_y = self.viewport_area.top();
        self.invalidate_viewport();
        Ok(())
    }

    /// Clear the retained interactive viewport while preserving rows above it.
    pub(crate) fn clear_viewport_to_end(&mut self) -> Result<(), B::Error> {
        if self.viewport_area.width == 0 || self.viewport_area.height == 0 {
            return Ok(());
        }

        self.clear_after_position(Position {
            x: self.viewport_area.x,
            y: self.viewport_area.y,
        })
    }

    /// Remove transient viewport chrome and leave the shell prompt at the
    /// first row after finalized history.
    pub(crate) fn prepare_shell_prompt_after_exit(&mut self) -> Result<(), B::Error> {
        if self.viewport_area.width == 0 {
            return Ok(());
        }

        let size = self.size()?;
        if size.height == 0 {
            return Ok(());
        }

        let prompt = Position {
            x: 0,
            y: self.viewport_area.top().min(size.height.saturating_sub(1)),
        };
        self.clear_after_position(prompt)?;
        self.backend.show_cursor()?;
        self.backend.set_cursor_position(prompt)?;
        self.backend.flush()
    }

    /// Clear from `position` through the visible screen bottom.
    pub(crate) fn clear_after_position(&mut self, position: Position) -> Result<(), B::Error> {
        self.backend.set_cursor_position(position)?;
        self.backend.clear_region(ClearType::CurrentLine)?;
        self.backend.clear_region(ClearType::AfterCursor)?;
        self.invalidate_viewport();
        self.backend.flush()
    }

    /// Insert finalized history rows immediately above the retained viewport.
    ///
    /// This first substrate uses ratatui's scrolling-region backend API so
    /// `TestBackend` can verify the visible behavior. The dedicated VT100 byte
    /// insertion path can be added under this same API without changing the
    /// transcript/history callers.
    pub(crate) fn insert_history_lines<I>(&mut self, lines: I) -> Result<u16, B::Error>
    where
        I: IntoIterator<Item = Line<'static>>,
    {
        let lines = lines.into_iter().collect::<Vec<_>>();
        if lines.is_empty() || self.viewport_area.width == 0 || self.viewport_area.top() == 0 {
            return Ok(0);
        }

        let rendered = render_history_lines(lines, self.viewport_area.width);
        let rows = rendered.area.height.min(self.viewport_area.top());
        if rows == 0 {
            return Ok(0);
        }

        let history_bottom = self.history_bottom_y.min(self.viewport_area.top());
        let gap_below_history = self.viewport_area.top().saturating_sub(history_bottom);
        let overflow = rows.saturating_sub(gap_below_history);
        if overflow > 0 {
            self.backend.scroll_region_up(0..history_bottom, overflow)?;
        }

        let insertion_top = history_bottom.saturating_sub(overflow);

        let updates = rendered
            .content
            .iter()
            .enumerate()
            .filter_map(|(index, cell)| {
                let (x, y) = rendered.pos_of(index);
                let target_y = insertion_top + y;
                (target_y < self.viewport_area.top()).then_some((x, target_y, cell))
            });
        self.backend.draw(updates)?;
        self.backend.flush()?;
        self.history_bottom_y = insertion_top
            .saturating_add(rows)
            .min(self.viewport_area.top());
        self.note_history_rows_inserted(rows);
        self.invalidate_viewport();
        Ok(rows)
    }

    /// Draw one retained viewport frame and apply the frame's cursor claim.
    pub(crate) fn draw_viewport<F>(&mut self, render: F) -> Result<(), B::Error>
    where
        F: FnOnce(&mut SurfaceFrame<'_>),
    {
        self.autoresize()?;
        let viewport_area = self.viewport_area;
        let current = self.current_buffer_mut();
        current.reset();
        let mut frame = SurfaceFrame {
            viewport_area,
            buffer: current,
            cursor_claim: None,
        };
        render(&mut frame);
        let cursor_claim = frame.cursor_claim;

        let updates = self.buffer_updates();
        self.backend
            .draw(updates.iter().map(|(x, y, cell)| (*x, *y, cell)))?;
        self.apply_cursor_claim(cursor_claim)?;
        self.backend.flush()?;
        self.swap_buffers();
        self.invalidated = false;
        Ok(())
    }

    fn autoresize(&mut self) -> Result<(), B::Error> {
        let screen_size = self.backend.size()?;
        if screen_size != self.last_known_screen_size {
            self.last_known_screen_size = screen_size;
            self.invalidate_viewport();
        }
        Ok(())
    }

    fn apply_cursor_claim(&mut self, claim: Option<CursorClaim>) -> Result<(), B::Error> {
        if let Some(claim) = claim {
            self.backend.show_cursor()?;
            self.backend.set_cursor_position(claim.position)?;
        } else {
            self.backend.hide_cursor()?;
            self.backend.set_cursor_position(Position { x: 0, y: 0 })?;
        }
        Ok(())
    }

    fn buffer_updates(&self) -> Vec<(u16, u16, Cell)> {
        let current = self.current_buffer();
        let previous = self.previous_buffer();
        current
            .content
            .iter()
            .zip(previous.content.iter())
            .enumerate()
            .filter_map(|(index, (next, prev))| {
                if self.invalidated || next != prev {
                    let (x, y) = current.pos_of(index);
                    Some((x, y, next.clone()))
                } else {
                    None
                }
            })
            .collect()
    }

    fn current_buffer(&self) -> &Buffer {
        &self.buffers[self.current]
    }

    fn current_buffer_mut(&mut self) -> &mut Buffer {
        &mut self.buffers[self.current]
    }

    fn previous_buffer(&self) -> &Buffer {
        &self.buffers[1 - self.current]
    }

    fn previous_buffer_mut(&mut self) -> &mut Buffer {
        &mut self.buffers[1 - self.current]
    }

    fn swap_buffers(&mut self) {
        self.current = 1 - self.current;
        self.current_buffer_mut().reset();
    }
}

#[cfg(test)]
#[path = "terminal.test.rs"]
mod tests;
