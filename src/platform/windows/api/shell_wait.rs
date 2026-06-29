//! Wait for Windows Explorer to finish initialising.
//!
//! After login, Explorer.exe and the immersive shell come up *after*
//! the user's shell auto-start triggers JacqueWM. The virtual desktop
//! subsystem lives inside the immersive shell, so calling into it
//! before it is ready produces `RPC_E_DISCONNECTED` or
//! `CO_E_OBJNOTCONNECTED`.
//!
//! The simplest, most reliable check is to look for the class window
//! `"Shell_TrayWnd"` — the notification area's parent. When it exists,
//! the immersive shell has begun accepting service queries.
//!
//! Strategy:
//! * Poll `FindWindowW("Shell_TrayWnd", null)` every 500 ms.
//! * Bound the wait to `timeout_ms` (default 30 s).
//! * Also re-check at runtime if we receive a `TaskbarCreated`
//!   broadcast.

use std::time::{Duration, Instant};

use windows::core::w;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;

use crate::error::{JacqueError, Result};

/// Returns `true` if Explorer's tray window exists.
pub fn explorer_tray_exists() -> bool {
    unsafe {
        let hwnd = FindWindowW(w!("Shell_TrayWnd"), None);
        hwnd.is_ok() && !hwnd.unwrap().0.is_null()
    }
}

/// Wait for Explorer's tray window to appear, polling every 500 ms.
///
/// # Errors
///
/// Returns [`JacqueError::ExplorerNotReady`] if the timeout expires
/// without Explorer appearing. The system continues running in that
/// case — desktop operations will likely fail, but the user is left
/// with a visible diagnostic rather than a silent crash.
pub fn wait_for_explorer(timeout: Option<Duration>) -> Result<()> {
    let deadline = timeout.unwrap_or(Duration::from_secs(30));
    let start = Instant::now();
    while !explorer_tray_exists() {
        if start.elapsed() >= deadline {
            return Err(JacqueError::ExplorerNotReady(Some(deadline.as_secs())));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Ok(())
}

/// Try once to query `HWND` of the tray, returning `None` if it's not
/// available. Used after a `TaskbarCreated` event to schedule a retry.
pub fn tray_hwnd() -> Option<HWND> {
    unsafe {
        FindWindowW(w!("Shell_TrayWnd"), None).ok()
    }
}
