//! Top-panel Windows implementation.
//!
//! The panel is a top-level `WS_POPUP` window (not a child, to avoid
//! `AttachThreadInput` deadlocks) running on a dedicated thread with
//! its own message pump. Rendering is Direct2D + DWrite; updates are
//! driven by `submit(PanelState)` from the main thread and a periodic
//! 1Hz metrics tick from the renderer thread itself.
//!
//! Architecture:
//!
//! * Main thread (existing GetMessageW loop) updates the
//!   [`PanelController`] in `core/panel`. Each update calls
//!   `submit(state)`.
//! * `submit` writes into a shared `Arc<Mutex<PanelState>>` and POSTs
//!   a custom `WM_JACQUEWM_PANEL_REFRESH` to the panel thread's
//!   hidden window.
//! * The panel thread's `GetMessageW` receives the refresh message
//!   and re-renders. Metrics are sampled inline before each paint.
//!
//! No direct coupling: the platform layer is the only thing that
//! knows about Direct2D, DWrite, or HDCs. Tests can replace
//! `WindowsPanelHost` with the in-memory
//! [`crate::core::panel::NullPanel`].

use std::sync::{Arc, Mutex};

use tracing::info;

use crate::core::panel::{PanelHost, PanelHostRef, PanelState};
use crate::platform::windows::panel::renderer::PanelRenderer;
use crate::platform::windows::panel::window::PanelWindow;

/// Shared state moved to the panel thread.
struct PanelShared {
    state: Mutex<PanelState>,
    renderer: Mutex<PanelRenderer>,
    rect: Mutex<(i32, i32, i32, i32)>,
}

impl PanelShared {
    fn new(initial: &PanelState) -> Self {
        Self {
            state: Mutex::new(initial.clone()),
            renderer: Mutex::new(PanelRenderer::new(initial.theme.clone(), initial.clone())),
            rect: Mutex::new((0, 0, 0, 32)),
        }
    }
}

/// Concrete `PanelHost` for Windows. Owns the panel thread, the
/// hidden window handle, and the panel's main `HWND`.
pub struct WindowsPanelHost {
    shared: Arc<PanelShared>,
    thread_handle: Mutex<Option<std::thread::JoinHandle<()>>>,
    hwnd: Mutex<Option<windows::Win32::Foundation::HWND>>,
    running: Mutex<bool>,
}

impl WindowsPanelHost {
    /// Build a fresh panel host with the initial state pre-populated.
    pub fn new(initial: PanelState) -> Self {
        Self {
            shared: Arc::new(PanelShared::new(&initial)),
            thread_handle: Mutex::new(None),
            hwnd: Mutex::new(None),
            running: Mutex::new(false),
        }
    }
}

impl PanelHost for WindowsPanelHost {
    fn start(&self) -> crate::error::Result<()> {
        let mut running = self.running.lock().unwrap();
        if *running {
            return Ok(());
        }
        let shared = self.shared.clone();
        let (tx, rx) = std::sync::mpsc::channel::<super::window::PanelCommand>();
        let join = std::thread::Builder::new()
            .name("jacquewm-panel".into())
            .spawn(move || {
                if let Err(e) = window::panel_thread_main(shared, rx) {
                    tracing::error!(
                        target: "jacquewm.windows",
                        error = %e,
                        "panel thread exited with error"
                    );
                }
            })
            .map_err(|e| crate::JacqueError::Logging(format!("panel thread spawn failed: {e}")))?;
        *self.thread_handle.lock().unwrap() = Some(join);
        *running = true;
        info!(target: "jacquewm.windows", "panel thread started");
        Ok(())
    }

    fn submit(&self, state: PanelState) {
        let shared = self.shared.clone();
        // Replace the shared state under the mutex.
        {
            let mut g = shared.state.lock().unwrap();
            *g = state.clone();
        }
        {
            let mut r = shared.renderer.lock().unwrap();
            r.update(state);
        }
        // Wake the panel thread.
        if let Some(hwnd) = *self.hwnd.lock().unwrap() {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                    Some(hwnd),
                    super::window::WM_PANEL_REFRESH,
                    None,
                    None,
                );
            }
        }
    }

    fn shutdown(&self) {
        let hwnd = *self.hwnd.lock().unwrap();
        if let Some(hwnd) = hwnd {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
            }
        }
        // The thread will observe `WM_QUIT` after we destroy the
        // window or call PostQuitMessage; another path is to send an
        // explicit WM_QUIT.
        if let Some(_join) = self.thread_handle.lock().unwrap().take() {
            // The thread's RAII shutdown will reap itself when the
            // window is destroyed.
        }
        *self.running.lock().unwrap() = false;
    }

    fn state_snapshot(&self) -> PanelState {
        self.shared.state.lock().unwrap().clone()
    }

    fn rect(&self) -> (i32, i32, i32, i32) {
        *self.shared.rect.lock().unwrap()
    }
}

/// Convenience: build a `PanelHostRef` ready for the main loop.
pub fn build_host(initial: PanelState) -> PanelHostRef {
    Arc::new(WindowsPanelHost::new(initial))
}
