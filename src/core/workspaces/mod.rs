//! Workspace engine.
//!
//! The engine owns the invariant that *exactly nine* desktops exist
//! (configurable down to a smaller number, but never below one and
//! never above nine). It also owns the "where am I right now?" pointer
//! that other subsystems consult.
//!
//! The engine is purely logical — it knows nothing about Win32. It
//! delegates every concrete desktop operation to the
//! [`VirtualDesktopAdapter`] injected at construction time.

use std::sync::Arc;

use parking_lot::RwLock;
use tracing::{debug, info, warn};

use crate::core::virtual_desktop::{DesktopId, VirtualDesktopAdapter};
use crate::core::WorkspaceIndex;
use crate::error::{JacqueError, Result};

/// Logical snapshot of the workspace state.
#[derive(Debug, Clone)]
pub struct WorkspaceState {
    /// The desktop the user is currently on (1..=9).
    pub current: WorkspaceIndex,
    /// All desktops, ordered as the underlying OS sees them.
    pub desktops: Vec<DesktopId>,
    /// Total count (== `desktops.len()` but kept for convenience).
    pub count: u8,
}

impl WorkspaceState {
    /// Returns `true` if the engine has detected the expected number of
    /// workspaces on the underlying OS.
    pub fn is_complete(&self) -> bool {
        self.count as usize == self.desktops.len()
            && self.desktops.len() == WorkspaceIndex::COUNT as usize
    }
}

/// The workspace engine itself.
pub struct WorkspaceEngine {
    adapter: Arc<dyn VirtualDesktopAdapter>,
    state: RwLock<WorkspaceState>,
}

impl WorkspaceEngine {
    /// Construct a new engine and synchronise its state with the OS.
    ///
    /// Calls [`VirtualDesktopAdapter::enumerate`] and stores the result.
    /// The engine does *not* create or delete desktops during
    /// construction — call [`Self::ensure_workspace_count`] for that.
    pub fn new(adapter: Arc<dyn VirtualDesktopAdapter>) -> Result<Self> {
        let desktops = adapter.enumerate()?;
        let count = desktops.len() as u8;
        let current = adapter.current()?;
        let current_index = desktops
            .iter()
            .position(|d| *d == current)
            .map(|p| (p + 1) as u8)
            .unwrap_or(1);
        let state = WorkspaceState {
            current: WorkspaceIndex::new(current_index).unwrap_or(WorkspaceIndex::new_unchecked(1)),
            desktops,
            count,
        };
        Ok(Self {
            adapter,
            state: RwLock::new(state),
        })
    }

    /// Returns the current logical workspace index.
    pub fn current(&self) -> WorkspaceIndex {
        self.state.read().current
    }

    /// Returns a snapshot of the workspace state.
    pub fn snapshot(&self) -> WorkspaceState {
        self.state.read().clone()
    }

    /// Returns the adapter for use by other subsystems (window manager,
    /// hotkey dispatcher, etc.).
    pub fn adapter(&self) -> Arc<dyn VirtualDesktopAdapter> {
        self.adapter.clone()
    }

    /// Synchronise the in-memory state with the OS.
    pub fn refresh(&self) -> Result<()> {
        let desktops = self.adapter.enumerate()?;
        let current = self.adapter.current()?;
        let pos = desktops
            .iter()
            .position(|d| *d == current)
            .map(|p| (p + 1) as u8)
            .unwrap_or(1);
        let mut guard = self.state.write();
        guard.desktops = desktops;
        guard.count = guard.desktops.len() as u8;
        guard.current = WorkspaceIndex::new(pos).unwrap_or(WorkspaceIndex::new_unchecked(1));
        debug!(
            target: "jacquewm.engine",
            current = ?guard.current,
            count = guard.count,
            "engine state refreshed"
        );
        Ok(())
    }

    /// Ensure the OS has at least `target_count` desktops, creating any
    /// missing ones. Returns the new count.
    ///
    /// JacqueWM semantics dictate *exactly nine* desktops, so the
    /// canonical call is `engine.ensure_workspace_count(9)`.
    pub fn ensure_workspace_count(&self, target_count: u8) -> Result<u8> {
        if target_count == 0 || target_count > WorkspaceIndex::COUNT {
            return Err(JacqueError::InvalidWorkspaceIndex(target_count));
        }
        let mut created = 0u8;
        loop {
            let current = self.adapter.enumerate()?.len() as u8;
            if current >= target_count {
                break;
            }
            self.adapter.create().map_err(|e| {
                warn!(
                    target: "jacquewm.engine",
                    error = %e,
                    "desktop creation failed; aborting bootstrap"
                );
                e
            })?;
            created += 1;
            info!(
                target: "jacquewm.engine",
                created = created,
                target = target_count,
                "created missing desktop"
            );
        }
        self.refresh()?;
        Ok(self.snapshot().count)
    }

    /// Switch to the given workspace. Validates that the destination
    /// exists, calls the adapter, refreshes state on success.
    pub fn switch_to(&self, index: WorkspaceIndex) -> Result<()> {
        let snapshot = self.snapshot();
        let pos = (index.get() - 1) as usize;
        if pos >= snapshot.desktops.len() {
            return Err(JacqueError::DesktopSwitch {
                index: index.get(),
                reason: format!(
                    "engine only knows {} desktops; index {} is out of range",
                    snapshot.desktops.len(),
                    index.get()
                ),
            });
        }
        self.adapter.switch_to(index).map_err(|e| {
            JacqueError::DesktopSwitch {
                index: index.get(),
                reason: format!("{e}"),
            }
        })?;
        self.refresh()?;
        info!(
            target: "jacquewm.engine",
            target = index.get(),
            "switched workspace"
        );
        Ok(())
    }
}

// =====================================================================
// Trait object used by sibling subsystems.
// =====================================================================

/// Object-safe trait used by the hotkey dispatcher and the window
/// manager. The concrete [`WorkspaceEngine`] implements it via the
/// blanket `impl` below.
pub trait WorkspaceEngineTrait: Send + Sync {
    /// Switch to the given workspace.
    fn switch_to(&self, index: WorkspaceIndex) -> Result<()>;
    /// Returns the currently-active workspace.
    fn current(&self) -> WorkspaceIndex;
}

impl WorkspaceEngineTrait for WorkspaceEngine {
    fn switch_to(&self, index: WorkspaceIndex) -> Result<()> {
        WorkspaceEngine::switch_to(self, index)
    }
    fn current(&self) -> WorkspaceIndex {
        WorkspaceEngine::current(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::virtual_desktop::DesktopId;

    /// Adapter used by the engine tests. It does not touch Win32 — it
    /// keeps an in-memory model of desktops and exposes the same API
    /// the real adapter does.
    struct MockAdapter {
        desktops: parking_lot::Mutex<Vec<DesktopId>>,
        current_idx: parking_lot::Mutex<u8>,
    }

    impl MockAdapter {
        fn new(initial: u8) -> Self {
            let mut desktops: Vec<DesktopId> = (0..initial)
                .map(|i| DesktopId([i as u8; 16]))
                .collect();
            desktops.truncate(9);
            Self {
                desktops: parking_lot::Mutex::new(desktops),
                current_idx: parking_lot::Mutex::new(1),
            }
        }
    }

    impl VirtualDesktopAdapter for MockAdapter {
        fn enumerate(&self) -> crate::Result<Vec<DesktopId>> {
            Ok(self.desktops.lock().clone())
        }
        fn current(&self) -> crate::Result<DesktopId> {
            let idx = *self.current_idx.lock() as usize;
            Ok(self.desktops.lock()[idx - 1])
        }
        fn switch_to(&self, index: WorkspaceIndex) -> crate::Result<()> {
            let pos = (index.get() - 1) as usize;
            if pos >= self.desktops.lock().len() {
                return Err(JacqueError::DesktopSwitch {
                    index: index.get(),
                    reason: "out of range".into(),
                });
            }
            *self.current_idx.lock() = index.get();
            Ok(())
        }
        fn create(&self) -> crate::Result<DesktopId> {
            let mut g = self.desktops.lock();
            let next = g.len();
            let id = DesktopId([next as u8; 16]);
            g.push(id);
            Ok(id)
        }
        fn move_window(&self, _hwnd: u64, _index: WorkspaceIndex) -> crate::Result<()> {
            Ok(())
        }
        fn window_desktop(&self, _hwnd: u64) -> crate::Result<DesktopId> {
            Ok(*self.current.lock())
        }
    }

    #[test]
    fn engine_creates_missing_desktops() {
        let adapter = Arc::new(MockAdapter::new(3));
        let engine = WorkspaceEngine::new(adapter).unwrap();
        assert_eq!(engine.snapshot().count, 3);
        engine.ensure_workspace_count(9).unwrap();
        assert_eq!(engine.snapshot().count, 9);
    }

    #[test]
    fn engine_refuses_invalid_target() {
        let adapter = Arc::new(MockAdapter::new(5));
        let engine = WorkspaceEngine::new(adapter).unwrap();
        assert!(engine.ensure_workspace_count(0).is_err());
        assert!(engine.ensure_workspace_count(10).is_err());
    }

    #[test]
    fn switch_to_updates_state() {
        let adapter = Arc::new(MockAdapter::new(9));
        let engine = WorkspaceEngine::new(adapter).unwrap();
        engine
            .switch_to(WorkspaceIndex::new_unchecked(5))
            .unwrap();
        assert_eq!(engine.current().get(), 5);
    }
}
