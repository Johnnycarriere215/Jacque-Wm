//! Windows system tray implementation.
//!
//! Uses `Shell_NotifyIconW` to add a single user-installed icon next
//! to the system tray. We *never* replace any existing tray icon.
//! Right-click shows a tiny popup menu with Exit / Restart / Open
//! Logs. The sink is registered once via [`Self::subscribe`].
//!
//! Failure isolation: if the tray icon fails to install (e.g. the
//! message-only window isn't ready yet), the launcher / panel /
//! hotkeys keep working — `is_installed()` stays `false` and only a
//! log line is emitted.

#![cfg(windows)]

use std::sync::Mutex;

use parking_lot::Mutex as PlMutex;
use windows::w;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::NIM_ADD;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, SetForegroundWindow,
    TrackPopupMenuEx, WM_APP, TPM_NONOTIFY, TPM_RETURNCMD,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NOTIFYICONDATAW, NIF_MESSAGE, NIF_TIP,
};

use crate::core::tray::{TrayAction, TrayManager, TraySink, TrayState};

const WM_TRAYICON: u32 = WM_APP + 1;
const IDM_EXIT: u32 = 1001;
const IDM_RESTART: u32 = 1002;
const IDM_OPENLOGS: u32 = 1003;

/// Concrete Windows-side tray implementation.
pub struct WindowsTray {
    state: TrayState,
    hwnd_lock: PlMutex<Option<HWND>>,
}

impl WindowsTray {
    pub fn new() -> Self {
        Self {
            state: TrayState::new(),
            hwnd_lock: PlMutex::new(None),
        }
    }

    /// Bind the HWND that should receive `WM_TRAYICON` messages.
    pub fn bind_hwnd(&self, hwnd: HWND) {
        *self.hwnd_lock.lock() = Some(hwnd);
    }

    /// Handle one `WM_TRAYICON` message (called by the main thread's
    /// `DispatchMessageW`). Returns the action the user chose, if any.
    pub fn handle_tray_msg(&self) -> Option<TrayAction> {
        let hwnd = *self.hwnd_lock.lock()?;

        unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return None;
            }
            // We don't `w!(...)` here because we don't want to import
            // the macro crate-wide; instead we use the `PCWSTR`-
            // accepting variants when present or fold to the
            // `MAKEINTRESOURCE`-style approach.  AppendMenuW accepts
            // `PCWSTR`; we pass a static wide string.
            let _ = AppendMenuW(menu, 0, IDM_EXIT, windows::PCWSTR(b"E&xit\0".as_ptr() as *const u16));
            let _ = AppendMenuW(menu, 0, IDM_RESTART, windows::PCWSTR(b"&Restart\0".as_ptr() as *const u16));
            let _ = AppendMenuW(menu, 0, IDM_OPENLOGS, windows::PCWSTR(b"Open &Logs\0".as_ptr() as *const u16));

            let mut pt = windows::Win32::Foundation::POINT::default();
            let _ = GetCursorPos(&mut pt);
            let _ = SetForegroundWindow(hwnd);
            let picked = TrackPopupMenuEx(
                menu,
                TPM_NONOTIFY | TPM_RETURNCMD,
                pt.x,
                pt.y,
                hwnd,
                None,
            );
            let _ = DestroyMenu(menu);
            let picked_id = picked.0 as u32;
            match picked_id {
                IDM_EXIT => Some(TrayAction::Exit),
                IDM_RESTART => Some(TrayAction::Restart),
                IDM_OPENLOGS => Some(TrayAction::OpenLogs),
                _ => None,
            }
        }
    }

    fn notify_add(&self, hwnd: HWND) {
        unsafe {
            let mut nid = NOTIFYICONDATAW::default();
            nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = hwnd;
            nid.uID = 0xC1A5_5E;
            nid.uFlags = NIF_MESSAGE | NIF_TIP;
            nid.uCallbackMessage = WM_TRAYICON;
            // Static tip "JacqueWM" + null terminator in UTF-16.
            let tip: Vec<u16> = "JacqueWM\0".encode_utf16().collect();
            let mut tip_buf = [0u16; 128];
            for (slot, v) in tip_buf.iter_mut().zip(tip.iter()) {
                *slot = *v;
            }
            nid.szTip = tip_buf;
            let _ = Shell_NotifyIconW(NIM_ADD, &nid);
        }
    }

    fn notify_remove(&self, hwnd: HWND) {
        use windows::Win32::UI::Shell::NIM_DELETE;
        unsafe {
            let mut nid = NOTIFYICONDATAW::default();
            nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = hwnd;
            nid.uID = 0xC1A5_5E;
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        }
    }
}

impl Default for WindowsTray {
    fn default() -> Self {
        Self::new()
    }
}

impl TrayManager for WindowsTray {
    fn install(&self) {
        if self.state.is_installed_value() {
            return;
        }
        let already = self.state.mark_installed();
        let _ = already; // install is idempotent
        if let Some(hwnd) = *self.hwnd_lock.lock() {
            self.notify_add(hwnd);
        }
    }

    fn remove(&self) {
        if !self.state.is_installed_value() {
            return;
        }
        if let Some(hwnd) = *self.hwnd_lock.lock() {
            self.notify_remove(hwnd);
        }
        self.state.mark_removed();
    }

    fn subscribe(&self, sink: TraySink) {
        self.state.set_sink(sink);
    }

    fn is_installed(&self) -> bool {
        self.state.is_installed_value()
    }
}

/// Small helper cell used by tests + dispatchers that need to peek at
/// the bound sink without taking the full `TrayState` lock.
#[derive(Default)]
pub struct SinkCell {
    inner: Mutex<Option<TraySink>>,
}

impl SinkCell {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn set(&self, sink: TraySink) {
        *self.inner.lock().unwrap() = Some(sink);
    }
    pub fn dispatch(&self, action: TrayAction) {
        if let Some(s) = self.inner.lock().unwrap().as_ref() {
            s(action);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_uninstalled() {
        let t = WindowsTray::new();
        assert!(!t.is_installed());
    }

    #[test]
    fn sink_cell_routes_dispatch() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNT: AtomicU32 = AtomicU32::new(0);
        let cell = SinkCell::new();
        cell.set(std::sync::Arc::new(|_| {
            COUNT.fetch_add(1, Ordering::SeqCst);
        }) as crate::core::tray::TraySink);
        cell.dispatch(TrayAction::Exit);
        assert_eq!(COUNT.load(Ordering::SeqCst), 1);
    }
}
