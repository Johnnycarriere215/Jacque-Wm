//! Low-level keyboard hook (`WH_KEYBOARD_LL`).
//!
//! Translates raw Win32 KBDLLHOOKSTRUCT values into OS-agnostic
//! [`HotkeyPress`] events and pushes them into a bounded crossbeam
//! channel. The main thread consumes from the channel.
//!
//! Important constraints:
//! * The hook callback must return *fast* — Windows unregisters the
//!   hook if it spends more than ~300ms inside the callback.
//! * All decoding must therefore be quick: array lookups and bit ops
//!   only. No COM, no logging, no allocations.

use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT,
    WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::core::hotkeys::keys::{HotkeyPress, KeyCode, Modifiers};
use crate::core::hotkeys::register::HotkeySink;
use crate::error::{JacqueError, Result};

/// Global state required by the hook callback. Set by
/// [`WindowsKeyboardHook::install`] and read on every event.
struct HookShared {
    sink: Option<Arc<dyn HotkeySink>>,
}

static HOOK_SHARED: Mutex<HookShared> = parking_lot::const_mutex(HookShared { sink: None });
static HOOK_HANDLE: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Translate a Win32 VK code to JacqueWM's [`KeyCode`].
///
/// Only digits 1-9 and a small set of named keys are mapped. Anything
/// else becomes `KeyCode::Virtual(vk)`.
fn vk_to_keycode(vk: VIRTUAL_KEY) -> KeyCode {
    let raw = vk.0;
    match raw {
        0x30..=0x39 => KeyCode::Digit((raw - 0x30) as u8), // '0'-'9'
        v @ 0x41..=0x5A => KeyCode::Letter((v as u8 as char).to_ascii_lowercase()),
        other => KeyCode::Virtual(other),
    }
}

/// Read the keyboard modifier state via `GetAsyncKeyState`.
///
/// We could derive this from the KBDLLHOOKSTRUCT flags, but
/// `GetAsyncKeyState` is faster and produces the same answer except
/// during the very first call of a press sequence.
fn read_modifiers() -> Modifiers {
    let mut m = Modifiers::empty();
    // `GetAsyncKeyState` returns the high bit set if the key is down.
    unsafe {
        if (GetAsyncKeyState(0x11).0 as u16) & 0x8000 != 0 {
            m |= Modifiers::CTRL;
        }
        if (GetAsyncKeyState(0x10).0 as u16) & 0x8000 != 0 {
            m |= Modifiers::SHIFT;
        }
        if (GetAsyncKeyState(0x12).0 as u16) & 0x8000 != 0 {
            m |= Modifiers::ALT;
        }
        if (GetAsyncKeyState(0x5B).0 as u16) & 0x8000 != 0
            || (GetAsyncKeyState(0x5C).0 as u16) & 0x8000 != 0
        {
            m |= Modifiers::SUPER;
        }
    }
    m
}

/// Translate the raw KBDLLHOOKSTRUCT into a [`HotkeyPress`] if the
/// event is a *down* event with at least one modifier. Returns `None`
/// for *up* events and unmodified digit presses (those would steal too
/// much from the user).
fn press_from_hook(code: i32, w: WPARAM, l: LPARAM) -> Option<HotkeyPress> {
    if code < 0 {
        return None;
    }
    let msg = w.0 as u32;
    let auto_repeat = match msg {
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let kh = unsafe { &*(l.0 as *const KBDLLHOOKSTRUCT) };
            kh.flags.0 & 0x00000002 != 0 // LLKHF_REPEAT
        }
        WM_KEYUP | WM_SYSKEYUP => return None,
        _ => return None,
    };
    let kh = unsafe { &*(l.0 as *const KBDLLHOOKSTRUCT) };
    let vk = VIRTUAL_KEY(kh.vkCode);
    let modifiers = read_modifiers();
    if modifiers.is_empty() {
        return None;
    }
    let key_code = vk_to_keycode(vk);
    Some(HotkeyPress::new(key_code, modifiers, auto_repeat))
}

/// The OS hook callback. Must be cheap.
///
/// # Safety
///
/// Windows calls this function on the thread that registered the
/// hook. The thread *must* be pumping messages for the hook to work.
unsafe extern "system" fn hook_proc(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if let Some(press) = press_from_hook(code, w, l) {
        let guard = HOOK_SHARED.lock();
        if let Some(sink) = guard.sink.as_ref() {
            sink.push(press);
        }
    }
    // Unconditionally pass the event on.
    CallNextHookEx(None, code, w, l)
}

/// OS-agnostic HotkeySource implementation.
pub struct WindowsKeyboardHook;

impl WindowsKeyboardHook {
    /// Construct a new hook (not yet installed).
    pub fn new() -> Self {
        Self
    }

    /// Install the hook. The supplied [`HotkeySink`] receives every
    /// decoded press.
    ///
    /// # Safety
    ///
    /// Installs a system-wide low-level hook. Caller must guarantee
    /// that the calling thread (1) is the main thread, and (2) is
    /// about to run a message pump (`GetMessageW` loop).
    pub fn install(&self, sink: Arc<dyn HotkeySink>) -> Result<()> {
        let mut guard = HOOK_SHARED.lock();
        if INSTALLED.load(Ordering::SeqCst) {
            return Err(JacqueError::HookInstall("hook already installed".into()));
        }
        guard.sink = Some(sink);

        let hhook = unsafe {
            SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0)
                .map_err(|e| JacqueError::HookInstall(format!("{:?}", e)))?
        };
        HOOK_HANDLE.store(hhook.0 as *mut _, Ordering::SeqCst);
        INSTALLED.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Remove the hook, freeing the system handle.
    pub fn uninstall(&self) -> Result<()> {
        if !INSTALLED.swap(false, Ordering::SeqCst) {
            return Ok(());
        }
        let raw = HOOK_HANDLE.swap(std::ptr::null_mut(), Ordering::SeqCst);
        if !raw.is_null() {
            unsafe {
                let _ = UnhookWindowsHookEx(HHOOK(raw));
            }
        }
        HOOK_SHARED.lock().sink = None;
        Ok(())
    }

    /// Returns `true` if a system-wide hook is currently installed.
    pub fn is_installed(&self) -> bool {
        INSTALLED.load(Ordering::SeqCst)
    }
}

impl Default for WindowsKeyboardHook {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for WindowsKeyboardHook {
    fn drop(&mut self) {
        let _ = self.uninstall();
    }
}

// SAFETY: `HWND` constants used in this module are `Copy` and
// trivially safe to use.
impl Send for WindowsKeyboardHook {}
impl Sync for WindowsKeyboardHook {}
