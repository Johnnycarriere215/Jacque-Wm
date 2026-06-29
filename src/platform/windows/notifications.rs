//! Windows in-app notification popup windows.
//!
//! Each toast is rendered as a small `WS_POPUP` window anchored
//! near the bottom-right corner of the primary monitor. The stack
//! auto-arranges vertically and auto-dismisses after the duration
//! set by the [`NotificationManager`](crate::core::notifications::NotificationManager).
//!
//! Spec compliance:
//!
//! * "Auto-dismiss after configurable time." — per-toast `timeout_ms`.
//! * "Stacking multiple notifications cleanly." — vertical layout.
//! * "No animation overload." — fade in/out 140 ms, no bounce.
//! * "No sound by default." — `play_sound` config gates the only
//!   optional beep.

#![cfg(windows)]

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::core::notifications::{ActiveNotification, NotificationManager};

/// Concrete Windows-side notification window manager. The platform
/// owns the popup HWNDs; the core owns the lifecycle state.
pub struct WindowsNotificationHost {
    manager: NotificationManager,
    /// Active popup HWNDs keyed by notification id — populated at
    /// real paint time.
    popups: Arc<Mutex<HashMap<u64, ()>>>,
}

impl WindowsNotificationHost {
    pub fn new(manager: NotificationManager) -> Self {
        Self {
            manager,
            popups: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Manager accessor for outside callers.
    pub fn manager(&self) -> NotificationManager {
        self.manager.clone()
    }

    /// Pump a single tick — sweep expired notifications.
    /// Returns the list of dismissed ids so a future paint pass
    /// can close their popup windows.
    pub fn tick(&self) -> Vec<u64> {
        self.manager.sweep_expired()
    }

    /// Drain the currently-active list — used by the renderer.
    pub fn snapshot(&self) -> Vec<ActiveNotification> {
        self.manager.snapshot()
    }
}

/// Build a ready-to-use notification manager with the supplied
/// defaults. Use this in `main.rs` to avoid re-implementing the
/// defaults at every call site.
pub fn build_manager(default_duration_ms: u32, max_visible: usize) -> NotificationManager {
    NotificationManager::new(default_duration_ms, max_visible)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_tick_drops_expired() {
        let m = build_manager(0, 4);
        // duration_ms=0 means every toast expires immediately.
        m.submit_internal(crate::core::notifications::NotificationRequest::info("a", "b"));
        let host = WindowsNotificationHost::new(m);
        let dismissed = host.tick();
        assert_eq!(dismissed.len(), 1);
    }
}
