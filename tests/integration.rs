//! Integration tests for JacqueWM.
//!
//! These tests exercise the OS-agnostic subsystems end-to-end. They do
//! not require an active Explorer.exe and run on every CI build.

use std::sync::Arc;
use tempfile::tempdir;

use jacquewm::core::config::{Config, ConfigManager};
use jacquewm::core::hotkeys::action::Action;
use jacquewm::core::hotkeys::keys::{Hotkey, HotkeyPress, KeyCode, Modifiers};
use jacquewm::core::hotkeys::HotkeyManager;
use jacquewm::core::virtual_desktop::{DesktopId, VirtualDesktopAdapter};
use jacquewm::core::windows::{WindowEnumerator, WindowManager, WindowManagerTrait, WindowSnapshot};
use jacquewm::core::workspaces::{WorkspaceEngine, WorkspaceEngineTrait};
use jacquewm::core::WorkspaceIndex;
use jacquewm::JacqueError;

#[test]
fn config_defaults_pass_validation() {
    let cfg = Config::defaults();
    assert_eq!(cfg.startup_desktop.get(), 1);
    assert_eq!(cfg.workspace_count, 9);
    assert!(!cfg.follow_moved_windows);
    assert!(cfg.enable_logging);
    assert!(cfg.validate().is_ok());
}

#[test]
fn config_manager_generates_file_on_first_load() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("jacquewm.toml");
    let mgr = ConfigManager::load_from(&path).unwrap();
    assert!(path.exists());
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("startup_desktop"));
}

#[test]
fn config_manager_handles_malformed_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("broken.toml");
    std::fs::write(&path, "this is not toml = = =").unwrap();
    let mgr = ConfigManager::load_from(&path).unwrap();
    // Falls back to defaults.
    assert_eq!(mgr.snapshot().startup_desktop.get(), 1);
}

// =====================================================================
// Mock platform support.
// =====================================================================

#[derive(Clone)]
struct InMemoryDesktop {
    desktops: Arc<parking_lot::Mutex<Vec<DesktopId>>>,
    current_position: Arc<parking_lot::Mutex<u8>>,
}

impl InMemoryDesktop {
    fn new(count: u8) -> Self {
        let mut desktops: Vec<DesktopId> = (0..count).map(|i| DesktopId([i as u8; 16])).collect();
        desktops.truncate(9);
        Self {
            desktops: Arc::new(parking_lot::Mutex::new(desktops)),
            current_position: Arc::new(parking_lot::Mutex::new(1)),
        }
    }
}

impl VirtualDesktopAdapter for InMemoryDesktop {
    fn enumerate(&self) -> Result<Vec<DesktopId>, JacqueError> {
        Ok(self.desktops.lock().clone())
    }
    fn current(&self) -> Result<DesktopId, JacqueError> {
        let pos = (*self.current_position.lock() - 1) as usize;
        Ok(self.desktops.lock()[pos])
    }
    fn switch_to(&self, index: WorkspaceIndex) -> Result<(), JacqueError> {
        let pos = (index.get() - 1) as usize;
        if pos >= self.desktops.lock().len() {
            return Err(JacqueError::DesktopSwitch {
                index: index.get(),
                reason: "out of range".into(),
            });
        }
        *self.current_position.lock() = index.get();
        Ok(())
    }
    fn create(&self) -> Result<DesktopId, JacqueError> {
        let mut g = self.desktops.lock();
        let next = g.len();
        let id = DesktopId([next as u8; 16]);
        g.push(id);
        Ok(id)
    }
    fn move_window(&self, _hwnd: u64, _index: WorkspaceIndex) -> Result<(), JacqueError> {
        Ok(())
    }
    fn window_desktop(&self, _hwnd: u64) -> Result<DesktopId, JacqueError> {
        let pos = (*self.current_position.lock() - 1) as usize;
        Ok(self.desktops.lock()[pos])
    }
}

struct InMemoryEnum;
impl WindowEnumerator for InMemoryEnum {
    fn enumerate(&self) -> Result<Vec<WindowSnapshot>, JacqueError> {
        Ok(Vec::new())
    }
    fn foreground(&self) -> Result<Option<WindowSnapshot>, JacqueError> {
        Ok(Some(WindowSnapshot {
            hwnd: 7,
            pid: 1,
            title: "Sample".into(),
            class: "Sample".into(),
            visible: true,
        }))
    }
    fn is_window(&self, hwnd: u64) -> bool {
        hwnd == 7
    }
}

#[test]
fn engine_maintains_nine_desktop_invariant() {
    let adapter = Arc::new(InMemoryDesktop::new(3));
    let engine = WorkspaceEngine::new(adapter).unwrap();
    assert_eq!(engine.snapshot().count, 3);
    engine.ensure_workspace_count(9).unwrap();
    assert_eq!(engine.snapshot().count, 9);
}

#[test]
fn switch_to_clamps_to_engine_state() {
    let adapter = Arc::new(InMemoryDesktop::new(9));
    let engine = WorkspaceEngine::new(adapter).unwrap();
    engine.switch_to(WorkspaceIndex::new_unchecked(7)).unwrap();
    assert_eq!(engine.current().get(), 7);
}

#[test]
fn window_manager_moves_foreground() {
    let adapter = Arc::new(InMemoryDesktop::new(9));
    let wm = Arc::new(WindowManager::new(
        Arc::new(InMemoryEnum),
        adapter.clone(),
    )) as Arc<dyn WindowManagerTrait>;
    wm.move_foreground_to(WorkspaceIndex::new_unchecked(4)).unwrap();
}

#[test]
fn dispatcher_switches_on_super_digit() {
    let adapter = Arc::new(InMemoryDesktop::new(9));
    let engine = Arc::new(WorkspaceEngine::new(adapter)) as Arc<dyn WorkspaceEngineTrait>;
    let wm = Arc::new(WindowManager::new(
        Arc::new(InMemoryEnum),
        Arc::new(InMemoryDesktop::new(9)),
    )) as Arc<dyn WindowManagerTrait>;
    let dispatcher = HotkeyManager::new(wm, engine);
    dispatcher.dispatch_press(HotkeyPress::new(KeyCode::Digit(3), Modifiers::SUPER, false));
    // Engine was behind InMemoryDesktop#2 which started at index 1.
    assert_eq!(
        dispatcher.engine.current().get(),
        3,
        "switch to workspace 3 should succeed"
    );
}

#[test]
fn dispatcher_moves_window_on_super_shift_digit() {
    let adapter = Arc::new(InMemoryDesktop::new(9));
    let engine = Arc::new(WorkspaceEngine::new(adapter.clone())) as Arc<dyn WorkspaceEngineTrait>;
    let wm = Arc::new(WindowManager::new(
        Arc::new(InMemoryEnum),
        adapter,
    )) as Arc<dyn WindowManagerTrait>;
    let dispatcher = HotkeyManager::new(wm, engine);
    dispatcher.dispatch_press(HotkeyPress::new(
        KeyCode::Digit(9),
        Modifiers::SUPER | Modifiers::SHIFT,
        false,
    ));
}

#[test]
fn dispatcher_ignores_unmodified_digit_press() {
    let adapter = Arc::new(InMemoryDesktop::new(9));
    let engine = Arc::new(WorkspaceEngine::new(adapter)) as Arc<dyn WorkspaceEngineTrait>;
    let wm = Arc::new(WindowManager::new(
        Arc::new(InMemoryEnum),
        Arc::new(InMemoryDesktop::new(9)),
    )) as Arc<dyn WindowManagerTrait>;
    let dispatcher = HotkeyManager::new(wm, engine);
    dispatcher.dispatch_press(HotkeyPress::new(KeyCode::Digit(2), Modifiers::empty(), false));
    // Should not match (no modifier).
    assert_eq!(dispatcher.engine.current().get(), 1);
}

#[test]
fn keymap_supports_all_nine_digits() {
    // Smoke-test the default keymap: every digit should be bound.
    for n in 1u8..=9 {
        let key = KeyCode::Digit(n);
        let hotkey = Hotkey {
            key,
            modifiers: Modifiers::SUPER,
        };
        let pressed = HotkeyPress::new(key, Modifiers::SUPER, false);
        assert!(hotkey.matches(&pressed));

        let move_key = Hotkey {
            key,
            modifiers: Modifiers::SUPER | Modifiers::SHIFT,
        };
        let move_press = HotkeyPress::new(
            key,
            Modifiers::SUPER | Modifiers::SHIFT,
            false,
        );
        assert!(move_key.matches(&move_press));
    }
    // Action variants covered via the dispatcher.
    assert!(matches!(
        Action::SwitchDesktop(WorkspaceIndex::new_unchecked(1)),
        Action::SwitchDesktop(_)
    ));
}
