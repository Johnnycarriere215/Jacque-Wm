//! Configuration management.
//!
//! Loads TOML configuration from
//! `%APPDATA%\JacqueWM\config.toml`. The configuration is validated on
//! load. Invalid values fall back to defaults and are logged. The schema
//! is intentionally minimal so that Prompt 2 + Prompt 3/4 can extend it
//! without breaking existing config files.
//!
//! Prompt 2 + 3 + 4 add sections for panel, tiling gaps, theme signals,
//! launcher / tray / notifications tuning, startup-time behaviour, the
//! `debug_mode` toggle for [`crate::core::debug`], and the disabled-by-
//! default plugin hooks. Every new field is `#[serde(default)]` so a
//! hand-written Prompt 1 config keeps working; adding new sections later
//! is the same story.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::core::WorkspaceIndex;
use crate::error::{JacqueError, Result};

/// The runtime configuration used by JacqueWM.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Desktop that JacqueWM switches to on startup. Must be 1..=9.
    pub startup_desktop: WorkspaceIndex,

    /// Total number of workspaces that JacqueWM will keep alive.
    /// Currently fixed to 9, but exposed for forward compatibility.
    pub workspace_count: u8,

    /// If true, after moving a window to another desktop the user is
    /// automatically switched to that desktop. Defaults to `false`
    /// because the user spec asks us to leave the user on the current
    /// desktop after a move.
    pub follow_moved_windows: bool,

    /// If true, the logging subsystem writes to
    /// `%APPDATA%\JacqueWM\logs\jacquewm.YYYY-MM-DD.log`.
    pub enable_logging: bool,

    /// Optional logging filter — follows the same syntax as the
    /// `RUST_LOG` env variable. If empty, the default filter is used.
    #[serde(default)]
    pub log_filter: String,

    // ----------------------------------------------------------------
    // Prompt 2 + 3 sections — every field defaults so existing configs
    // continue to load identically.
    // ----------------------------------------------------------------
    #[serde(default)]
    pub panel: PanelSection,
    #[serde(default)]
    pub tiling: TilingSection,
    #[serde(default)]
    pub theme: ThemeSection,
    #[serde(default)]
    pub launcher: LauncherSection,
    #[serde(default)]
    pub tray: TraySection,
    #[serde(default)]
    pub notifications: NotificationSection,
    #[serde(default)]
    pub startup: StartupSection,
    #[serde(default)]
    pub debug: DebugSection,
    #[serde(default)]
    pub plugins: PluginSection,
}

impl Default for Config {
    fn default() -> Self {
        Self::defaults()
    }
}

// =====================================================================
// Prompt 2 + 3 sub-sections
// =====================================================================

/// Top-panel placement, transparency, and refresh knobs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PanelSection {
    /// Pixel height of the panel. Spec allows 30..=34.
    pub height: i32,
    /// Opacity 0..=1.
    pub opacity: f32,
    /// `true` if the panel should overlay fullscreen windows by default.
    pub visible_in_fullscreen: bool,
    /// Refresh interval for the metrics section (CPU/GPU/RAM/net)
    /// in milliseconds. Lower = smoother but more wakeups.
    pub metrics_refresh_ms: u32,
    /// Names of workspaces shown to the user in the LEFT section.
    /// If empty, defaults to "1".."9".
    #[serde(default)]
    pub custom_names: Vec<String>,
}

impl Default for PanelSection {
    fn default() -> Self {
        Self {
            height: 32,
            opacity: 0.92,
            visible_in_fullscreen: true,
            metrics_refresh_ms: 1000,
            custom_names: Vec::new(),
        }
    }
}

/// Tiling-engine knobs — gap inner/outer, smart-gaps toggle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TilingSection {
    pub outer_gap: i32,
    pub inner_gap: i32,
    /// When only one tiled window exists, drop outer gaps.
    pub smart_gaps: bool,
    /// Bar thickness for the drag divider — in screen pixels.
    pub divider_thickness_px: i32,
    /// Divider colour override (0xAARRGGBB). 0 = use theme.
    #[serde(default)]
    pub divider_color_override: u32,
}

impl Default for TilingSection {
    fn default() -> Self {
        Self {
            outer_gap: 6,
            inner_gap: 6,
            smart_gaps: true,
            divider_thickness_px: 6,
            divider_color_override: 0,
        }
    }
}

/// Theme tokens the panel + tiled-aware UI consume. The full
/// [`crate::core::panel::state::ThemePalette`] lives in core and is
/// resolved by the [`ThemeManager`]. This config section only changes
/// *which* named theme is active.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ThemeSection {
    /// `"omarchy-dark"` is the only built-in today. The
    /// [`crate::core::theme::ThemeManager`] treats unknown names as
    /// `omarchy-dark` and logs.
    pub name: String,
    /// Animation speed multiplier. 1.0 = stock.
    pub animation_speed: f32,
}

impl Default for ThemeSection {
    fn default() -> Self {
        Self {
            name: "omarchy-dark".into(),
            animation_speed: 1.0,
        }
    }
}

/// App launcher toggle + hotkey + behaviour.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LauncherSection {
    pub enabled: bool,
    /// Width in pixels of the launcher centre panel.
    pub width: i32,
    /// Height in pixels.
    pub height: i32,
    /// Maximum number of recent results to display.
    pub max_results: usize,
    /// Auto-dismiss after `n` seconds of inactivity (0 = never).
    pub auto_dismiss_seconds: u32,
}

impl Default for LauncherSection {
    fn default() -> Self {
        Self {
            enabled: true,
            width: 640,
            height: 360,
            max_results: 32,
            auto_dismiss_seconds: 0,
        }
    }
}

/// System-tray toggle and menu contents.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TraySection {
    pub enabled: bool,
    pub show_icon: bool,
    pub allow_exit: bool,
    pub allow_restart: bool,
    pub allow_open_logs: bool,
}

impl Default for TraySection {
    fn default() -> Self {
        Self {
            enabled: true,
            show_icon: true,
            allow_exit: true,
            allow_restart: true,
            allow_open_logs: true,
        }
    }
}

/// Notification defaults — duration, vertical stacking, sound.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NotificationSection {
    pub enabled: bool,
    pub duration_ms: u32,
    pub max_visible: usize,
    pub play_sound: bool,
}

impl Default for NotificationSection {
    fn default() -> Self {
        Self {
            enabled: true,
            duration_ms: 3500,
            max_visible: 4,
            play_sound: false,
        }
    }
}

/// Startup-time behaviour — boot-on-login, first-run UX, opening
/// windows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StartupSection {
    /// Opt-in `HKCU\...\Run` registration.
    pub auto_start: bool,
    /// If true, wait for Explorer.exe to be ready before bringing
    /// up the panel. Settings → false speeds boot but risks flicker.
    pub wait_for_explorer: bool,
    /// Maximum seconds to wait for Explorer.
    pub explorer_wait_timeout_secs: u64,
    /// Try-restart on crash — does NOT install a service; just
    /// respawns if the previous run died.
    pub crash_watchdog: bool,
}

impl Default for StartupSection {
    fn default() -> Self {
        Self {
            auto_start: false,
            wait_for_explorer: true,
            explorer_wait_timeout_secs: 30,
            crash_watchdog: false,
        }
    }
}

/// Debug knobs. The DebugManager [`crate::core::debug`] refuses to
/// expose anything unless `debug_mode = true`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DebugSection {
    pub debug_mode: bool,
    pub log_to_stdout: bool,
    pub layout_visualization: bool,
}

impl Default for DebugSection {
    fn default() -> Self {
        Self {
            debug_mode: false,
            log_to_stdout: false,
            layout_visualization: false,
        }
    }
}

/// Plugin hooks. **Architectural only** for Prompt 2 (Part 4). The
/// [`crate::core::plugins`] module declares the trait and lifecycle
/// hooks; no plugin is ever loaded at runtime.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PluginSection {
    /// Future-friendly directory hint; ignored for now.
    pub plugin_dir: String,
    /// Future-friendly allow-list. Empty = no plugins will ever be
    /// loaded even when the loader lands.
    pub allow: Vec<String>,
    /// Future-friendly deny-list.
    pub deny: Vec<String>,
}

impl Default for PluginSection {
    fn default() -> Self {
        Self {
            plugin_dir: String::new(),
            allow: Vec::new(),
            deny: Vec::new(),
        }
    }
}

// =====================================================================
// Config — defaults and validators
// =====================================================================

impl Config {
    /// Construct the default configuration.
    pub fn defaults() -> Self {
        Self {
            startup_desktop: WorkspaceIndex::new_unchecked(1),
            workspace_count: WorkspaceIndex::COUNT,
            follow_moved_windows: false,
            enable_logging: true,
            log_filter: String::new(),
            panel: PanelSection::default(),
            tiling: TilingSection::default(),
            theme: ThemeSection::default(),
            launcher: LauncherSection::default(),
            tray: TraySection::default(),
            notifications: NotificationSection::default(),
            startup: StartupSection::default(),
            debug: DebugSection::default(),
            plugins: PluginSection::default(),
        }
    }

    /// Returns the default log filter when the user has not configured
    /// one explicitly.
    pub fn default_log_filter() -> &'static str {
        "jacquewm=info"
    }

    /// Validate the configuration, returning typed errors instead of
    /// silent fallbacks. This is called once on load. Sub-sections are
    /// clamped at use-site, not in this validator, so a slightly out of
    /// range value never prevents the system from booting.
    pub fn validate(&self) -> Result<()> {
        if !(1..=WorkspaceIndex::COUNT).contains(&self.startup_desktop.get()) {
            return Err(JacqueError::ConfigValidation(format!(
                "startup_desktop must be 1..={}",
                WorkspaceIndex::COUNT
            )));
        }
        if self.workspace_count == 0 || self.workspace_count > WorkspaceIndex::COUNT {
            return Err(JacqueError::ConfigValidation(format!(
                "workspace_count must be 1..={}",
                WorkspaceIndex::COUNT
            )));
        }
        if !(30..=34).contains(&self.panel.height) {
            tracing::warn!(
                target: "jacquewm.config",
                value = self.panel.height,
                "panel.height outside 30..=34; clamping to 32"
            );
        }
        Ok(())
    }

    /// Builder-style: apply a panel height clamp before validation.
    pub fn clamped(mut self) -> Self {
        if !(30..=34).contains(&self.panel.height) {
            self.panel.height = 32;
        }
        if !(0.0..=1.0).contains(&self.panel.opacity) {
            self.panel.opacity = 0.92;
        }
        if self.tiling.inner_gap < 0 {
            self.tiling.inner_gap = 0;
        }
        if self.tiling.outer_gap < 0 {
            self.tiling.outer_gap = 0;
        }
        if self.tiling.divider_thickness_px < 2 {
            self.tiling.divider_thickness_px = 2;
        }
        self
    }
}

/// The on-disk serialised form. Mirrors [`Config`] but uses raw
/// integers for fields that map to newtype wrappers, so that a hand
/// edited TOML file remains readable.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RawConfig {
    #[serde(default = "default_startup")]
    startup_desktop: u8,
    #[serde(default = "default_count")]
    workspace_count: u8,
    #[serde(default)]
    follow_moved_windows: bool,
    #[serde(default = "default_logging")]
    enable_logging: bool,
    #[serde(default)]
    log_filter: String,
    #[serde(default)]
    panel: PanelSection,
    #[serde(default)]
    tiling: TilingSection,
    #[serde(default)]
    theme: ThemeSection,
    #[serde(default)]
    launcher: LauncherSection,
    #[serde(default)]
    tray: TraySection,
    #[serde(default)]
    notifications: NotificationSection,
    #[serde(default)]
    startup: StartupSection,
    #[serde(default)]
    debug: DebugSection,
    #[serde(default)]
    plugins: PluginSection,
}

fn default_startup() -> u8 {
    1
}
fn default_count() -> u8 {
    WorkspaceIndex::COUNT
}
fn default_logging() -> bool {
    true
}

impl Default for RawConfig {
    fn default() -> Self {
        Self {
            startup_desktop: default_startup(),
            workspace_count: default_count(),
            follow_moved_windows: false,
            enable_logging: default_logging(),
            log_filter: String::new(),
            panel: PanelSection::default(),
            tiling: TilingSection::default(),
            theme: ThemeSection::default(),
            launcher: LauncherSection::default(),
            tray: TraySection::default(),
            notifications: NotificationSection::default(),
            startup: StartupSection::default(),
            debug: DebugSection::default(),
            plugins: PluginSection::default(),
        }
    }
}

impl From<RawConfig> for Config {
    fn from(raw: RawConfig) -> Self {
        Self {
            startup_desktop: WorkspaceIndex::new(raw.startup_desktop)
                .unwrap_or(WorkspaceIndex::new_unchecked(1)),
            workspace_count: if (1..=WorkspaceIndex::COUNT).contains(&raw.workspace_count) {
                raw.workspace_count
            } else {
                warn!(
                    target: "jacquewm.config",
                    value = raw.workspace_count,
                    "invalid workspace_count in config; falling back to default"
                );
                WorkspaceIndex::COUNT
            },
            follow_moved_windows: raw.follow_moved_windows,
            enable_logging: raw.enable_logging,
            log_filter: raw.log_filter,
            panel: raw.panel,
            tiling: raw.tiling,
            theme: raw.theme,
            launcher: raw.launcher,
            tray: raw.tray,
            notifications: raw.notifications,
            startup: raw.startup,
            debug: raw.debug,
            plugins: raw.plugins,
        }
    }
}

// =====================================================================
// ConfigManager — thread-safe loader, holder and writer.
// =====================================================================

/// Thread-safe configuration holder and loader.
///
/// `ConfigManager` keeps a live copy of the validated configuration in
/// memory and exposes accessor methods that other subsystems use. It
/// owns the canonical path to the on-disk file but never blocks other
/// subsystems on I/O once initialised.
#[derive(Clone)]
pub struct ConfigManager {
    inner: Arc<RwLock<Config>>,
    path: PathBuf,
}

impl ConfigManager {
    /// Returns the canonical configuration file path.
    ///
    /// Resolution order:
    /// 1. `JACQUEWM_CONFIG` environment variable.
    /// 2. `%APPDATA%\JacqueWM\config.toml`.
    pub fn canonical_path() -> Result<PathBuf> {
        if let Ok(env_path) = std::env::var("JACQUEWM_CONFIG") {
            if !env_path.is_empty() {
                return Ok(PathBuf::from(env_path));
            }
        }
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(dirs::config_dir)
            .ok_or_else(|| JacqueError::ConfigLoad("could not resolve APPDATA".into()))?;
        Ok(base.join("JacqueWM").join("config.toml"))
    }

    /// Construct a manager that wraps the given already-parsed config.
    ///
    /// This is intended for tests and for the auto-generated bootstrap
    /// path. Use [`Self::load`] for the standard on-disk boot path.
    pub fn new(config: Config, path: PathBuf) -> Self {
        Self {
            inner: Arc::new(RwLock::new(config)),
            path,
        }
    }

    /// Load the configuration from disk, falling back to defaults and
    /// creating the directory tree + file on first launch.
    pub fn load() -> Result<Self> {
        let path = Self::canonical_path()?;
        Self::load_from(&path)
    }

    /// Load from the given path. Mirrors [`Self::load`] but is path
    /// parameterised for tests.
    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            info!(
                target: "jacquewm.config",
                path = %path.display(),
                "configuration file does not exist; writing defaults"
            );
            let mut cfg = Config::defaults().clamped();
            cfg.validate()?;
            Self::write_to(&cfg, path)?;
            return Ok(Self::new(cfg, path.to_path_buf()));
        }

        let raw_text = std::fs::read_to_string(path).map_err(|e| {
            JacqueError::ConfigLoad(format!("could not read {}: {}", path.display(), e))
        })?;

        let raw: RawConfig = match toml::from_str(&raw_text) {
            Ok(r) => r,
            Err(e) => {
                warn!(
                    target: "jacquewm.config",
                    error = %e,
                    "configuration file is malformed; falling back to defaults"
                );
                RawConfig::default()
            }
        };

        let mut config: Config = raw.into();
        config = config.clamped();
        if let Err(e) = config.validate() {
            warn!(
                target: "jacquewm.config",
                error = %e,
                "configuration validation failed; falling back to defaults"
            );
            return Ok(Self::new(Config::defaults().clamped(), path.to_path_buf()));
        }
        Ok(Self::new(config, path.to_path_buf()))
    }

    /// Persist the in-memory configuration back to disk.
    pub fn save(&self) -> Result<()> {
        let guard = self.inner.read();
        Self::write_to(&guard, &self.path)
    }

    /// Returns the canonical path of the config file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns a snapshot of the current configuration.
    pub fn snapshot(&self) -> Config {
        self.inner.read().clone()
    }

    /// Replace the in-memory configuration.
    ///
    /// This is intended for the parts of JacqueWM that may tweak the
    /// configuration at runtime. The new value is validated before
    /// being stored.
    pub fn replace(&self, new: Config) -> Result<()> {
        let clamped = new.clamped();
        clamped.validate()?;
        *self.inner.write() = clamped;
        Ok(())
    }

    /// Read-only accessor for the inner `RwLock<Config>` Arc, intended
    /// for the live-reload watcher. Callers must not mutate.
    pub fn inner_arc(&self) -> Arc<RwLock<Config>> {
        self.inner.clone()
    }

    fn write_to(cfg: &Config, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let serialised = toml::to_string_pretty(cfg).map_err(|e| {
            JacqueError::ConfigLoad(format!("could not serialise config: {e}"))
        })?;
        std::fs::write(path, serialised).map_err(|e| {
            JacqueError::ConfigLoad(format!("could not write {}: {}", path.display(), e))
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip() {
        let cfg = Config::defaults();
        cfg.validate().unwrap();
        assert_eq!(cfg.startup_desktop.get(), 1);
        assert_eq!(cfg.workspace_count, WorkspaceIndex::COUNT);
        assert!(!cfg.follow_moved_windows);
        assert!(cfg.enable_logging);
        // Prompt 2/3/4 sub-sections all default sane:
        assert_eq!(cfg.panel.height, 32);
        assert!(cfg.launcher.enabled);
        assert!(cfg.tray.enabled);
        assert!(!cfg.debug.debug_mode);
        assert!(cfg.plugins.allow.is_empty());
    }

    #[test]
    fn invalid_index_falls_back() {
        let raw: RawConfig = toml::from_str("startup_desktop = 99\nworkspace_count = 9").unwrap();
        let cfg: Config = raw.into();
        assert_eq!(cfg.startup_desktop.get(), 1);
    }

    #[test]
    fn validate_rejects_out_of_range() {
        let mut cfg = Config::defaults();
        cfg.workspace_count = 0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn load_does_not_duplicate_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jacquewm.toml");
        let mgr = ConfigManager::load_from(&path).unwrap();
        assert!(path.exists());
        // Reload — should produce the same configuration.
        let mgr2 = ConfigManager::load_from(&path).unwrap();
        assert_eq!(mgr.snapshot().startup_desktop.get(), mgr2.snapshot().startup_desktop.get());
    }

    #[test]
    fn clamps_panel_height_to_default_window() {
        let mut cfg = Config::defaults();
        cfg.panel.height = 999;
        let cfg = cfg.clamped();
        assert_eq!(cfg.panel.height, 32);
    }

    #[test]
    fn clamps_negative_gap_to_zero() {
        let mut cfg = Config::defaults();
        cfg.tiling.inner_gap = -3;
        let cfg = cfg.clamped();
        assert_eq!(cfg.tiling.inner_gap, 0);
    }

    #[test]
    fn backward_compatible_minimal_config() {
        // A Prompt 1 era minimalist config — only the top-level
        // fields — must still load.
        let raw = r#"
            startup_desktop = 3
            workspace_count = 9
            follow_moved_windows = false
            enable_logging = true
            log_filter = ""
        "#;
        let r: RawConfig = toml::from_str(raw).unwrap();
        let cfg: Config = r.into();
        assert_eq!(cfg.startup_desktop.get(), 3);
        assert_eq!(cfg.panel.height, 32);
        assert_eq!(cfg.theme.name, "omarchy-dark");
        assert!(cfg.launcher.enabled);
    }
}
