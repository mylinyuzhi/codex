//! Native-surface rendering helpers for integration tests.

use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::layout::Size;

use crate::state::AppState;
use crate::surface::compatibility::TerminalCompatibility;
use crate::surface::controller::NativeSurfaceController;
use crate::surface::overlay::OverlaySurfacePlacement;
use crate::surface::overlay::OverlaySurfaceState;
use crate::surface::terminal::SurfaceTerminal;
use crate::surface::viewport::interactive_viewport_desired_height;
use crate::terminal::NATIVE_VIEWPORT_MAX_HEIGHT;
use crate::terminal::native_viewport_area_with_max;

#[derive(Debug, Default)]
pub struct NativeSurfaceTestState {
    overlay_surface: OverlaySurfaceState,
}

/// Render `state` through the native-scrollback surface into a string.
///
/// This mirrors the production `Tui::draw` surface path closely enough for
/// integration tests while keeping raw-mode and crossterm stdin ownership out
/// of test binaries.
pub fn render_native_surface_to_string(state: &AppState, width: u16, height: u16) -> String {
    let mut surface_state = NativeSurfaceTestState::default();
    render_native_surface_to_string_with_surface_state(state, width, height, &mut surface_state)
}

/// Render with caller-owned overlay surface state so tests can exercise
/// production placement latching across multiple frames.
pub fn render_native_surface_to_string_with_surface_state(
    state: &AppState,
    width: u16,
    height: u16,
    surface_state: &mut NativeSurfaceTestState,
) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = SurfaceTerminal::new(backend).expect("test backend is infallible");
    let size = Size { width, height };
    let plan = surface_state.overlay_surface.plan_for_native_viewport(
        state,
        TerminalCompatibility::NativeScrollback,
        std::time::Instant::now(),
        width,
        NATIVE_VIEWPORT_MAX_HEIGHT,
    );
    let area = match plan.overlay_placement {
        Some(OverlaySurfacePlacement::AltScreen) => Rect::new(0, 0, width, height),
        _ => {
            let desired_height =
                interactive_viewport_desired_height(state, width, NATIVE_VIEWPORT_MAX_HEIGHT, plan);
            native_viewport_area_with_max(
                terminal.history_bottom_y(),
                size,
                desired_height,
                NATIVE_VIEWPORT_MAX_HEIGHT,
            )
        }
    };
    terminal.set_viewport_area(area);

    let mut controller = NativeSurfaceController::new();
    controller
        .draw_with_plan(&mut terminal, state, plan)
        .expect("test backend is infallible");

    buffer_to_string(terminal.backend().buffer())
}

fn buffer_to_string(buf: &Buffer) -> String {
    let mut out = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            out.push_str(buf[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}
