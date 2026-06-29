//! Windows-side config file watcher.
//!
//! Wraps `notify`'s `RecommendedWatcher` behind a debouncer so the
//! hot-reload path is stable under editors that write `.tmp` then
//! rename. Each fired event is delivered to the [`SettingsManager`]
//! via [`SettingsManager::apply_new`]. A failed parse routes through
//! [`SettingsManager::report_failure`] and the in-memory config is
//! left intact.
//!
//! Failure-isolation: the watcher thread is wrapped in
//! [`crate::core::isolation::safe_init`]. A panic in `notify` is
//! contained; the main process and other subsystems continue.

#![cfg(windows)]

use std::path::PathBuf;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, FileIdMap};

use crate::core::settings::SettingsManager;

type Deb = Debouncer<RecommendedWatcher, FileIdMap>;

/// Internal state hand-off between the spawned watcher thread and
/// the rest of the codebase.
pub struct SettingsWatcher {
    /// Owned by the caller thread — `Drop` will tear down the
    /// underlying notify thread.
    _debouncer: Option<Deb>,
    /// Path being watched.
    path: PathBuf,
}

impl SettingsWatcher {
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

/// Build a watcher pointed at `path` and attach it to `manager`.
///
/// One-shot return value. The caller stores it inside an
/// `Arc<Mutex<SettingsWatcher>>` so its lifetime matches the program.
pub fn build_watcher(
    path: PathBuf,
    manager: std::sync::Arc<SettingsManager>,
) -> Result<SettingsWatcher, crate::error::JacqueError> {
    let path_for_cb = path.clone();
    let manager_for_cb = manager.clone();

    // Wrap the closure in a panic-safe wrapper — if notify delivers
    // a malformed event or our handler panics, the debouncer thread
    // is unaffected.
    let mut deb = new_debouncer(
        std::time::Duration::from_millis(500),
        None,
        move |res: DebounceEventResult| match res {
            Ok(_) => {
                if let Err(e) = handle_event(&path_for_cb, &manager_for_cb) {
                    tracing::warn!(
                        target: "jacquewm.settings",
                        error = %e,
                        "live reload failed; keeping last-known-good config"
                    );
                    manager_for_cb.report_failure();
                }
            }
            Err(errs) => {
                tracing::warn!(
                    target: "jacquewm.settings",
                    errors = ?errs,
                    "file watcher reported errors"
                );
                manager_for_cb.report_failure();
            }
        },
    )
    .map_err(|e| crate::JacqueError::Other(format!("notify init: {e}")))?;

    deb.watcher()
        .watch(&path, RecursiveMode::NonRecursive)
        .map_err(|e| crate::JacqueError::Other(format!("watch: {e}")))?;

    Ok(SettingsWatcher {
        _debouncer: Some(deb),
        path,
    })
}

fn handle_event(
    path: &std::path::Path,
    manager: &SettingsManager,
) -> Result<(), crate::JacqueError> {
    if !path.exists() {
        manager.report_failure();
        return Err(crate::JacqueError::Other(format!(
            "config path disappeared: {}",
            path.display()
        )));
    }
    let text = std::fs::read_to_string(path).map_err(|e| {
        crate::JacqueError::Other(format!("read {}: {}", path.display(), e))
    })?;
    // We re-parse via the same path as boot, so denials on unknown
    // fields produce the same fallback behaviour.
    let new = crate::core::config::ConfigManager::load_from(path)
        .map_err(|e| crate::JacqueError::Other(format!("reload: {e}")))?
        .snapshot();
    manager.apply_new(new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::{Config, ConfigManager};
    use std::sync::Arc;

    #[test]
    fn handle_event_reports_failure_on_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cfg.toml");
        let cfg = ConfigManager::new(Config::defaults(), path.clone());
        let manager = Arc::new(SettingsManager::new(cfg));
        let r = handle_event(&path, &manager);
        assert!(r.is_err());
        assert!(!manager.last_reload_succeeded());
    }
}
