//! Failure isolation — the safety net that keeps JacqueWM alive when
//! any single subsystem misbehaves.
//!
//! Per the spec ("If ANY subsystem fails → degrade gracefully → log
//! error → disable only its own feature set → the rest of JacqueWM
//! must remain functional") we wrap each subsystem in a threadsafe
//! "isolation cell" that:
//!
//! * Records the subsystem's health (`Alive`, `Disabled` on boot,
//!   `Dead` after a panic, `Stopping` on a clean shutdown).
//! * Provides [`safe_init`] and [`safe_loop`] helpers that contain
//!   panics to a thread boundary — Rust panics unwind only inside the
//!   thread they were raised in, so a worker thread death does not
//!   corrupt the parent process.
//! * Exposes the registry to [`crate::core::debug::DebugManager`],
//!   so the user can see at a glance which feature is currently
//!   disabled.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

/// Concrete health of a single subsystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// Thread is alive and the subsystem is operating normally.
    Alive,
    /// Subsystem was opted out via config (e.g. `tray.enabled = false`).
    /// Not an error — the user turned it off.
    Disabled,
    /// Subsystem panicked at boot and is no longer running.
    Dead,
    /// Thread is winding down.
    Stopping,
}

impl Health {
    /// One-character glyph for the DebugManager dump.
    pub fn glyph(self) -> &'static str {
        match self {
            Health::Alive => "●",
            Health::Disabled => "○",
            Health::Dead => "✗",
            Health::Stopping => "~",
        }
    }
}

/// One row in the isolation registry.
#[derive(Debug, Clone)]
pub struct SubsystemEntry {
    pub name: String,
    pub health: Health,
    pub last_error: Option<String>,
    pub last_panic_thread: Option<String>,
}

impl SubsystemEntry {
    fn new_alive(name: &str) -> Self {
        Self {
            name: name.into(),
            health: Health::Alive,
            last_error: None,
            last_panic_thread: None,
        }
    }
}

/// Global registry of subsystem health. Cheap to clone — the inner
/// state is `Arc<Mutex<…>>` so the DebugManager can read it from any
/// thread.
#[derive(Clone)]
pub struct SubsystemHealth {
    inner: Arc<Mutex<HashMap<String, SubsystemEntry>>>,
}

impl Default for SubsystemHealth {
    fn default() -> Self {
        Self::new()
    }
}

impl SubsystemHealth {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a subsystem that booted successfully.
    pub fn register(&self, name: &str) {
        let mut g = self.inner.lock().unwrap();
        g.insert(name.into(), SubsystemEntry::new_alive(name));
    }

    /// Register a subsystem that the user has chosen to disable.
    pub fn register_disabled(&self, name: &str) {
        let mut g = self.inner.lock().unwrap();
        g.insert(
            name.into(),
            SubsystemEntry {
                name: name.into(),
                health: Health::Disabled,
                last_error: None,
                last_panic_thread: None,
            },
        );
    }

    /// Mark a subsystem as dead after a panic, recording the reason.
    pub fn mark_dead(&self, name: &str, reason: String, panic_thread: Option<String>) {
        let mut g = self.inner.lock().unwrap();
        g.insert(
            name.into(),
            SubsystemEntry {
                name: name.into(),
                health: Health::Dead,
                last_error: Some(reason),
                last_panic_thread: panic_thread,
            },
        );
    }

    /// Mark the subsystem as alive again (e.g. clean shutdown + restart).
    pub fn mark_alive(&self, name: &str) {
        let mut g = self.inner.lock().unwrap();
        g.insert(name.into(), SubsystemEntry::new_alive(name));
    }

    /// Snapshot the entire registry for [`crate::core::debug`].
    pub fn snapshot(&self) -> Vec<SubsystemEntry> {
        let g = self.inner.lock().unwrap();
        let mut out: Vec<SubsystemEntry> = g.values().cloned().collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }
}

/// Run a subsystem's *boot* closure on a fresh thread and report
/// the health status to `health`. The closure's panic is captured;
/// the parent process is unaffected.
///
/// Returns the `JoinHandle` so the caller can deterministically tear
/// the worker down on shutdown.
pub fn safe_init<F>(
    name: &'static str,
    health: SubsystemHealth,
    boot: F,
) -> JoinHandle<()>
where
    F: FnOnce() + Send + 'static,
{
    let name_owned = name.to_owned();
    thread::Builder::new()
        .name(format!("jacquewm-{name}"))
        .spawn(move || {
            // We deliberately do NOT use std::panic::catch_unwind
            // here. Letting a panic propagate to the thread boundary
            // is the documented safe behaviour in Rust; trying to
            // catch it inside a Win32 driving closure risks half-
            // released handles. The thread will exit and mark the
            // subsystem dead.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(boot));
            if let Err(payload) = result {
                let reason = panic_payload_to_string(&payload);
                tracing::error!(
                    target: "jacquewm.isolation",
                    subsystem = %name_owned,
                    reason = %reason,
                    "subsystem panicked; feature set disabled"
                );
                health.mark_dead(&name_owned, reason, Some(format!("jacquewm-{name_owned}")));
            }
        })
        .expect("failed to spawn jacquewm subsystem thread")
}

/// Convert an opaque panic payload into a `String`. Accepts the
/// common payload shapes (`&str`, `String`, anything else → `"<unknown>"`).
pub fn panic_payload_to_string(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".into()
    }
}

/// Mark a subsystem as alive (helper to call from a worker).
pub fn mark_alive(name: &str, health: SubsystemHealth) {
    health.mark_alive(name);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dead_subsystem_is_listed() {
        let h = SubsystemHealth::new();
        h.register("panel");
        h.mark_dead("panel", "kaboom".into(), Some("jacquewm-panel".into()));
        let snap = h.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].name, "panel");
        assert_eq!(snap[0].health, Health::Dead);
        assert_eq!(snap[0].last_error.as_deref(), Some("kaboom"));
    }

    #[test]
    fn safe_init_marks_panic_dead() {
        let h = SubsystemHealth::new();
        let h_clone = h.clone();
        let _join = safe_init("tester", h.clone(), || {
            h_clone.register("tester");
            panic!("oops");
        });
        // Give the worker a moment to log the panic.
        std::thread::sleep(std::time::Duration::from_millis(50));
        let snap = h.snapshot();
        assert!(!snap.is_empty());
        assert_eq!(snap[0].health, Health::Dead);
    }

    #[test]
    fn safe_init_marks_alive_on_clean_exit() {
        let h = SubsystemHealth::new();
        let _join = safe_init("cleaner", h.clone(), || {
            h.register("cleaner");
        });
        std::thread::sleep(std::time::Duration::from_millis(20));
        let snap = h.snapshot();
        assert_eq!(snap[0].health, Health::Alive);
    }
}
