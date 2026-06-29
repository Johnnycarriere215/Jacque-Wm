//! OS-agnostic subsystem code.
//!
//! Prompt 1 introduced the workspace engine, hotkey manager, virtual
//! desktop adapter, configuration loader, logger, and startup
//! orchestrator.
//!
//! Prompt 2 *adds* the top panel data-model, the event-driven
//! WindowManager, the tiling engine, the application-rules engine,
//! the focus tracker, and shared metric types without removing or
//! renaming anything from Prompt 1.

pub mod config;
pub mod logging;
pub mod workspaces;
pub mod windows;
pub mod virtual_desktop;
pub mod hotkeys;
pub mod startup;

// =====================================================================
// New in Prompt 2 — additive; never modifies the Prompt 1 modules.
// =====================================================================

pub mod apps;
pub mod focus;
pub mod metrics;
pub mod panel;
pub mod tiling;
pub mod wm;

// =====================================================================
// New in Prompt 2 (Parts 3 + 4) — additive; no further refactoring.
// =====================================================================

pub mod debug;
pub mod isolation;
pub mod launcher;
pub mod notifications;
pub mod plugins;
pub mod settings;
pub mod theme;
pub mod tray;

// =====================================================================
// Shared core types
// =====================================================================

/// Stable, 1-based desktop index.
///
/// The integer always lies in `1..=9`. Convert via [`WorkspaceIndex::new`]
/// (returns `Result`) or [`WorkspaceIndex::new_unchecked`] (panics).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkspaceIndex(u8);

impl WorkspaceIndex {
    /// The total number of workspaces that JacqueWM manages.
    pub const COUNT: u8 = 9;

    /// Construct a workspace index from a 1-based integer.
    ///
    /// # Errors
    ///
    /// Returns [`JacqueError::InvalidWorkspaceIndex`] when the integer is
    /// outside `1..=9`.
    pub fn new(value: u8) -> crate::Result<Self> {
        if (1..=Self::COUNT).contains(&value) {
            Ok(Self(value))
        } else {
            Err(crate::JacqueError::InvalidWorkspaceIndex(value))
        }
    }

    /// Construct an unchecked workspace index. Crashes on out-of-range.
    #[inline]
    pub const fn new_unchecked(value: u8) -> Self {
        debug_assert!(value >= 1 && value <= Self::COUNT);
        Self(value)
    }

    /// The 1-based integer representation.
    #[inline]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl std::fmt::Display for WorkspaceIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl serde::Serialize for WorkspaceIndex {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(self.0)
    }
}

impl<'de> serde::Deserialize<'de> for WorkspaceIndex {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = u8::deserialize(deserializer)?;
        Self::new(raw).map_err(serde::de::Error::custom)
    }
}
