//! Windows boot-time helpers for JacqueWM.
//!
//! Currently exposes:
//! * [`register`]     — toggle for the `HKCU\...\Run` auto-start entry.
//! * [`run_boot`]     — function that walks the entire boot sequence.

use std::time::Duration;
use tracing::{info, warn};

use crate::core::config::ConfigManager;
use crate::core::logging;
use crate::core::startup::{Phase, Startup};
use crate::error::{JacqueError, Result};
use crate::platform::windows::api::{registry, shell_wait};

/// Register (or unregister) JacqueWM for auto-start at login.
///
/// `enabled == true` writes the path of the running executable to
/// `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`. `enabled ==
/// false` removes the entry.
pub fn toggle_auto_start(enabled: bool) -> Result<()> {
    use crate::platform::windows::api::process;
    let exe = process::current_exe().map_err(|e| JacqueError::AutoStart(e.to_string()))?;
    let path_str = exe.to_string_lossy().to_string();
    if enabled {
        registry::register_auto_start(&path_str)?;
        info!(target: "jacquewm.startup", path = %path_str, "auto-start registered");
    } else {
        registry::unregister_auto_start()?;
        info!(target: "jacquewm.startup", "auto-start unregistered");
    }
    Ok(())
}

/// Run the full boot sequence.
///
/// The function returns a [`BootContext`] that the main function uses
/// to wire subsystems together. The boot sequence is intentionally
/// *let-it-fail-and-continue* — every step has its own OK/error path
/// and never panics.
pub fn run_boot(startup: &Startup, timeout: Option<Duration>) -> Result<BootContext> {
    startup.advance(Phase::ExplorerReady);
    shell_wait::wait_for_explorer(timeout)?;

    startup.advance(Phase::LoggerReady);
    let cfg = ConfigManager::load().inspect_err(|e| warn!(target: "jacquewm.startup", error = %e, "config load failed; fallbacks will apply"))?;

    startup.advance(Phase::ConfigReady);

    logging::init(
        &cfg.snapshot().log_filter,
        false,
        cfg.snapshot().enable_logging,
    )
    .inspect_err(|e| warn!(target: "jacquewm.startup", error = %e, "logging init failed; continuing without file log"))?;

    info!(
        target: "jacquewm.startup",
        version = env!("CARGO_PKG_VERSION"),
        "JacqueWM boot sequence started"
    );

    Ok(BootContext { config: cfg })
}

/// Holder returned by [`run_boot`] so the caller can hand off the
/// loaded config to the rest of the system.
pub struct BootContext {
    /// The loaded, validated, and persisted configuration manager.
    pub config: ConfigManager,
}
