//! Minimal system-tray integration.
//!
//! Spec says: "Run silently in background. Provide exit option,
//! restart option, open logs option. Never interfere with Windows
//! tray behaviour. Never override system tray icons. Never inject
//! custom shell hooks."
//!
//! Implementation strategy:
//!
//! * The JavaScipt-free Rust core defines a [`TrayManager`] trait and
//!   [`TrayAction`] enum.
//! * The Windows platform layer uses `Shell_NotifyIconW` to add a
//!   *single* user-installed icon next to the system tray; never
//!   replaces anything.
//! * No global shortcuts; the tray handles left/right click via the
//!   `WM_TRAYICON` message handler.

use std::sync::Arc;

use parking_lot::Mutex;

/// User actions that the tray menu offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrayAction {
    /// Quit JacqueWM cleanly.
    Exit,
    /// Restart JacqueWM (Process::exit + spawn_child).
    Restart,
    /// Open the log directory in Explorer.
    OpenLogs,
    /// Toggle whether hotkeys are currently processing input.
    TogglePause,
}

/// A subscription to tray clicks. The platform layer invokes the
/// closure once per click.
pub type TraySink = Arc<dyn Fn(TrayAction) + Send + Sync + 'static>;

/// Trait-object view used everywhere outside the platform layer.
pub trait TrayManager: Send + Sync {
    /// Add the tray icon (idempotent — silent on repeat)
    fn install(&self);
    /// Remove the tray icon.
    fn remove(&self);
    /// Subscribe to user clicks. Multiple subscribers are not allowed;
    /// calling twice replaces the existing sink.
    fn subscribe(&self, sink: TraySink);
    /// Returns the install state.
    fn is_installed(&self) -> bool;
}

/// Generic platform-agnostic state holder used by every concrete
/// implementation.
#[derive(Default)]
pub struct TrayState {
    installed: bool,
    /// Combined lock for `installed` + `sink` so all three mutating
    /// methods stay consistent — both are set/read on the main
    /// thread's message pump, so contention is essentially zero.
    inner: Mutex<TrayStateInner>,
}

#[derive(Default)]
struct TrayStateInner {
    installed: bool,
    sink: Option<TraySink>,
}

impl TrayState {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn mark_installed(&self) -> bool {
        let mut g = self.inner.lock().unwrap();
        let prev = g.installed;
        g.installed = true;
        prev
    }
    pub fn mark_removed(&self) {
        self.inner.lock().unwrap().installed = false;
    }
    pub fn is_installed_value(&self) -> bool {
        self.inner.lock().unwrap().installed
    }
    pub fn set_sink(&self, sink: TraySink) {
        self.inner.lock().unwrap().sink = Some(sink);
    }
    pub fn dispatch(&self, action: TrayAction) {
        let g = self.inner.lock().unwrap();
        if let Some(sink) = g.sink.as_ref() {
            sink(action);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sink_receives_dispatch() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNT: AtomicU32 = AtomicU32::new(0);
        let mut s = TrayState::new();
        s.set_sink(Arc::new(|_| {
            COUNT.fetch_add(1, Ordering::SeqCst);
        }) as TraySink);
        s.dispatch(TrayAction::Exit);
        assert_eq!(COUNT.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn mark_install_round_trips() {
        let s = TrayState::new();
        assert!(!s.is_installed_value());
        assert!(!s.mark_installed());
        assert!(s.is_installed_value());
        s.mark_removed();
        assert!(!s.is_installed_value());
    }
}
