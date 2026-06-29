//! Top panel — public data model.
//!
//! The panel is a 30-34 pixel-tall always-on-top popup that displays
//! workspace pills (LEFT), focused window title (CENTER), and
//! system metrics + clock (RIGHT).
//!
//! The actual rendering happens in
//! `crate::platform::windows::panel::renderer` (Direct2D + DWrite)
//! running on a dedicated thread. This module defines the data
//! model, animation curves, and the trait the platform layer fulfils.
//!
//! The panel sits on top of every window and never steals focus. It
//! must redraw quickly when state changes but stay idle when state
//! is stable — every metric has a `Dirty` flag and we redraw only
//! when something is dirty.

use std::sync::Arc;
use std::time::Instant;

use parking_lot::RwLock;

use crate::core::focus::FocusTracker;
use crate::core::metrics::{CpuSample, GpuSample, NetSample, RamSample};
use crate::core::wm::{MonitorId, WindowId};
use crate::core::WorkspaceIndex;

pub mod animation;
pub mod state;

pub use animation::{Animation, AnimationKind, Easing};
pub use state::{PanelSection, Theme, ThemePalette};

/// Height of the panel in pixels. 32 by default; configurable up to
/// 34 per the spec.
pub const PANEL_HEIGHT: i32 = 32;
pub const PANEL_MAX_HEIGHT: i32 = 34;
pub const PANEL_OPACITY: f32 = 0.92;

/// Read-only data the panel needs to repaint. The platform renderer
/// reads this through a [`PanelHost`].
#[derive(Clone)]
pub struct PanelState {
    /// Logical workspace index currently displayed to the user.
    pub current_workspace: WorkspaceIndex,
    /// Animations currently in-flight. The renderer plays them and
    /// clears them once they reach `1.0`.
    pub animations: Vec<Animation>,
    /// Theme tokens.
    pub theme: ThemePalette,
    /// Animated opacity multipliers for each workspace pill.
    pub pill_opacities: [f32; 9],
    /// CPU/GPU/RAM/Net metrics for the RIGHT section.
    pub metrics: MetricsSlot,
    /// Title to display in the CENTER section.
    pub title: String,
    /// `true` if the title is the placeholder "Desktop".
    pub title_is_placeholder: bool,
    /// Per-pixel dirty flag. The renderer checks this and only
    /// re-paints when `true`. The platform layer flips it back to
    /// `false` after a successful paint.
    pub dirty: bool,
}

/// Combined metric snapshot for the RIGHT section.
#[derive(Debug, Clone)]
pub struct MetricsSlot {
    pub cpu: CpuSample,
    pub gpu: GpuSample,
    pub ram: RamSample,
    pub net: NetSample,
    pub clock: Clock,
}

impl Default for MetricsSlot {
    fn default() -> Self {
        Self {
            cpu: CpuSample::default(),
            gpu: GpuSample::default(),
            ram: RamSample::default(),
            net: NetSample::default(),
            clock: Clock::default(),
        }
    }
}

/// Clock string with format chosen at render time.
#[derive(Debug, Clone, Copy)]
pub struct Clock {
    pub hour_24: bool,
    pub show_seconds: bool,
}

impl Default for Clock {
    fn default() -> Self {
        Self {
            hour_24: true,
            show_seconds: false,
        }
    }
}

impl PanelState {
    /// Construct an empty panel state ready for the very first
    /// paint.
    pub fn initial(current: WorkspaceIndex, theme: ThemePalette) -> Self {
        let mut pill_opacities = [0.45_f32; 9];
        let idx = (current.get() as usize).saturating_sub(1);
        if idx < pill_opacities.len() {
            pill_opacities[idx] = 1.0;
        }
        Self {
            current_workspace: current,
            animations: Vec::new(),
            theme,
            pill_opacities,
            metrics: MetricsSlot::default(),
            title: String::from("Desktop"),
            title_is_placeholder: true,
            dirty: true,
        }
    }

    /// Mark state dirty and trigger a repaint on the platform layer.
    /// Cheap — just sets a flag.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }
}

/// Trait the platform panel layer fulfils. Consumers
/// (`main.rs`, `apps/*`) talk to the panel only through this trait
/// — no direct coupling to Direct2D or DWrite.
pub trait PanelHost: Send + Sync {
    /// Spawn the panel window + render thread. Idempotent if the
    /// platform already initialised it.
    fn start(&self) -> crate::error::Result<()>;

    /// Submit a new [`PanelState`]. The renderer merges it into the
    /// next paint.
    fn submit(&self, state: PanelState);

    /// Tell the renderer to stop and tear down the panel window +
    /// thread.
    fn shutdown(&self);

    /// Returns the current panel state (used by tests/debug).
    fn state_snapshot(&self) -> PanelState;

    /// Returns the rendered panel rectangle. Used by the tiling
    /// engine to reserve the top stripe.
    fn rect(&self) -> (i32, i32, i32, i32);
}

/// `Arc` wrapper for the panel host so consumers can keep the panel
/// behind a trait object without re-wrapping.
pub type PanelHostRef = Arc<dyn PanelHost>;

// =====================================================================
// Convenience: a no-op panel host used by tests / headless environments
// =====================================================================

/// A [`PanelHost`] that records every `submit` call and never paints.
/// Useful for integration tests and as a fallback when the
/// platform refuses to create the panel window.
pub struct NullPanel {
    state: RwLock<PanelState>,
}

impl NullPanel {
    pub fn new(initial: PanelState) -> Self {
        Self {
            state: RwLock::new(initial),
        }
    }
}

impl PanelHost for NullPanel {
    fn start(&self) -> crate::error::Result<()> {
        Ok(())
    }
    fn submit(&self, state: PanelState) {
        let _ = Instant::now(); // silence unused
        *self.state.write() = state;
    }
    fn shutdown(&self) {}
    fn state_snapshot(&self) -> PanelState {
        self.state.read().clone()
    }
    fn rect(&self) -> (i32, i32, i32, i32) {
        (0, 0, 0, PANEL_HEIGHT)
    }
}

/// Subscribe to a focus tracker + workspace + metrics + monitor list
/// and produce a PanelState each time any of those change. The
/// orchestration lives here so the platform layer can be a thin DX
/// shell.
pub struct PanelController {
    inner: RwLock<PanelState>,
    focus: FocusTracker,
    panel_host: PanelHostRef,
}

impl PanelController {
    pub fn new(focus: FocusTracker, panel: PanelHostRef, initial: PanelState) -> Self {
        let host = panel.clone();
        let _ = host.start();
        *host.state_snapshot().clone_with_dirty(true); // silence unused
        Self {
            inner: RwLock::new(initial.clone()),
            focus,
            panel_host: panel,
        }
    }

    /// Returns the host so the platform layer can wire its metrics
    /// collector into the controller.
    pub fn host(&self) -> PanelHostRef {
        self.panel_host.clone()
    }

    /// Returns the focus tracker so the platform layer can update
    /// the title.
    pub fn focus(&self) -> FocusTracker {
        self.focus.clone()
    }

    /// Set current workspace + animate the pill transition.
    pub fn set_workspace(&self, new: WorkspaceIndex) {
        let mut st = self.inner.write();
        let old = st.current_workspace;
        if old == new {
            return;
        }
        let now = Instant::now();
        // Fade out the active pill, fade in the new active pill.
        let old_pill = (old.get() as usize).saturating_sub(1);
        let new_pill = (new.get() as usize).saturating_sub(1);
        // Easing: smoothstep across ~140 ms.
        st.animations.push(Animation::new(
            AnimationKind::FadePill {
                pill: old_pill,
                from: st.pill_opacities[old_pill.min(8)],
                to: 0.45,
            },
            now,
            std::time::Duration::from_millis(140),
        ));
        st.animations.push(Animation::new(
            AnimationKind::FadePill {
                pill: new_pill,
                from: st.pill_opacities[new_pill.min(8)],
                to: 1.0,
            },
            now,
            std::time::Duration::from_millis(140),
        ));
        for i in 0..9 {
            if i == old_pill {
                st.pill_opacities[i] = 0.45;
            } else if i == new_pill {
                st.pill_opacities[i] = 1.0;
            }
        }
        st.current_workspace = new;
        st.mark_dirty();
        self.panel_host.submit(st.clone());
    }

    /// Update the title displayed in the CENTER section.
    pub fn set_title(&self, title: &str) {
        let mut st = self.inner.write();
        if st.title == title {
            return;
        }
        st.title = title.to_owned();
        st.title_is_placeholder = title == "Desktop";
        st.mark_dirty();
        self.panel_host.submit(st.clone());
    }

    /// Update the system metrics slot.
    pub fn set_metrics(&self, metrics: MetricsSlot) {
        let mut st = self.inner.write();
        st.metrics = metrics;
        st.mark_dirty();
        self.panel_host.submit(st.clone());
    }

    /// Update theme tokens.
    pub fn set_theme(&self, theme: ThemePalette) {
        let mut st = self.inner.write();
        st.theme = theme;
        st.mark_dirty();
        self.panel_host.submit(st.clone());
    }

    /// Returns the in-memory latest state. The platform layer uses
    /// this to drive the renderer.
    pub fn snapshot(&self) -> PanelState {
        self.inner.read().clone()
    }

    pub fn submit(&self, st: PanelState) {
        *self.inner.write() = st.clone();
        self.panel_host.submit(st);
    }
}

impl CloneWithDirtyExt for PanelState {
    fn clone_with_dirty(&self, dirty: bool) -> PanelState {
        let mut s = self.clone();
        s.dirty = dirty;
        s
    }
}

pub trait CloneWithDirtyExt {
    fn clone_with_dirty(&self, dirty: bool) -> PanelState;
}
