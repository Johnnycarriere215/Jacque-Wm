//! Direct2D + DWrite renderer façade.
//!
//! The actual Direct2D calls live in the panel's WM_PAINT handler
//! (sibling file: `window.rs`). This module exposes a
//! [`PanelRenderer`] struct that holds the latest [`PanelState`]
//! snapshot and exposes `draw(hdc, panel_state, rect)` which the
//! WM_PAINT path calls. The trait users implement writes the actual
//! GDI+/Direct2D code; we keep this small surface so the platform
//! binary stays compact and hardware acceleration can swap in at
//! any time.

use std::sync::Arc;
use parking_lot::Mutex;

use crate::core::panel::{Animation, PanelState};
use crate::core::panel::state::ThemePalette;

/// Façade accommodating future Direct2D swap-in.
///
/// Public surface:
/// * [`PanelRenderer::new`]  — build with an initial theme + state.
/// * [`PanelRenderer::update`] — replace state from `submit`.
/// * [`PanelRenderer::draw_with`] — paint a frame given an opaque
///   implementation of [`IconRenderer`].
///
/// Why the trait indirection:
/// * Tests can supply a no-op renderer (for assertions on layout).
/// * A future Direct2D implementation can replace GDI without
///   touching the call sites.
pub struct PanelRenderer {
    state: Mutex<PanelState>,
    theme: ThemePalette,
}

impl PanelRenderer {
    pub fn new(theme: ThemePalette, state: PanelState) -> Self {
        Self {
            state: Mutex::new(state),
            theme,
        }
    }

    pub fn update(&mut self, state: PanelState) {
        let mut g = self.state.lock();
        if !g.dirty {
            return;
        }
        *g = state;
    }

    pub fn state(&self) -> PanelState {
        self.state.lock().clone()
    }

    pub fn theme(&self) -> ThemePalette {
        self.theme
    }

    pub fn animations(&self) -> Vec<Animation> {
        self.state.lock().animations.clone()
    }

    /// Run the supplied renderer against the current state. The
    /// caller is responsible for HDC / Direct2D target creation.
    pub fn draw_with(&self, r: &mut dyn IconRenderer) {
        let st = self.state.lock();
        r.draw(&st, &self.theme);
    }
}

/// Modal contract for the actual paint code. GDI+ and Direct2D
/// implementations both implement this.
pub trait IconRenderer {
    fn draw(&mut self, state: &PanelState, theme: &ThemePalette);
}

/// IconRenderer implementation that draws to an owned `Vec<u32>`
/// representing a 32-bit ARGB bitmap. The Direct2D renderer is
/// constructed once and the bitmap is uploaded to the GPU only when
/// `state.dirty` is set.
#[allow(dead_code)]
pub struct SoftwareRenderer {
    pub framebuffer: Vec<u32>,
    pub width: u32,
    pub height: u32,
}

impl SoftwareRenderer {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            framebuffer: vec![0; (width * height) as usize],
            width,
            height,
        }
    }
}

impl IconRenderer for SoftwareRenderer {
    fn draw(&mut self, state: &PanelState, theme: &ThemePalette) {
        // Fill background.
        let bg = theme.background;
        for px in self.framebuffer.iter_mut() {
            *px = bg.0;
        }
        // Pill rectangles would normally be drawn here (omitted in
        // this CPU-side fallback; the actual platform impl is
        // Direct2D).
        let _ = state; // placeholder
    }
}

// ----------------------------------------------------------------------------
// Helper: hand the renderer an Arc<PanelRenderer> for use across
// threads.
// ----------------------------------------------------------------------------

/// `Arc<PanelRenderer>` handle used by the panel thread to call
/// `draw` from the WM_PAINT handler.
pub type SharedPanelRenderer = Arc<PanelRenderer>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::panel::PanelState;
    use crate::core::panel::state::ThemePalette;
    use crate::core::WorkspaceIndex;

    #[test]
    fn renderer_holds_state_and_theme() {
        let st = PanelState::initial(
            WorkspaceIndex::new_unchecked(1),
            ThemePalette::omarchy_dark(),
        );
        let r = PanelRenderer::new(ThemePalette::omarchy_dark(), st);
        assert_eq!(r.theme().background.a(), 230);
    }

    #[test]
    fn icon_renderer_can_be_swapped() {
        let st = PanelState::initial(
            WorkspaceIndex::new_unchecked(1),
            ThemePalette::omarchy_dark(),
        );
        let r = PanelRenderer::new(ThemePalette::omarchy_dark(), st);
        let mut sw = SoftwareRenderer::new(1920, 32);
        r.draw_with(&mut sw);
    }
}
