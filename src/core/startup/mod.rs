//! Startup orchestration.
//!
//! Coordinates the *order* in which subsystems come online.
//!
//! The boot sequence documented in the JacqueWM specification is:
//!
//! 1. Wait for Explorer initialization.
//! 2. Initialise the logger.
//! 3. Load configuration.
//! 4. Initialise the workspace engine.
//! 5. Initialise the virtual desktop adapter.
//! 6. Enumerate desktops.
//! 7. Ensure Desktop 1 exists; switch to it.
//! 8. Register all keyboard shortcuts.
//! 9. Begin event monitoring.
//! 10. Enter running state.
//!
//! This module exposes the [`Startup`] struct which records each step
//! so the rest of the codebase can refuse to do work until each phase
//! completes. The actual Win32-specific implementations (registry,
//! wait-for-explorer, etc.) live under `platform::windows::startup`.

use std::sync::Arc;
use parking_lot::RwLock;
use tracing::info;

/// Lifecycle phases the system can be in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// The process just started. Nothing is initialised yet.
    Boot,
    /// Wait-for-Explorer has completed.
    ExplorerReady,
    /// Logger is wired up.
    LoggerReady,
    /// Configuration has been loaded.
    ConfigReady,
    /// Workspace engine is constructed and synchronised.
    EngineReady,
    /// Virtual desktop adapter is wired into the engine.
    AdapterReady,
    /// Desktop enumeration completed; nine desktops exist.
    DesktopsReady,
    /// Hotkeys are registered with the platform.
    HotkeysReady,
    /// Event loop is running. Detached, cannot transition out without quit.
    Running,
    /// A graceful shutdown was requested.
    ShuttingDown,
}

/// Holds the global phase pointer. Cheap to clone.
#[derive(Clone, Default)]
pub struct Startup {
    inner: Arc<RwLock<Phase>>,
}

impl Startup {
    /// Construct a startup tracker in the [`Phase::Boot`] state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Current phase.
    pub fn phase(&self) -> Phase {
        *self.inner.read()
    }

    /// Transition to a new phase, logging the transition. Returns the
    /// new phase on success.
    pub fn advance(&self, next: Phase) -> Phase {
        let mut g = self.inner.write();
        let prev = *g;
        *g = next;
        info!(
            target: "jacquewm.startup",
            from = ?prev,
            to = ?next,
            "lifecycle phase transition"
        );
        next
    }

    /// Returns `true` only when the system is fully online.
    pub fn is_running(&self) -> bool {
        matches!(self.phase(), Phase::Running)
    }

    /// Request shutdown. The main loop polls this and exits when set.
    pub fn shutdown(&self) {
        self.advance(Phase::ShuttingDown);
    }
}

impl Default for Phase {
    fn default() -> Self {
        Phase::Boot
    }
}
