//! [`Action`] enum — the operations the hotkey dispatcher can trigger.

use crate::core::WorkspaceIndex;

/// What a hotkey should do when triggered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    /// Switch to the given 1-based workspace index.
    SwitchDesktop(WorkspaceIndex),

    /// Move the focused window to the given workspace.
    MoveWindowToDesktop(WorkspaceIndex),

    /// Quit JacqueWM.
    Quit,

    /// Hot-reload configuration from disk.
    ReloadConfig,
}
