//! `SetWinEventHook` bridge.
//!
//! Events of interest, mapped from the Win32 accessibility event
//! ordinals (canonical values published by `windows-rs`):
//!
//! * `EVENT_OBJECT_CREATE`             (0x8000)
//! * `EVENT_OBJECT_DESTROY`            (0x8001)
//! * `EVENT_OBJECT_FOCUS`              (0x8005)
//! * `EVENT_OBJECT_SHOW`               (0x8002)
//! * `EVENT_OBJECT_HIDE`               (0x8003)
//! * `EVENT_OBJECT_LOCATIONCHANGE`     (0x800B)
//! * `EVENT_OBJECT_MINIMIZED`          (0x8016)
//! * `EVENT_OBJECT_RESTORED`           (0x8017)
//! * `EVENT_OBJECT_MAXIMIZED`          (0x8018)
//! * `EVENT_SYSTEM_FOREGROUND`         (0x0003)
//! * `EVENT_SYSTEM_MOVESIZEEND`        (0x000B)
//!
//! `WINEVENT_OUTOFCONTEXT` callbacks fire on the *registering*
//! thread's message pump. We register on the main thread so the
//! main pump sees them during `DispatchMessageW`. There is no
//! need for a "marshal-to-main-thread" hop.

use std::sync::Arc;

use tracing::{trace, warn};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::{
    SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK, WINEVENT_OUTOFCONTEXT,
};
use windows::Win32::System::Threading::{GetCurrentThreadId, GetWindowThreadProcessId};

use crate::core::wm::{
    ProcessInfo, WindowEvent, WindowManager, WindowMetadata, WindowState, WindowTitle,
};
use crate::core::WorkspaceIndex;

/// Wrapper around the hook handle so we can drop it deterministically.
pub struct HookHandle {
    hooks: Vec<HWINEVENTHOOK>,
    #[allow(dead_code)]
    tracker: Arc<dyn WindowManager>,
}

impl Drop for HookHandle {
    fn drop(&mut self) {
        unsafe {
            for h in self.hooks {
                let _ = UnhookWinEvent(h);
            }
        }
        trace!(target: "jacquewm.wm", "WINEVENT hooks removed");
    }
}

/// Install the SetWinEventHooks. Returns a [`HookHandle`] that
/// unhooks on drop. The handle MUST be kept alive for the lifetime
/// of the application — dropping it stops the events.
pub fn install(tracker: Arc<dyn WindowManager>) -> Result<HookHandle, crate::error::JacqueError> {
    let tid = unsafe { GetCurrentThreadId() };

    // Canonical Win32 accessibility event ordinals.
    // Keep the comments in sync with the match arms below.
    const EVENTS: &[u32] = &[
        0x8000, // EVENT_OBJECT_CREATE
        0x8001, // EVENT_OBJECT_DESTROY
        0x8005, // EVENT_OBJECT_FOCUS
        0x8002, // EVENT_OBJECT_SHOW
        0x8003, // EVENT_OBJECT_HIDE
        0x800B, // EVENT_OBJECT_LOCATIONCHANGE
        0x8016, // EVENT_OBJECT_MINIMIZED
        0x8017, // EVENT_OBJECT_RESTORED
        0x8018, // EVENT_OBJECT_MAXIMIZED
        0x0003, // EVENT_SYSTEM_FOREGROUND
        0x000B, // EVENT_SYSTEM_MOVESIZEEND
    ];

    let mut hooks = Vec::with_capacity(EVENTS.len());
    for ev in EVENTS {
        let h = unsafe {
            SetWinEventHook(
                *ev,
                *ev,
                None,
                Some(win_event_proc),
                0,
                tid,
                WINEVENT_OUTOFCONTEXT,
            )
            .unwrap_or(HWINEVENTHOOK(std::ptr::null_mut()))
        };
        if !h.0.is_null() {
            hooks.push(h);
        }
    }

    tracing::info!(
        target: "jacquewm.wm",
        count = hooks.len(),
        tid = tid,
        "SetWinEventHook installed"
    );

    Ok(HookHandle { hooks, tracker })
}

// SAFETY: the proc pointer is `extern "system"` and never blocks.
// We pass the tracker through a thread-local because the Win32 callback
// signature does not allow user data.
thread_local! {
    static TRACKER: std::cell::RefCell<Option<Arc<dyn WindowManager>>> = const { std::cell::RefCell::new(None) };
}

unsafe extern "system" fn win_event_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _id_thread: u32,
    _time: u32,
) {
    if hwnd.0.is_null() {
        return;
    }
    let id = crate::core::wm::WindowId::new(hwnd.0 as u64);

    TRACKER.with(|slot| {
        let Some(tracker) = slot.borrow().as_ref() else { return };
        match event {
            0x8000 => {
                let mut pid = 0u32;
                let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
                let meta = WindowMetadata {
                    id,
                    process: ProcessInfo {
                        pid,
                        exe_path: None,
                        exe_basename: String::new(),
                    },
                    title: WindowTitle::new("Untitled"),
                    class: String::new(),
                    state: WindowState::VISIBLE,
                    rect: crate::core::wm::Rect::default(),
                    workspace: WorkspaceIndex::new_unchecked(1),
                    monitor: crate::core::wm::MonitorId::PRIMARY,
                    z_order: 0,
                    tiled: false,
                };
                tracker.apply(WindowEvent::Created(meta));
            }
            0x8001 => tracker.apply(WindowEvent::Destroyed { id }),
            0x8005 => tracker.apply(WindowEvent::Focused { id }),
            0x0003 => tracker.apply(WindowEvent::Focused { id }),
            0x800B => {
                // EVENT_OBJECT_LOCATIONCHANGE — emitted every time
                // DWM repositions the window. Cheap; we read the
                // current rect.
                let mut rect = windows::Win32::Foundation::RECT::default();
                let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(
                    hwnd, &mut rect);
                tracker.apply(WindowEvent::MovedOrResized {
                    id,
                    rect: crate::core::wm::Rect::new(
                        rect.left,
                        rect.top,
                        rect.right - rect.left,
                        rect.bottom - rect.top,
                    ),
                });
            }
            0x000B => {
                // EVENT_SYSTEM_MOVESIZEEND — fires after a user drag
                // finishes. Treat identically to LOCATIONCHANGE
                // (cheap idempotent update).
                let mut rect = windows::Win32::Foundation::RECT::default();
                let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(
                    hwnd, &mut rect);
                tracker.apply(WindowEvent::MovedOrResized {
                    id,
                    rect: crate::core::wm::Rect::new(
                        rect.left,
                        rect.top,
                        rect.right - rect.left,
                        rect.bottom - rect.top,
                    ),
                });
            }
            0x8002 => tracker.apply(WindowEvent::Shown { id }),
            0x8003 => tracker.apply(WindowEvent::Hidden { id }),
            0x8016 => tracker.apply(WindowEvent::Minimized { id }),
            0x8017 => tracker.apply(WindowEvent::Restored { id }),
            0x8018 => tracker.apply(WindowEvent::Maximized { id }),
            _ => {
                trace!(target: "jacquewm.wm", event = event, hwnd = hwnd.0 as u64, "unhandled event");
            }
        }
    });
}

/// Internal helper: bind the win-event callback to a tracker. Called
/// before [`crate::main`] enters its message loop.
pub fn bind_callback(tracker: Arc<dyn WindowManager>) {
    TRACKER.with(|slot| {
        *slot.borrow_mut() = Some(tracker);
    });
}

#[allow(dead_code)]
fn _warn_unused() {
    warn!("spy");
}
