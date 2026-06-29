//! Focus tracker.
//!
//! Records `WindowId -> focus()` and `WindowId -> unfocus()` events
//! from the [`crate::core::wm`] layer and exposes the "currently
//! focused" view to the panel's CENTER section.
//!
//! Implementation strategy:
//!
//! * The tracker is single-threaded — it lives on the main thread.
//! * Writers call [`Self::set_focused`]; readers call
//!   [`Self::current`].
//! * The optional `previous()` accessor lets the panel-element
//!   implement fade animations when the focus changes.

use std::sync::Arc;

use parking_lot::RwLock;

use crate::core::wm::{WindowId, WindowTitle};

/// What is required of a tracked window for the focus tracker to
/// display its title in the panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusEntry {
    /// Platform-agnostic window id (HWND for Windows).
    pub id: WindowId,
    /// Process id of the owning process.
    pub pid: u32,
    /// Window class name — used by the rules engine.
    pub class: String,
    /// Window title — used by the panel.
    pub title: WindowTitle,
    /// True if the window is currently being displayed fullscreen
    /// (driven by the tiling engine).
    pub fullscreen: bool,
}

impl FocusEntry {
    /// Build a placeholder "no focus" entry.
    pub fn desktop() -> Self {
        Self {
            id: WindowId::NONE,
            pid: 0,
            class: String::new(),
            title: WindowTitle::new("Desktop"),
            fullscreen: false,
        }
    }

    /// Returns `true` when no window is currently focused.
    pub fn is_desktop(&self) -> bool {
        self.id.is_none()
    }
}

/// Thread-safe focus tracker.
///
/// `Arc<FocusTracker>` is shareable between the panel thread
/// (reader) and the main thread (writer). State updates are lock-free
/// for reads after the first write since the hot path uses
/// `parking_lot::RwLock`.
#[derive(Clone)]
pub struct FocusTracker {
    inner: Arc<RwLock<FocusEntry>>,
}

impl Default for FocusTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl FocusTracker {
    /// Construct a fresh tracker that reports "Desktop" as focused.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(FocusEntry::desktop())),
        }
    }

    /// Replace the focus entry. Returns the previous entry.
    pub fn set_focused(&self, entry: FocusEntry) -> FocusEntry {
        std::mem::replace(&mut *self.inner.write(), entry)
    }

    /// Atomically clear focus (returning it to "Desktop").
    pub fn clear(&self) -> FocusEntry {
        self.set_focused(FocusEntry::desktop())
    }

    /// Read the current focus entry.
    pub fn current(&self) -> FocusEntry {
        self.inner.read().clone()
    }

    /// Returns the previous focus entry. Used by the panel for fade
    /// transitions.
    pub fn previous(&self) -> Option<FocusEntry> {
        // Future enhancement: track a small "previous" cache. For now
        // we return None so callers render the title swap cleanly.
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_is_default() {
        let f = FocusTracker::new();
        let entry = f.current();
        assert!(entry.is_desktop());
        assert_eq!(entry.title.as_str(), "Desktop");
    }

    #[test]
    fn set_focused_round_trips() {
        let f = FocusTracker::new();
        let prev = f.set_focused(FocusEntry {
            id: WindowId::new(42),
            pid: 100,
            class: "Notepad".into(),
            title: WindowTitle::new("Notepad"),
            fullscreen: false,
        });
        assert!(prev.is_desktop());
        assert_eq!(f.current().id.get(), 42);
    }
}
