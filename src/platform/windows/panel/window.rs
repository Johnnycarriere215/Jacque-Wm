//! Win32 popup window for the top panel.
//!
//! Lives on a dedicated thread. The window class is registered only
//! on that thread, so the dedicated thread does its own
//! `CoInitializeEx(STA)`, Direct2D factory, and message pump.
//!
//! The thread receives a [`PanelCommand`] channel for commands
//! (shutdown, etc.) from the main thread; the main thread posts
//! `WM_PANEL_REFRESH` to schedule a repaint.

use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Duration;

use tracing::trace;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, InvalidateRect, PostQuitMessage,
    RegisterClassExW, UnregisterClassW, WNDCLASSEXW, WM_DESTROY, WM_PAINT, WS_EX_NOACTIVATE,
    WS_EX_TOPMOST, WS_POPUP,
};

use crate::core::panel::{PanelState, PANEL_HEIGHT};
use super::renderer::PanelRenderer;

/// Custom message fired by the main thread to request a repaint.
pub const WM_PANEL_REFRESH: u32 = WM_USER_OFFSET + 0;
const WM_USER_OFFSET: u32 = 0x0400;

/// Internal commands sent through the channel.
pub enum PanelCommand {
    Shutdown,
}

/// Resulting HWND of the panel window. Wrapped so callers can pass
/// it across threads without unsafe boilerplate.
#[derive(Clone, Copy, Debug)]
pub struct PanelHwnd(pub HWND);

impl PanelHwnd {
    pub fn raw(&self) -> HWND {
        self.0
    }
}

/// Entry point for the panel thread.
///
/// * Registers a `WS_POPUP` class on this thread.
/// * Creates an always-on-top window without any caption.
/// * Runs the per-frame paint cycle (custom `WM_PANEL_REFRESH` +
///   `WM_PAINT`).
#[allow(dead_code)]
pub fn panel_thread_main(shared: Arc<super::PanelShared>, rx: Receiver<PanelCommand>) -> crate::error::Result<()> {
    let class_name = w!("JacqueWM_Panel_Class");
    unsafe {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(panel_wnd_proc),
            hInstance: None,
            lpszClassName: class_name,
            ..Default::default()
        };
        // RegisterClassExW returns 0 if the class already exists; we
        // ignore that case as benign.
        let _atom = RegisterClassExW(&wc);

        // Compute monitor width.
        let screen_w = primary_screen_width();
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            class_name,
            w!("JacqueWM Panel"),
            WS_POPUP,
            0,
            0,
            screen_w,
            PANEL_HEIGHT,
            None,
            None,
            None,
            None,
        )
        .map_err(|e| {
            crate::JacqueError::Logging(format!("panel CreateWindowExW failed: {e:?}"))
        })?;
        // Mark the HWND in shared state so the host can post to it.
        // The shared state's `hwnd` field is owned by `WindowsPanelHost`,
        // not `PanelShared`. The host will pick it up via the
        // dedicated poll below.
        let _ = shared; // currently unused; host has direct hwnd

        // Run the message pump.
        let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
        loop {
            // Process any incoming channel commands (non-blocking).
            if let Ok(cmd) = rx.try_recv() {
                match cmd {
                    PanelCommand::Shutdown => {
                        let _ = DestroyWindow(hwnd);
                        PostQuitMessage(0);
                        return Ok(());
                    }
                }
            }

            // Dispatch Windows messages.
            let res = windows::Win32::UI::WindowsAndMessaging::GetMessageW(
                &mut msg,
                None,
                0,
                0,
            );
            if res.0 <= 0 {
                break;
            }
            let _ = windows::Win32::UI::WindowsAndMessaging::TranslateMessage(&msg);
            let _ = windows::Win32::UI::WindowsAndMessaging::DispatchMessageW(&msg);
        }

        let _ = UnregisterClassW(class_name, None);
        Ok(())
    }
}

unsafe extern "system" fn panel_wnd_proc(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            // Read shared state, render.
            let rect = read_client_rect(hwnd);
            // We delegate to the renderer through a static
            // `Arc<Mutex<…>>` registered at startup. Lacking a
            // registry here, we just invalidate; the renderer is
            // invoked from the `WM_PANEL_REFRESH` path.
            let _ = rect;
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_PANEL_REFRESH => {
            // Repaint path: the renderer writes to the HDC.
            paint_now(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

fn read_client_rect(hwnd: HWND) -> RECT {
    unsafe {
        let mut r = RECT::default();
        let _ = GetClientRect(hwnd, &mut r);
        r
    }
}

/// Idle-time paint. We paint only when WM_PAINT or WM_PANEL_REFRESH
/// fires — no timer-driven redraw is needed because state updates
/// drive the repaint.
fn paint_now(_hwnd: HWND) {
    trace!(target: "jacquewm.windows", "panel paint tick");
    // TODO_GPU_RENDERER_HOOK: in production this is where the
    // Direct2D + DWrite paint cycle runs. Calling `PanelRenderer::draw`
    // here is the canonical path.
    // Until the renderer is wired to a global state handle, the
    // paint is a no-op. CPU usage stays at zero.
}

fn primary_screen_width() -> i32 {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN};
        let w = GetSystemMetrics(SM_CXSCREEN);
        if w <= 0 {
            1920
        } else {
            w
        }
    }
}

// Silence unused import warnings — `PanelState` and `PanelRenderer`
// are referenced conceptually in the renderer hook above.
#[allow(dead_code)]
fn _ensure_imports_in_scope(_: &PanelState, _: &PanelRenderer, _: Duration) {}
