//! Surface terminal substrate for the native-scrollback TUI.

use crossterm::cursor::SetCursorStyle;
use crossterm::queue;
use crossterm::terminal::BeginSynchronizedUpdate;
use crossterm::terminal::EndSynchronizedUpdate;
use ratatui::backend::Backend;
use ratatui::backend::ClearType;
use ratatui::backend::CrosstermBackend;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::buffer::Cell;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::layout::Size;
use ratatui::text::Line;
use ratatui::widgets::Widget;
use std::io::Write;

use crate::cursor::CursorClaim;
use crate::surface::history_insert::render_history_lines;

pub(crate) trait SurfaceBackend: Backend {
    fn clear_scrollback_and_screen(&mut self) -> Result<(), Self::Error> {
        self.clear_region(ClearType::All)
    }

    fn set_cursor_style(&mut self, _style: SetCursorStyle) -> Result<(), Self::Error> {
        Ok(())
    }

    fn begin_synchronized_update(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn end_synchronized_update(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl<W> SurfaceBackend for CrosstermBackend<W>
where
    W: Write,
{
    fn clear_scrollback_and_screen(&mut self) -> Result<(), Self::Error> {
        write!(self, "\x1b[r\x1b[0m\x1b[H\x1b[2J\x1b[3J\x1b[H")?;
        Write::flush(self)
    }

    fn set_cursor_style(&mut self, style: SetCursorStyle) -> Result<(), Self::Error> {
        queue!(self, style)?;
        Ok(())
    }

    fn begin_synchronized_update(&mut self) -> Result<(), Self::Error> {
        queue!(self, BeginSynchronizedUpdate)?;
        Ok(())
    }

    fn end_synchronized_update(&mut self) -> Result<(), Self::Error> {
        queue!(self, EndSynchronizedUpdate)?;
        Write::flush(self)
    }
}

impl SurfaceBackend for TestBackend {
    fn clear_scrollback_and_screen(&mut self) -> Result<(), Self::Error> {
        let size = self.size()?;
        *self = TestBackend::new(size.width, size.height);
        Ok(())
    }
}

/// A retained inline viewport with explicit cursor and history accounting.
///
/// This is intentionally smaller than the final native terminal contract. It
/// proves the central ownership model on crates.io `ratatui 0.30`: coco owns
/// viewport geometry, draw buffers, cursor policy application, and visible
/// history row accounting instead of relying on stock `Viewport::Inline`.
#[derive(Debug)]
pub(crate) struct SurfaceTerminal<B: SurfaceBackend> {
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
    B: SurfaceBackend,
{
    /// Create a surface terminal using the backend's current size as the
    /// initial viewport.
    pub(crate) fn new(backend: B) -> Result<Self, B::Error> {
        let screen_size = backend.size()?;
        let viewport_area = Rect::new(0, 0, screen_size.width, 0);
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
    #[cfg(any(test, feature = "testing"))]
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

    /// Row immediately after the finalized history owned by this surface.
    pub(crate) fn history_bottom_y(&self) -> u16 {
        self.history_bottom_y
    }

    /// Last backend screen size observed by the surface terminal.
    #[cfg(test)]
    pub(crate) fn last_known_screen_size(&self) -> Size {
        self.last_known_screen_size
    }

    /// Current backend-reported terminal size.
    pub(crate) fn size(&self) -> Result<Size, B::Error> {
        self.backend.size()
    }

    /// Rows of finalized history known to be visible above the viewport.
    #[cfg(test)]
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

        if scroll_history_on_growth
            && !initial_fullscreen
            && area.y < previous.y
            && area.bottom() >= size.height
        {
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

        tracing::debug!(
            target: "tui::surface",
            previous = ?previous,
            next = ?area,
            history_bottom_y = self.history_bottom_y,
            scroll_history_on_growth,
            "apply viewport area"
        );
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
        self.history_bottom_y = self
            .history_bottom_y
            .saturating_add(rows)
            .min(self.viewport_area.top());
        self.visible_history_rows = self
            .visible_history_rows
            .saturating_add(rows)
            .min(self.history_bottom_y);
    }

    /// Clear visible terminal content owned by the surface and reset history
    /// accounting.
    pub(crate) fn clear_owned_scrollback(&mut self) -> Result<(), B::Error> {
        let previous = self.viewport_area;
        self.backend.clear_scrollback_and_screen()?;
        self.visible_history_rows = 0;
        self.history_bottom_y = 0;
        self.viewport_area.y = 0;
        self.buffers[0].resize(self.viewport_area);
        self.buffers[1].resize(self.viewport_area);
        tracing::debug!(
            target: "tui::surface",
            previous = ?previous,
            next = ?self.viewport_area,
            "clear owned scrollback"
        );
        self.invalidate_viewport();
        Ok(())
    }

    /// Clear the retained interactive viewport while preserving rows above it.
    #[cfg(test)]
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
        if lines.is_empty() || self.viewport_area.width == 0 {
            return Ok(0);
        }

        let rendered = render_history_lines(lines, self.viewport_area.width);
        let rows = rendered.area.height;
        let previous = self.viewport_area;
        self.move_viewport_down_for_history(rows)?;

        let viewport_top = self.viewport_area.top();
        if rows == 0 || viewport_top == 0 {
            return Ok(0);
        }

        let mut start_row = 0;
        let gap_below_history = viewport_top.saturating_sub(self.history_bottom_y);
        if gap_below_history > 0 {
            let rows_to_draw = rows.min(gap_below_history);
            let target_top = self.history_bottom_y;
            let updates = rendered
                .content
                .iter()
                .enumerate()
                .filter_map(|(index, cell)| {
                    let (x, y) = rendered.pos_of(index);
                    (y < rows_to_draw).then_some((x, target_top + y, cell))
                });
            self.backend.draw(updates)?;
            self.history_bottom_y = self.history_bottom_y.saturating_add(rows_to_draw);
            start_row = rows_to_draw;
        }

        while start_row < rows {
            let chunk_rows = (rows - start_row).min(viewport_top);
            self.backend.scroll_region_up(0..viewport_top, chunk_rows)?;
            let target_top = viewport_top - chunk_rows;
            let updates = rendered
                .content
                .iter()
                .enumerate()
                .filter_map(|(index, cell)| {
                    let (x, y) = rendered.pos_of(index);
                    if y >= start_row && y < start_row + chunk_rows {
                        Some((x, target_top + (y - start_row), cell))
                    } else {
                        None
                    }
                });
            self.backend.draw(updates)?;
            start_row += chunk_rows;
        }
        self.backend.flush()?;
        self.history_bottom_y = viewport_top;
        self.note_history_rows_inserted(rows);
        tracing::debug!(
            target: "tui::surface",
            rows,
            previous_viewport = ?previous,
            next_viewport = ?self.viewport_area,
            history_bottom_y = self.history_bottom_y,
            visible_history_rows = self.visible_history_rows,
            "insert history lines"
        );
        self.invalidate_viewport();
        Ok(rows)
    }

    fn move_viewport_down_for_history(&mut self, rows: u16) -> Result<(), B::Error> {
        if rows == 0 {
            return Ok(());
        }
        let screen_size = self.size()?;
        let available_below = screen_size
            .height
            .saturating_sub(self.viewport_area.bottom());
        let scroll_amount = rows.min(available_below);
        if scroll_amount == 0 {
            return Ok(());
        }

        let region_top = self.viewport_area.top();
        self.backend
            .scroll_region_down(region_top..screen_size.height, scroll_amount)?;
        self.viewport_area.y = self.viewport_area.y.saturating_add(scroll_amount);
        self.buffers[0].resize(self.viewport_area);
        self.buffers[1].resize(self.viewport_area);
        self.invalidate_viewport();
        Ok(())
    }

    pub(crate) fn begin_synchronized_update(&mut self) -> Result<(), B::Error> {
        self.backend.begin_synchronized_update()
    }

    pub(crate) fn end_synchronized_update(&mut self) -> Result<(), B::Error> {
        self.backend.end_synchronized_update()
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
            self.backend.set_cursor_style(claim.style)?;
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
