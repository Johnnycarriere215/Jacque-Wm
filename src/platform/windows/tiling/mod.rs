//! Windows tiling operations.
//!
//! Marshal tile rectangles from [`crate::core::tiling`] into
//! `SetWindowPos` calls. Uses [`BeginDeferWindowPos`] for atomic
//! batch moves so the user never sees intermediate positions during
//! a workspace re-layout.
//!
//! This is the only place in the codebase that *writes* window
//! geometry. The COM virtual-desktop layer already handles
//! "move-window-to-desktop" — this module only handles
//! "move-window-within-its-desktop".

use std::collections::HashMap;

use tracing::{debug, warn};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    BeginDeferWindowPos, DeferWindowPos, EndDeferWindowPos, SetWindowPos, SWP_NOACTIVATE,
    SWP_NOZORDER, SWP_SHOWWINDOW, HDWP,
};

use crate::core::tiling::LayoutSolution;
use crate::core::wm::{Rect, WindowId};

/// Apply `solution` to the live OS windows using atomic
/// `BeginDeferWindowPos/EndDeferWindowPos`.
pub fn apply_to_windows(solution: &LayoutSolution) -> u32 {
    let n = solution.windows.len().max(1);
    let mut defer: HDWP = match unsafe { BeginDeferWindowPos(n as i32) } {
        Ok(h) => h,
        Err(e) => {
            warn!(target: "jacquewm.tiling", error = ?e, "BeginDeferWindowPos failed");
            return 0;
        }
    };

    let mut moved = 0u32;
    for (id, rect) in &solution.windows {
        let hwnd = HWND(id.get() as *mut std::ffi::c_void);
        let target = RECT {
            left: rect.x,
            top: rect.y,
            right: rect.x + rect.width,
            bottom: rect.y + rect.height,
        };
        let next = unsafe {
            DeferWindowPos(
                defer,
                hwnd,
                None,
                target.left,
                target.top,
                target.width,
                target.height,
                SWP_NOACTIVATE | SWP_NOZORDER | SWP_SHOWWINDOW,
            )
        };
        match next {
            Ok(h) => {
                defer = h;
                moved += 1;
            }
            Err(e) => {
                warn!(target: "jacquewm.tiling", error = ?e, hwnd = id.get(), "DeferWindowPos failed");
            }
        }
    }
    let _ = unsafe { EndDeferWindowPos(defer) };
    debug!(target: "jacquewm.tiling", moved, "applied layout");
    moved
}

/// Move a single window to the given rect (used for fullscreen
/// transitions, drag updates, etc.). Sets no-activate/no-z-order
/// flags so focus is not stolen.
pub fn move_window_to(id: WindowId, rect: Rect) -> std::result::Result<(), windows::core::Error> {
    let hwnd = HWND(id.get() as *mut std::ffi::c_void);
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            SWP_NOACTIVATE,
        )
    }
}

// =====================================================================
// Geometry conversion helpers — application rules and tiling engine
// both operate in core::wm::Rect; the platform speaks windows RECT.
// =====================================================================

/// Convert a vector of `Rect` into a `RECT` array ready for
/// `DeferWindowPos`.
pub fn to_win_rects(rects: &[Rect]) -> Vec<RECT> {
    rects
        .iter()
        .map(|r| RECT {
            left: r.x,
            top: r.y,
            right: r.x + r.width,
            bottom: r.y + r.height,
        })
        .collect()
}

/// Identity map from `WindowId` to `Rect` used by the inner-loop
/// callers.
pub fn solution_to_map(solution: &LayoutSolution) -> HashMap<WindowId, Rect> {
    solution.windows.iter().copied().collect()
}
