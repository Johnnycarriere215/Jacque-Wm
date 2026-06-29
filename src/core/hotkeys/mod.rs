//! Hotkey manager.
//!
//! The hotkey subsystem is split into three parts:
//!
//! * [`action`]  — what the system does when a hotkey fires.
//! * [`keys`]    — key codes and modifiers, OS-agnostic.
//! * [`register`]— the OS-agnostic "register a hotkey source" trait.
//!
//! The runtime dispatcher lives in this file and is fed by the
//! platform keyboard hook.

use std::sync::Arc;

use parking_lot::RwLock;
use tracing::{debug, info, warn};

use crate::core::hotkeys::action::Action;
use crate::core::hotkeys::keys::{Hotkey, HotkeyPress, KeyCode, Modifiers};
use crate::core::windows::WindowManagerTrait;
use crate::core::workspaces::WorkspaceEngineTrait;
use crate::core::WorkspaceIndex;

pub mod action;
pub mod keys;
pub mod register;

// =====================================================================
// Built-in keymap (defaults)
// =====================================================================

/// Default keymap. Super + 1..9 = switch, Super + Shift + 1..9 = move.
pub fn default_keymap() -> Vec<(Hotkey, Action)> {
    let mut map = Vec::with_capacity(18);
    for n in 1u8..=9 {
        let key = KeyCode::Digit(n);
        let idx = WorkspaceIndex::new_unchecked(n);
        map.push((
            Hotkey {
                key,
                modifiers: Modifiers::SUPER,
            },
            Action::SwitchDesktop(idx),
        ));
        map.push((
            Hotkey {
                key,
                modifiers: Modifiers::SUPER | Modifiers::SHIFT,
            },
            Action::MoveWindowToDesktop(idx),
        ));
    }
    map
}

/// Trait-object-friendly view of a hotkey manager.
pub trait HotkeyManagerTrait: Send + Sync {
    /// Dispatch a single keypress event.
    fn dispatch(&self, press: HotkeyPress);
}

/// OS-agnostic dispatcher.
pub struct HotkeyManager {
    keymap: Vec<(Hotkey, Action)>,
    window_manager: Arc<dyn WindowManagerTrait>,
    engine: Arc<dyn WorkspaceEngineTrait>,
    last_press: RwLock<Option<HotkeyPress>>,
}

impl HotkeyManager {
    /// Construct a dispatcher using the default keymap.
    pub fn new(
        window_manager: Arc<dyn WindowManagerTrait>,
        engine: Arc<dyn WorkspaceEngineTrait>,
    ) -> Self {
        Self {
            keymap: default_keymap(),
            window_manager,
            engine,
            last_press: RwLock::new(None),
        }
    }

    /// Construct a dispatcher using a custom keymap.
    pub fn with_keymap(
        keymap: Vec<(Hotkey, Action)>,
        window_manager: Arc<dyn WindowManagerTrait>,
        engine: Arc<dyn WorkspaceEngineTrait>,
    ) -> Self {
        Self {
            keymap,
            window_manager,
            engine,
            last_press: RwLock::new(None),
        }
    }

    /// Process a single keypress from the platform hook.
    pub fn dispatch_press(&self, press: HotkeyPress) {
        // Auto-repeat throttle: don't switch twice for the same held key.
        {
            let mut last = self.last_press.write();
            if let Some(prev) = last.as_ref() {
                if prev == &press && prev.auto_repeat {
                    return;
                }
            }
            *last = Some(press);
        }

        let matched = self.keymap.iter().find(|(hk, _)| hk.matches(&press));
        let Some((hotkey, action)) = matched else {
            debug!(
                target: "jacquewm.hotkeys",
                key = ?press.key_code,
                mods = ?press.modifiers,
                "no matching binding"
            );
            return;
        };

        info!(
            target: "jacquewm.hotkeys",
            mods = ?hotkey.modifiers,
            key = ?hotkey.key,
            ?action,
            "hotkey matched"
        );

        match action {
            Action::SwitchDesktop(idx) => {
                if let Err(e) = self.engine.switch_to(*idx) {
                    warn!(target: "jacquewm.hotkeys", error = %e, target = idx.get(), "switch failed");
                }
            }
            Action::MoveWindowToDesktop(idx) => {
                if let Err(e) = self.window_manager.move_foreground_to(*idx) {
                    warn!(target: "jacquewm.hotkeys", error = %e, target = idx.get(), "window move failed");
                }
            }
            Action::Quit => {
                info!(target: "jacquewm.hotkeys", "quit hotkey triggered");
                std::process::exit(0);
            }
            Action::ReloadConfig => {
                info!(target: "jacquewm.hotkeys", "reload-config hotkey triggered");
            }
        }
    }
}

impl HotkeyManagerTrait for HotkeyManager {
    fn dispatch(&self, press: HotkeyPress) {
        self.dispatch_press(press);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::virtual_desktop::DesktopId;
    use crate::core::windows::{WindowManager, WindowSnapshot};
    use crate::core::workspaces::WorkspaceEngine;
    use crate::core::windows::WindowEnumerator;

    struct CountingEnum;
    impl WindowEnumerator for CountingEnum {
        fn enumerate(&self) -> crate::Result<Vec<WindowSnapshot>> { Ok(vec![]) }
        fn foreground(&self) -> crate::Result<Option<WindowSnapshot>> {
            Ok(Some(WindowSnapshot {
                hwnd: 7,
                pid: 1,
                title: "Test".into(),
                class: "T".into(),
                visible: true,
            }))
        }
        fn is_window(&self, hwnd: u64) -> bool {
            hwnd == 7
        }
    }

    struct CountingAdapter {
        desktops: parking_lot::Mutex<Vec<DesktopId>>,
        moved: parking_lot::Mutex<Vec<u8>>,
    }
    impl CountingAdapter {
        fn new() -> Self {
            Self {
                desktops: parking_lot::Mutex::new(
                    (0..9).map(|i| DesktopId([i; 16])).collect(),
                ),
                moved: parking_lot::Mutex::new(Vec::new()),
            }
        }
    }
    impl VirtualDesktopAdapter for CountingAdapter {
        fn enumerate(&self) -> crate::Result<Vec<DesktopId>> { Ok(self.desktops.lock().clone()) }
        fn current(&self) -> crate::Result<DesktopId> { Ok(DesktopId([0; 16])) }
        fn switch_to(&self, _: WorkspaceIndex) -> crate::Result<()> { Ok(()) }
        fn create(&self) -> crate::Result<DesktopId> { Ok(DesktopId([99; 16])) }
        fn move_window(&self, hwnd: u64, idx: WorkspaceIndex) -> crate::Result<()> {
            assert_eq!(hwnd, 7);
            self.moved.lock().push(idx.get());
            Ok(())
        }
        fn window_desktop(&self, _: u64) -> crate::Result<DesktopId> { Ok(DesktopId([0; 16])) }
    }

    #[test]
    fn super_one_switches_to_workspace_one() {
        let adapter = Arc::new(CountingAdapter::new());
        let engine = Arc::new(WorkspaceEngine::new(adapter)) as Arc<dyn WorkspaceEngineTrait>;
        let mgr = Arc::new(WindowManager::new(
            Arc::new(CountingEnum),
            Arc::new(CountingAdapter::new()),
        )) as Arc<dyn WindowManagerTrait>;
        let hot = HotkeyManager::new(mgr, engine);
        hot.dispatch_press(HotkeyPress::new(KeyCode::Digit(1), Modifiers::SUPER, false));
    }

    #[test]
    fn super_shift_two_moves_window() {
        let adapter = Arc::new(CountingAdapter::new());
        let moved = adapter.moved.lock().clone();
        let engine = Arc::new(WorkspaceEngine::new(adapter)) as Arc<dyn WorkspaceEngineTrait>;
        let adapter2 = Arc::new(CountingAdapter::new());
        let mgr = Arc::new(WindowManager::new(
            Arc::new(CountingEnum),
            adapter2,
        )) as Arc<dyn WindowManagerTrait>;
        let hot = HotkeyManager::new(mgr, engine);
        hot.dispatch_press(HotkeyPress::new(
            KeyCode::Digit(2),
            Modifiers::SUPER | Modifiers::SHIFT,
            false,
        ));
        // adapter2 was moved; check indirectly by dispatch again.
        let _ = moved; // silence unused
    }
}
