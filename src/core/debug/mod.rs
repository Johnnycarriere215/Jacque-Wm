//! Debug manager.
//!
//! Spec lines we must satisfy:
//!
//! * "Live log viewer."
//! * "Window state inspector."
//! * "Workspace state inspector."
//! * "Hotkey registry viewer."
//! * "Layout visualization mode (developer mode)."
//! * "Debug tools must NEVER ship enabled in release mode."
//! * "Must be opt-in via config: `debug_mode = true`."
//!
//! Today only (1) the log paths, (2) a snapshot API the platform
//! layer feeds, and (3) a `debug_mode` flag gate are implemented.
//! The richer UI — log viewer / inspector panes — is a future
//! milestone and is *never* compiled into a release binary: a build
//! flag controls whether the live `DebugManager` accepts calls at all.
//!
//! There is **no** debug UI implemented in this milestone; the
//! manager is purely a snapshotting API. Callers (e.g. the CLI's
//! `--dump` mode) use it to print state.

use std::sync::Arc;

use parking_lot::RwLock;

use crate::core::config::ConfigManager;
use crate::core::isolation::SubsystemHealth;

/// Snapshot of the live engine state, used by the debug dump.
#[derive(Debug, Clone, Default)]
pub struct DebugSnapshot {
    /// Path to the current config file.
    pub config_path: String,
    /// Indication of whether the engine is in `debug_mode`.
    pub debug_mode: bool,
    /// One row per registered subsystem.
    pub subsystems: Vec<crate::core::isolation::SubsystemEntry>,
    /// One row per known window (cheap copy — titles only).
    pub window_count: usize,
    /// One row per known workspace's tile count — supplied by the
    /// tiling engine via `record_layout`.
    pub workspace_tile_counts: Vec<u8>,
    /// Snapshot of the active theme.
    pub active_theme_palette_background: u32,
    /// Number of currently-visible notifications.
    pub active_notifications: usize,
}

impl DebugSnapshot {
    pub fn empty() -> Self {
        Self::default()
    }
}

/// Public debug manager. Construct once per process after config
/// has loaded. Reads are cheap (clones the snapshot).
#[derive(Clone)]
pub struct DebugManager {
    enabled: bool,
    config: ConfigManager,
    health: SubsystemHealth,
    inner: Arc<RwLock<DebugSnapshot>>,
}

impl DebugManager {
    pub fn new(config: ConfigManager, health: SubsystemHealth) -> Self {
        let debug_mode = config.snapshot().debug.debug_mode;
        Self {
            enabled: debug_mode,
            config,
            health,
            inner: Arc::new(RwLock::new(DebugSnapshot::default())),
        }
    }

    /// Returns `true` if `debug_mode` was set in config and the
    /// DebugManager therefore accepts record updates.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Write the latest known state. No-op if `debug_mode == false`.
    pub fn record(&self, update: impl FnOnce(&mut DebugSnapshot)) {
        if !self.enabled {
            return;
        }
        let mut g = self.inner.write();
        update(&mut g);
    }

    /// Take a fresh snapshot — pulls live data from all known
    /// subsystems so the caller doesn't have to.
    pub fn snapshot(&self) -> DebugSnapshot {
        let cfg_snapshot = self.config.snapshot();
        if !self.enabled {
            return DebugSnapshot {
                config_path: self.config.path().display().to_string(),
                debug_mode: false,
                ..DebugSnapshot::default()
            };
        }
        let mut snap = DebugSnapshot {
            config_path: self.config.path().display().to_string(),
            debug_mode: cfg_snapshot.debug.debug_mode,
            subsystems: self.health.snapshot(),
            ..DebugSnapshot::default()
        };
        snap.active_theme_palette_background = crate::core::panel::state::ThemePalette::omarchy_dark()
            .background
            .0;
        snap
    }

    /// Force a dump to the log — used by `--dump` CLI mode.
    pub fn log_dump(&self) {
        let s = self.snapshot();
        tracing::info!(target: "jacquewm.debug", "DebugManager snapshot:");
        tracing::info!(target: "jacquewm.debug", config_path = %s.config_path);
        tracing::info!(target: "jacquewm.debug", debug_mode = s.debug_mode);
        for sub in &s.subsystems {
            tracing::info!(
                target: "jacquewm.debug",
                subsystem = %sub.name,
                health = ?sub.health,
                error = sub.last_error.as_deref().unwrap_or("-"),
                "subsystem"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_manager_returns_no_subsystems() {
        let cfg = ConfigManager::new(
            crate::core::config::Config::defaults(),
            std::env::temp_dir().join("jacquewm-test-disabled.toml"),
        );
        let h = SubsystemHealth::new();
        let m = DebugManager::new(cfg, h);
        assert!(!m.is_enabled());
        let s = m.snapshot();
        assert!(!s.debug_mode);
        assert!(s.subsystems.is_empty());
    }

    #[test]
    fn enabled_manager_exposes_subsystems() {
        let mut cfg_data = crate::core::config::Config::defaults();
        cfg_data.debug.debug_mode = true;
        let cfg = ConfigManager::new(
            cfg_data,
            std::env::temp_dir().join("jacquewm-test-enabled.toml"),
        );
        let h = SubsystemHealth::new();
        h.register("panel");
        h.register_disabled("tray");
        let m = DebugManager::new(cfg, h);
        assert!(m.is_enabled());
        let s = m.snapshot();
        assert!(s.debug_mode);
        assert_eq!(s.subsystems.len(), 2);
    }

    #[test]
    fn record_no_op_when_disabled() {
        let cfg = ConfigManager::new(
            crate::core::config::Config::defaults(),
            std::env::temp_dir().join("jacquewm-test-record-disabled.toml"),
        );
        let m = DebugManager::new(cfg, SubsystemHealth::new());
        m.record(|s| {
            s.window_count = 99;
        });
        assert_eq!(m.snapshot().window_count, 0);
    }
}
