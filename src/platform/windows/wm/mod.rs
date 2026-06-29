//! Windows implementation of [`crate::core::wm::WindowManager`].
//!
//! Connects the OS event sources to the OS-agnostic tracker:
//! * Initial discovery via [`enumerate_at_startup`] on startup.
//! * Live updates via [`hook`] → main thread's `GetMessageW` loop
//!   (because `WINEVENT_OUTOFCONTEXT` callbacks fire on the
//!   registering thread's message pump).

use std::cell::RefCell;
use std::sync::{Arc, Mutex};

use tracing::{debug, info, warn};
use windows::Win32::Foundation::{HWND, LPARAM, BOOL};
use windows::Win32::UI::Accessibility::HWINEVENTHOOK;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindow, GetWindowLongPtrW, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, GWL_EXSTYLE, GW_OWNER,
};

use crate::core::wm::{MonitorDef, ProcessInfo, WindowEvent, WindowManager, WindowMetadata, Registry};
use crate::core::WorkspaceIndex;

pub mod hook;

/// Construct a fresh tracked WindowManager for Windows. The returned
/// `Arc<dyn WindowManager>` is ready for the main loop to feed events
/// into.
pub fn build_tracker() -> Arc<dyn WindowManager> {
    Arc::new(Registry::new())
}

thread_local! {
    static ENUM_TRACKER: RefCell<Option<Arc<dyn WindowManager>>> = const { RefCell::new(None) };
}

/// Run initial window discovery on the calling thread. The `tracker`
/// parameter takes ownership of an `Arc`, so the inner
/// `EnumWindows` callback can push `WindowEvent::Created` records.
pub fn enumerate_at_startup(tracker: Arc<dyn WindowManager>) {
    // Bind for the duration of this call.
    ENUM_TRACKER.with(|slot| {
        *slot.borrow_mut() = Some(tracker.clone());
    });

    unsafe extern "system" fn proc(hwnd: HWND, _lparam: LPARAM) -> BOOL {
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        let owner = GetWindow(hwnd, GW_OWNER);
        if !owner.is_err() && !owner.unwrap().0.is_null() {
            return BOOL(1);
        }
        unsafe {
            let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            const WS_EX_TOOLWINDOW: isize = 0x0000_0080;
            const WS_EX_APPWINDOW: isize = 0x0004_0000;
            if (ex_style & WS_EX_TOOLWINDOW) != 0 && (ex_style & WS_EX_APPWINDOW) == 0 {
                return BOOL(1);
            }
        }
        let mut pid = 0u32;
        unsafe {
            let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
        }
        let len = unsafe { GetWindowTextLengthW(hwnd) } as usize;
        let mut text_buf = vec![0u16; len + 1];
        let actual =
            unsafe { GetWindowTextW(hwnd, &mut text_buf) } as usize;
        let title = String::from_utf16_lossy(
            &text_buf[..actual.min(text_buf.len())],
        );
        let mut class_buf = [0u16; 256];
        let class_len =
            unsafe { GetClassNameW(hwnd, &mut class_buf) } as usize;
        let class = String::from_utf16_lossy(
            &class_buf[..class_len.min(class_buf.len())],
        );

        let id = crate::core::wm::WindowId::new(hwnd.0 as u64);
        let meta = WindowMetadata {
            id,
            process: ProcessInfo {
                pid,
                exe_path: None,
                exe_basename: String::new(),
            },
            title: crate::core::wm::WindowTitle::new(title),
            class,
            state: crate::core::wm::WindowState::VISIBLE,
            rect: crate::core::wm::Rect::default(),
            workspace: WorkspaceIndex::new_unchecked(1),
            monitor: crate::core::wm::MonitorId::PRIMARY,
            z_order: 0,
            tiled: false,
        };
        ENUM_TRACKER.with(|slot| {
            if let Some(t) = slot.borrow().as_ref() {
                t.apply(WindowEvent::Created(meta));
            }
        });
        BOOL(1)
    }
    unsafe {
        let _ = EnumWindows(Some(proc), LPARAM(0));
    }

    // Restore the thread-local to None so the inner Arc can be
    // dropped if this is the last reference.
    ENUM_TRACKER.with(|slot| {
        *slot.borrow_mut() = None;
    });
    info!(target: "jacquewm.wm", "startup window enumeration complete");
}

/// Install `SetWinEventHook` on the main thread. Callbacks fire on the
/// main pump. Returns a handle that unhooks on drop.
pub fn install_event_hook(tracker: Arc<dyn WindowManager>) -> Result<hook::HookHandle, crate::error::JacqueError> {
    hook::install(tracker)
}

/// Best-effort enumeration of monitors. Uses `EnumDisplayMonitors`.
///
/// **Reviewer fix:** uses an `Arc<Mutex<Vec<MonitorDef>>>` shared
/// across the callback to accumulate. Replaces the tracker list once
/// after the enumeration completes — no leaks, no per-iteration
/// replace.
pub fn enumerate_monitors(tracker: &dyn WindowManager) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, MONITORINFOEXW,
    };

    // Inner collector: pinned for the lifetime of `EnumDisplayMonitors`.
    // Dropping this at end of function is safe because the
    // `EnumDisplayMonitors` call has already returned.
    let collector: Arc<Mutex<Vec<MonitorDef>>> = Arc::new(Mutex::new(Vec::new()));
    let collector_for_cb = collector.clone();

    unsafe extern "system" fn monitor_proc(
        hmonitor: windows::Win32::Graphics::Gdi::HMONITOR,
        _hdc: windows::Win32::Graphics::Gdi::HDC,
        _rect_clip: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        let ok = GetMonitorInfoW(hmonitor, &mut info as *mut _ as *mut _);
        if !ok.as_bool() {
            return BOOL(1);
        }
        let rect = info.monitorInfo.rcMonitor;
        let id = crate::core::wm::MonitorId::new(hmonitor.0 as u32);
        let friendly = String::from_utf16_lossy(&info.szDevice)
            .trim_end_matches('\0')
            .to_string();
        let mon = MonitorDef {
            id,
            friendly_name: friendly,
            rect: crate::core::wm::Rect::new(
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
            ),
            dpi: 96,
            primary: (info.monitorInfo.dwFlags & 1) != 0,
        };
        let collector = unsafe { &*(data.0 as *const Arc<Mutex<Vec<MonitorDef>>>) };
        collector.lock().unwrap().push(mon);
        BOOL(1)
    }
    unsafe {
        let collector_ptr = Arc::into_raw(collector_for_cb) as isize;
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(monitor_proc),
            LPARAM(collector_ptr),
        );
        // Reconstruct and drop the Arc. The unsafe `from_raw` here
        // balances the `into_raw` above. Safe because no one else
        // holds a pointer after this returns.
        let _ = Arc::from_raw(collector_ptr as *const Mutex<Vec<MonitorDef>>);
    }

    let collected: Vec<MonitorDef> = collector.lock().unwrap().drain(..).collect();
    debug!(target: "jacquewm.wm", count = collected.len(), "monitor collector drained");
    if !collected.is_empty() {
        tracker.replace_monitors(collected);
    }
    warn!(target: "jacquewm.wm", "monitor enumeration complete");
}

// We don't use a separate spawning thread for the event hook — the
// registering thread is the message-pump thread (main).
pub use hook::HookHandle as WinEventHandle;

#[allow(unused_imports)]
use HWINEVENTHOOK as _KeepHWINEVENTHOOKInScope;
