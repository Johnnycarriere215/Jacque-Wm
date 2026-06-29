//! Hidden-window + thread-message helpers.
//!
//! JacqueWM registers a single message-only window (`HWND_MESSAGE`)
//! on the main thread. Other components post custom messages to this
//! window to ask the main thread to perform COM calls, to deliver
//! custom hotkey events, or to react to Explorer restarts.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicIsize, Ordering};

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, RegisterWindowMessageW, DestroyWindow, CW_USEDEFAULT, HWND_MESSAGE,
    WNDCLASSEXW, WS_EX_NOACTIVATE,
};

use crate::error::{JacqueError, Result};

/// `WM_USER`-derived custom messages used internally. Starts at 0x0400.
pub mod msg {
    use windows::Win32::Foundation::{LPARAM, WPARAM};

    /// Posted by the keyboard hook to give the dispatcher a chance to
    /// drain queued events. The LPARAM is unused.
    pub const WM_DRAIN_INPUT: u32 = WM_USER_OFFSET;

    /// Posted by external components to "switch to workspace index X".
    /// LPARAM is unused; WPARAM holds the index.
    pub const WM_REQUEST_SWITCH: u32 = WM_USER_OFFSET + 1;

    /// Posted by external components to "move foreground window to X".
    pub const WM_REQUEST_MOVE: u32 = WM_USER_OFFSET + 2;

    /// Offset for JacqueWM private messages. WM_USER is 0x0400.
    pub const WM_USER_OFFSET: u32 = 0x0400;
}

static MAIN_THREAD_ID: AtomicIsize = AtomicIsize::new(0);
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();
static HIDDEN_HWND: OnceLock<HWND> = OnceLock::new();

/// Records the calling thread as the main thread. Must be invoked once
/// at startup.
pub fn register_main_thread() {
    unsafe {
        let id = GetCurrentThreadId();
        MAIN_THREAD_ID.store(id as isize, Ordering::SeqCst);
    }
}

/// Returns the main thread id, if registered.
pub fn main_thread_id() -> u32 {
    MAIN_THREAD_ID.load(Ordering::SeqCst) as u32
}

/// Returns the registered `TaskbarCreated` message id. Returns `None`
/// if Explorer has not been notified yet.
pub fn taskbar_created_message() -> Option<u32> {
    TASKBAR_CREATED.get().copied()
}

/// Build a hidden message-only window and return its `HWND`.
///
/// `caption_proc` is `None` because the window has no caption. The
/// created class name is "JacqueWM_MessageWindow_Class".
///
/// # Safety
///
/// Calls into `CreateWindowExW` and `RegisterWindowMessageW`. Both
/// are safe under normal conditions provided they are called on the
/// main thread before any other component tries to send post messages
/// at this window.
pub fn create_message_window() -> Result<HWND> {
    unsafe {
        let class_name = w!("JacqueWM_MessageWindow_Class");
        let instance = windows::Win32::System::Com::CoTaskMemFree;
        // Register a no-op WNDCLASS.
        use windows::Win32::UI::WindowsAndMessaging::{
            RegisterClassExW, DefWindowProcW, CS_HREDRAW, CS_VREDRAW,
        };
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: None,
            lpszClassName: class_name,
            ..Default::default()
        };
        let atom = RegisterClassExW(&wc);
        if atom == 0 {
            // already registered — proceed.
        }

        let hwnd = CreateWindowExW(
            WS_EX_NOACTIVATE,
            class_name,
            w!("JacqueWM"),
            windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(0),
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            Some(HWND_MESSAGE), // message-only window
            None,
            None,
            None,
        )?;
        HIDDEN_HWND.set(hwnd).map_err(|_| {
            JacqueError::Other("Message window already created".into())
        })?;

        // Register for Explorer's TaskbarCreated broadcast message.
        let atom = RegisterWindowMessageW(w!("TaskbarCreated"));
        if atom != 0 {
            let _ = TASKBAR_CREATED.set(atom);
        }

        Ok(hwnd)
    }
}

/// Returns the previously-created hidden message window, if any.
pub fn hidden_hwnd() -> Option<HWND> {
    HIDDEN_HWND.get().copied()
}

/// Post a custom message at the hidden window from any thread.
pub fn post_at_hidden(message: u32, w: WPARAM, l: LPARAM) -> Result<()> {
    let Some(hwnd) = HIDDEN_HWND.get().copied() else {
        return Err(JacqueError::Other("Message window not created".into()));
    };
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
            Some(hwnd),
            message,
            Some(w),
            Some(l),
        );
    }
    Ok(())
}

/// Tear down the hidden window. Call at shutdown only.
pub fn destroy_message_window() {
    if let Some(hwnd) = HIDDEN_HWND.get().copied() {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
) -> LRESULT {
    // The hidden window does no work by itself. The custom dispatch
    // happens via PostMessageW + the main message pump.
    if msg == *TASKBAR_CREATED.get_or_init(|| 0) && msg != 0 {
        // Explorer has restarted. Notify via a one-shot callback.
        // The actual re-acquisition is performed by the platform
        // virtual-desktop module via dwm_init_watcher().
        tracing::warn!(
            target: "jacquewm.windows",
            "TaskbarCreated received — Explorer restarted"
        );
    }
    windows::Win32::UI::WindowsAndMessaging::DefWindowProcW(hwnd, msg, w, l)
}
