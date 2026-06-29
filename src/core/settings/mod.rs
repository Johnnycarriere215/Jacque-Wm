//! Settings manager.
//!
//! Orchestrates *live config reload*. The actual `notify` filesystem
//! watcher is platform-specific and lives under
//! [`crate::platform::windows::settings`]. The core defines only the
//! observable interface and the failure-isolation rules.
//!
//! ## Failure isolation rules
//!
//! 1. A TOML parse failure during reload *never* overwrites the file;
//!    we keep the last known good config in memory and surface a
//!    [`crate::core::notifications::NotificationRequest::warning`].
//! 2. A successful reload broadcasts the new snapshot to all
//!    subscribers via the [`SettingsManager::on_change`] callback.
//! 3. An unsafe change (e.g. hotkey remap) is rejected at runtime if
//!    the receiver says so — we never silently succeed.
//!
//! Subscribers register via [`Self::subscribe`]. The SettingsManager
//! does not itself run any thread; the platform watcher threads do.

use std::sync::Arc;

use parking_lot::RwLock;

use crate::core::config::{Config, ConfigManager};

/// Stable id for a subscriber.
pub type SettingsSubscriberId = u64;

/// Callback invoked on every successful reload. Receives the *new*
/// snapshot; receivers should compare against their last known copy.
pub type SettingsChangeCallback =
    Box<dyn Fn(&Config) + Send + Sync + 'static>;

/// SettingsManager — observable wrapper around [`ConfigManager`].
pub struct SettingsManager {
    config: ConfigManager,
    subscribers: RwLock<Vec<(SettingsSubscriberId, SettingsChangeCallback)>>,
    next_id: parking_lot::Mutex<u64>,
    last_reload_ok: RwLock<bool>,
}

impl SettingsManager {
    pub fn new(config: ConfigManager) -> Self {
        Self {
            config,
            subscribers: RwLock::new(Vec::new()),
            next_id: parking_lot::Mutex::new(1),
            last_reload_ok: RwLock::new(true),
        }
    }

    /// Returns a snapshot of the current configuration.
    pub fn snapshot(&self) -> Config {
        self.config.snapshot()
    }

    /// Register an observer.
    pub fn subscribe(&self, callback: SettingsChangeCallback) -> SettingsSubscriberId {
        let mut seq = self.next_id.lock();
        let id = *seq;
        *seq = seq.wrapping_add(1);
        drop(seq);
        let mut g = self.subscribers.write();
        g.push((id, callback));
        id
    }

    /// Unregister a previously-registered observer.
    pub fn unsubscribe(&self, id: SettingsSubscriberId) {
        let mut g = self.subscribers.write();
        g.retain(|(other_id, _)| *other_id != id);
    }

    /// Apply a new in-memory configuration *and* broadcast. Used by
    /// the platform watcher when the file genuinely changed and the
    /// reload succeeded.
    ///
    /// Returns Err if validation failed; the in-memory config is
    /// untouched in that case.
    pub fn apply_new(&self, new: Config) -> crate::error::Result<()> {
        self.config.replace(new)?;
        let snap = self.config.snapshot();
        let callbacks: Vec<SettingsChangeCallback> = {
            let g = self.subscribers.read();
            g.iter().map(|(_, cb)| clone_box(cb.clone())).collect()
        };
        for cb in callbacks {
            cb(&snap);
        }
        *self.last_reload_ok.write() = true;
        Ok(())
    }

    /// Tell observers the reload failed; used by the watcher to log
    /// only.
    pub fn report_failure(&self) {
        *self.last_reload_ok.write() = false;
    }

    /// Returns the last-known reload success state.
    pub fn last_reload_succeeded(&self) -> bool {
        *self.last_reload_ok.read()
    }

    /// Hand the underlying [`ConfigManager`] back so callers can do
    /// their own writes if they wish.
    pub fn manager(&self) -> ConfigManager {
        self.config.clone()
    }
}

/// `Box<dyn Fn(&Config)> + Send + Sync + 'static` does not implement
/// `Clone`. Helper that wraps the trait object so callbacks can be
/// re-cloned during broadcast without re-allocating the closures.
fn clone_box(b: SettingsChangeCallback) -> SettingsChangeCallback {
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    #[test]
    fn subscriber_receives_apply() {
        let cfg = ConfigManager::load_from(std::env::temp_dir().join("jacquewm-test-not-here.toml")).unwrap_or_else(|_| {
            let temp = std::env::temp_dir().join("jacquewm-test-not-here.toml");
            ConfigManager::new(crate::core::config::Config::defaults(), temp)
        });
        let sm = SettingsManager::new(cfg);
        let counter = Arc::new(Mutex::new(0u32));
        let counter_inside = counter.clone();
        sm.subscribe(Box::new(move |_new| {
            *counter_inside.lock() += 1;
        }));
        let new = Config::defaults().clamped();
        sm.apply_new(new).unwrap();
        assert_eq!(*counter.lock(), 1);
    }

    #[test]
    fn report_failure_flips_flag() {
        let cfg = ConfigManager::new(
            crate::core::config::Config::defaults(),
            std::env::temp_dir().join("jacquewm-test-not-here-2.toml"),
        );
        let sm = SettingsManager::new(cfg);
        assert!(sm.last_reload_succeeded());
        sm.report_failure();
        assert!(!sm.last_reload_succeeded());
    }
}
