//! Central error types for JacqueWM.
//!
//! Every subsystem returns [`JacqueError`], which is built on
//! `thiserror`. The `Result` alias is provided for ergonomic propagation.

use std::fmt;
use thiserror::Error;

use crate::platform::windows::desktop::guids::ComInterfaceId;

/// Convenient alias used throughout JacqueWM.
pub type Result<T> = std::result::Result<T, JacqueError>;

/// The single error type used by JacqueWM subsystems.
///
/// Individual variants document the *failure mode* — not the precise
/// Win32 error code — so that higher layers can decide what to do
/// (e.g. retry, fall back, surface to the user).
#[derive(Debug, Error)]
pub enum JacqueError {
    /// The supplied workspace index is outside the supported range (1..=9).
    #[error("workspace index {0} is out of range (must be 1..=9)")]
    InvalidWorkspaceIndex(u8),

    /// Configuration file could not be loaded.
    #[error("could not load configuration: {0}")]
    ConfigLoad(String),

    /// Configuration file contained invalid values.
    #[error("invalid configuration: {0}")]
    ConfigValidation(String),

    /// Desktop creation failed.
    #[error("failed to create virtual desktop: {0}")]
    DesktopCreate(String),

    /// Desktop switch failed.
    #[error("failed to switch to desktop index {index}: {reason}")]
    DesktopSwitch {
        /// The target desktop index (1..=9).
        index: u8,
        /// Human readable reason.
        reason: String,
    },

    /// Desktop enumeration failed.
    #[error("failed to enumerate virtual desktops: {0}")]
    DesktopEnumeration(String),

    /// Window movement failed.
    #[error("failed to move window {hwnd:?} to desktop index {index}: {reason}")]
    WindowMove {
        /// Window handle (raw HWND value).
        hwnd: u64,
        /// Target desktop index (1..=9).
        index: u8,
        /// Human readable reason.
        reason: String,
    },

    /// Window enumeration failed.
    #[error("failed to enumerate windows: {0}")]
    WindowEnumeration(String),

    /// Keyboard hook failed to install.
    #[error("failed to install keyboard hook: {0}")]
    HookInstall(String),

    /// Hotkey registration failed.
    #[error("failed to register hotkey {0}")]
    HotkeyRegister(String),

    /// The COM subsystem returned a non-success HRESULT.
    #[error("COM call failed on interface {interface}: HRESULT 0x{hr:08X}")]
    Com {
        /// Interface that produced the failure.
        interface: ComInterfaceId,
        /// Raw HRESULT value.
        hr: u32,
    },

    /// COM apartment could not be initialised.
    #[error("COM initialization failed: {0}")]
    ComInit(String),

    /// Explorer.exe did not become ready in time.
    #[error("Explorer.exe was not ready after waiting {0:?} seconds")]
    ExplorerNotReady(Option<u64>),

    /// Auto-start registration failed.
    #[error("could not register JacqueWM for auto-start: {0}")]
    AutoStart(String),

    /// An IO error escaped into the public surface.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// An unsafe Win32 operation produced an unexpected handle.
    #[error("invalid handle: {0}")]
    InvalidHandle(String),

    /// Logging subsystem failed to initialise.
    #[error("logger initialization failed: {0}")]
    Logging(String),

    /// Generic / unclassified error.
    #[error("{0}")]
    Other(String),
}

impl JacqueError {
    /// Convert any `Display`able value into a [`JacqueError::Other`] variant.
    pub fn other<S: fmt::Display>(msg: S) -> Self {
        JacqueError::Other(msg.to_string())
    }
}

// =====================================================================
// Conversions — kept local to avoid coupling to anyhow elsewhere.
// =====================================================================

impl From<windows_result::Error> for JacqueError {
    fn from(err: windows_result::Error) -> Self {
        JacqueError::Com {
            interface: ComInterfaceId::Unknown,
            hr: err.code().0 as u32,
        }
    }
}

impl From<toml::de::Error> for JacqueError {
    fn from(err: toml::de::Error) -> Self {
        JacqueError::ConfigLoad(err.to_string())
    }
}

impl From<anyhow::Error> for JacqueError {
    fn from(err: anyhow::Error) -> Self {
        JacqueError::Other(format!("{err:#}"))
    }
}
