//! Window manager.
//!
//! Knows how to enumerate the windows the user sees, identify the
//! foreground (focused) window, and ask the virtual-desktop adapter to
//! move a window between desktops. Knows nothing about Win32 — depends
//! only on the [`VirtualDesktopAdapter`] trait plus an injected
//! [`WindowEnumerator`] trait.

use std::sync::Arc;

use parking_lot::RwLock;
use tracing::{debug, info};

use crate::core::virtual_desktop::{DesktopId, VirtualDesktopAdapter};
use crate::core::WorkspaceIndex;
use crate::error::{JacqueError, Result};

/// A light-weight representation of a top-level window.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WindowSnapshot {
    /// Raw HWND value (Windows-specific field name kept for clarity).
    pub hwnd: u64,
    /// Process id of the owner.
    pub pid: u32,
    /// Window title (truncated to a sane length so logs stay readable).
    pub title: String,
    /// Window class name (e.g. "CabinetWClass", "Notepad").
    pub class: String,
    /// `true` if the window is currently visible.
    pub visible: bool,
}

/// OS-agnostic enumeration contract.
pub trait WindowEnumerator: Send + Sync {
    /// Enumerate the windows according to the policy used by the
    /// concrete implementation. Implementations are expected to skip
    /// system / invisible / tool windows.
    fn enumerate(&self) -> Result<Vec<WindowSnapshot>>;

    /// Return the foreground top-level window, or `None` if there is
    /// no foreground.
    fn foreground(&self) -> Result<Option<WindowSnapshot>>;

    /// Returns `true` if the window with `hwnd` exists and is visible.
    fn is_window(&self, hwnd: u64) -> bool;
}

/// Trait-object-friendly re-export used by sibling subsystems.
pub trait WindowManagerTrait: Send + Sync {
    /// Move the foreground window to the given workspace index.
    fn move_foreground_to(&self, idx: WorkspaceIndex) -> Result<()>;
}

/// Default window manager used by the rest of JacqueWM.
pub struct WindowManager {
    enumerator: Arc<dyn WindowEnumerator>,
    adapter: Arc<dyn VirtualDesktopAdapter>,
    foreground_cache: RwLock<Option<WindowSnapshot>>,
}

impl WindowManager {
    /// Construct a manager backed by the given sources.
    pub fn new(
        enumerator: Arc<dyn WindowEnumerator>,
        adapter: Arc<dyn VirtualDesktopAdapter>,
    ) -> Self {
        Self {
            enumerator,
            adapter,
            foreground_cache: RwLock::new(None),
        }
    }

    /// List all visible user windows.
    pub fn list_windows(&self) -> Result<Vec<WindowSnapshot>> {
        Ok(self.enumerator.enumerate()?)
    }

    /// Return the foreground window, caching the result so that rapid
    /// hotkey presses do not produce flicker in the logs.
    pub fn foreground(&self) -> Result<Option<WindowSnapshot>> {
        let cached = self.foreground_cache.read().clone();
        if let Some(ref cached) = cached {
            if self.enumerator.is_window(cached.hwnd) {
                return Ok(Some(cached.clone()));
            }
        }
        let live = self.enumerator.foreground()?;
        if let Some(ref f) = live {
            *self.foreground_cache.write() = Some(f.clone());
        }
        Ok(live)
    }

    /// Move the foreground window to the given workspace. The caller
    /// decides whether to follow the window (see `Config::follow_moved_windows`).
    pub fn move_foreground_to(&self, index: WorkspaceIndex) -> Result<()> {
        let fg = self
            .foreground()?
            .ok_or_else(|| JacqueError::WindowMove {
                hwnd: 0,
                index: index.get(),
                reason: "no foreground window".into(),
            })?;
        info!(
            target: "jacquewm.windows",
            hwnd = fg.hwnd,
            title = %fg.title,
            target = index.get(),
            "moving focused window to workspace"
        );
        self.adapter.move_window(fg.hwnd, index).map_err(|e| {
            JacqueError::WindowMove {
                hwnd: fg.hwnd,
                index: index.get(),
                reason: format!("{e}"),
            }
        })?;
        let id = self.adapter.window_desktop(fg.hwnd).unwrap_or(DesktopId::UNKNOWN);
        let live = self.adapter.enumerate()?;
        if let Some(pos) = live.iter().position(|d| *d == id) {
            let _ = WorkspaceIndex::new((pos + 1) as u8).map(|idx| {
                if idx != index {
                    tracing::warn!(
                        target: "jacquewm.windows",
                        expected = index.get(),
                        actual = idx.get(),
                        "window placed on unexpected desktop"
                    );
                }
            });
        }
        debug!(
            target: "jacquewm.windows",
            hwnd = fg.hwnd,
            target = index.get(),
            "window move completed"
        );
        Ok(())
    }
}

impl WindowManagerTrait for WindowManager {
    fn move_foreground_to(&self, idx: WorkspaceIndex) -> Result<()> {
        WindowManager::move_foreground_to(self, idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::virtual_desktop::DesktopId;

    struct MockEnum;
    impl WindowEnumerator for MockEnum {
        fn enumerate(&self) -> Result<Vec<WindowSnapshot>> {
            Ok(vec![])
        }
        fn foreground(&self) -> Result<Option<WindowSnapshot>> {
            Ok(Some(WindowSnapshot {
                hwnd: 42,
                pid: 100,
                title: "Notepad".into(),
                class: "Notepad".into(),
                visible: true,
            }))
        }
        fn is_window(&self, hwnd: u64) -> bool {
            hwnd == 42
        }
    }

    struct MockAdapter;
    impl VirtualDesktopAdapter for MockAdapter {
        fn enumerate(&self) -> Result<Vec<DesktopId>> {
            Ok((0..9).map(|i| DesktopId([i as u8; 16])).collect())
        }
        fn current(&self) -> Result<DesktopId> {
            Ok(DesktopId([0u8; 16]))
        }
        fn switch_to(&self, _: WorkspaceIndex) -> Result<()> {
            Ok(())
        }
        fn create(&self) -> Result<DesktopId> {
            Ok(DesktopId([99u8; 16]))
        }
        fn move_window(&self, hwnd: u64, idx: WorkspaceIndex) -> Result<()> {
            assert_eq!(hwnd, 42);
            assert!(
                idx.get() == 3 || idx.get() == 4,
                "idx was {}",
                idx.get()
            );
            Ok(())
        }
        fn window_desktop(&self, _: u64) -> Result<DesktopId> {
            Ok(DesktopId([0u8; 16]))
        }
    }

    #[test]
    fn move_foreground_uses_cached_hwnd() {
        let mgr = WindowManager::new(Arc::new(MockEnum), Arc::new(MockAdapter));
        mgr.move_foreground_to(WorkspaceIndex::new_unchecked(3)).unwrap();
        // Second call should also succeed thanks to cache.
        mgr.move_foreground_to(WorkspaceIndex::new_unchecked(4)).unwrap();
    }
}
