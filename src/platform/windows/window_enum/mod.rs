//! Windows window enumerator.
//!
//! Uses [`EnumWindows`] to walk every top-level window on the desktop
//! and assembles a [`WindowSnapshot`] for each one that passes our
//! visibility / system-window filter.
//!
//! The enumerator is allocated in the user's process and uses no
//! shared state, so it can be cloned freely via `Arc`.

use std::sync::Arc;

use tracing::trace;
use windows::core::w;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindow, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsWindowVisible, GWL_EXSTYLE, GW_OWNER,
};

use crate::core::windows::{WindowEnumerator, WindowSnapshot};
use crate::error::{JacqueError, Result};

/// Windows-backed window enumerator.
///
/// Stateless — every call re-walks the window list. Performance is
/// adequate because JacqueWM only calls it on hotkey events.
pub struct WindowsWindowEnumerator;

impl WindowsWindowEnumerator {
    /// Construct a new enumerator.
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsWindowEnumerator {
    fn default() -> Self {
        Self::new()
    }
}

// =====================================================================
// EnumWindows callback implementation.
// =====================================================================
//
// EnumWindows passes a raw pointer to an enum-context struct; we wrap
// that pointer carefully: the lifetime is bounded by the call to
// `EnumWindows`, so we use a scoped `Box` to store the destination.

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // Skip invisible & owner-owned windows.
    if !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1);
    }
    let owner = GetWindow(hwnd, GW_OWNER);
    if !owner.is_err() && !owner.unwrap().0.is_null() {
        return BOOL(1);
    }
    // Skip tool / layered / no-activate windows — they look invisible
    // to the user.
    use windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW;
    let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    const WS_EX_TOOLWINDOW: isize = 0x00000080;
    const WS_EX_NOACTIVATE: isize = 0x08000000;
    const WS_EX_APPWINDOW: isize = 0x00040000;
    if (ex_style & WS_EX_TOOLWINDOW) != 0 && (ex_style & WS_EX_APPWINDOW) == 0 {
        return BOOL(1);
    }
    if (ex_style & WS_EX_NOACTIVATE) != 0 && (ex_style & WS_EX_APPWINDOW) == 0 {
        return BOOL(1);
    }

    let snapshot = make_snapshot(hwnd);
    let ctx = &mut *(lparam.0 as *mut EnumContext);
    ctx.results.push(snapshot);
    BOOL(1) // continue enumeration
}

struct EnumContext {
    results: Vec<WindowSnapshot>,
}

fn make_snapshot(hwnd: HWND) -> WindowSnapshot {
    unsafe {
        let title = read_window_text(hwnd);
        let class = read_window_class(hwnd);
        let mut pid = 0u32;
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
        WindowSnapshot {
            hwnd: hwnd.0 as u64,
            pid,
            title,
            class,
            visible: true,
        }
    }
}

unsafe fn read_window_text(hwnd: HWND) -> String {
    let length = GetWindowTextLengthW(hwnd);
    if length == 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; length as usize + 1];
    let actual = GetWindowTextW(hwnd, &mut buffer);
    if actual == 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..actual as usize])
}

unsafe fn read_window_class(hwnd: HWND) -> String {
    let mut buffer = [0u16; 256];
    let actual = GetClassNameW(hwnd, &mut buffer);
    if actual == 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..actual as usize])
}

impl WindowEnumerator for WindowsWindowEnumerator {
    fn enumerate(&self) -> Result<Vec<WindowSnapshot>> {
        unsafe {
            let mut ctx = EnumContext { results: Vec::new() };
            let _ = EnumWindows(Some(enum_proc), LPARAM(&mut ctx as *mut _ as isize))
                .map_err(|e| JacqueError::WindowEnumeration(format!("EnumWindows failed: {:?}", e)))?;
            trace!(
                target: "jacquewm.windows",
                count = ctx.results.len(),
                "window enumeration complete"
            );
            Ok(ctx.results)
        }
    }

    fn foreground(&self) -> Result<Option<WindowSnapshot>> {
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() {
                return Ok(None);
            }
            Ok(Some(make_snapshot(hwnd)))
        }
    }

    fn is_window(&self, hwnd: u64) -> bool {
        unsafe {
            let hwnd = HWND(hwnd as *mut std::ffi::c_void);
            let _ = w!("");
            windows::Win32::UI::WindowsAndMessaging::IsWindow(hwnd).as_bool()
        }
    }
}

// SAFETY: stateless, safe to send across threads.
unsafe impl Send for WindowsWindowEnumerator {}
unsafe impl Sync for WindowsWindowEnumerator {}
