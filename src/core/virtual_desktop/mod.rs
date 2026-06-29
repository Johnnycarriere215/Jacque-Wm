//! Virtual desktop abstraction.
//!
//! The OS-agnostic core subsystems depend only on the
//! [`VirtualDesktopAdapter`] trait defined here. The Win32 implementation
//! under `platform::windows::desktop` provides a concrete adapter that
//! nails down the trait's contract against the Windows Virtual Desktop
//! COM surface.

use std::fmt;

use crate::core::WorkspaceIndex;
use crate::error::Result;

/// Opaque, stable identifier for a real Windows virtual desktop.
///
/// On Windows these are GUIDs assigned by the immersive shell. We
/// expose them as opaque bytes so that `core` code never sees a
/// platform-specific type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DesktopId(pub [u8; 16]);

impl DesktopId {
    /// Sentinel "unknown" identifier.
    pub const UNKNOWN: Self = Self([0u8; 16]);

    /// Returns `true` if this is the sentinel [`Self::UNKNOWN`].
    #[inline]
    pub fn is_unknown(&self) -> bool {
        self.0 == [0u8; 16]
    }
}

impl fmt::Display for DesktopId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, b) in self.0.iter().enumerate() {
            if i > 0 {
                write!(f, "-")?;
            }
            write!(f, "{:02X}", b)?;
        }
        Ok(())
    }
}

// =====================================================================
// VirtualDesktopAdapter — the trait the platform layer implements.
// =====================================================================

/// Adapter contract that exposes virtual-desktop actions to the core
/// engine.
///
/// All methods are blocking and synchronous. Higher-level async
/// orchestration happens elsewhere; this trait is meant to be easy to
/// wrap in spawn_blocking calls if needed.
pub trait VirtualDesktopAdapter: Send + Sync {
    /// Enumerate the current set of desktops in their natural order.
    /// Index 0 corresponds to `WorkspaceIndex::ONE`, index 1 to TWO,
    /// and so on.
    ///
    /// **Ordering is part of the contract** — the engine assumes that
    /// the first element is "desktop 1", the second is "desktop 2", and
    /// so on.
    fn enumerate(&self) -> Result<Vec<DesktopId>>;

    /// Return the desktop the user is currently looking at.
    fn current(&self) -> Result<DesktopId>;

    /// Switch to the desktop at the given *position* in the enumeration
    /// returned by [`Self::enumerate`]. The caller is responsible for
    /// ensuring `index` is in range.
    fn switch_to(&self, index: WorkspaceIndex) -> Result<()>;

    /// Create a new desktop. The new desktop is appended to the end of
    /// the enumeration.
    fn create(&self) -> Result<DesktopId>;

    /// Move the top-level window with `hwnd` (raw HWND value) to the
    /// desktop at the given *position* in the enumeration.
    fn move_window(&self, hwnd: u64, index: WorkspaceIndex) -> Result<()>;

    /// Query the desktop the window is on.
    fn window_desktop(&self, hwnd: u64) -> Result<DesktopId>;

    /// Current number of desktops. Convenience wrapper.
    fn count(&self) -> Result<usize> {
        Ok(self.enumerate()?.len())
    }

    /// Switch to the desktop that contains the window with `hwnd`.
    fn switch_to_window(&self, hwnd: u64) -> Result<()> {
        let id = self.window_desktop(hwnd)?;
        let desktops = self.enumerate()?;
        let pos = desktops
            .iter()
            .position(|d| d == &id)
            .ok_or_else(|| crate::JacqueError::Other(format!("window {} on unknown desktop {}", hwnd, id)))?;
        let idx = WorkspaceIndex::new((pos + 1) as u8)?;
        self.switch_to(idx)
    }
}
