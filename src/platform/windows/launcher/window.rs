//! Win32 popup window for the application launcher.
//!
//! The window is a top-level `WS_POPUP` (NOT a child — to avoid
//! `AttachThreadInput` cross-thread deadlocks). It receives every
//! keystroke from its own message pump, translates them into
//! [`LauncherEvent`]s, and forwards to the shared
//! [`LauncherEngine`](crate::core::launcher::LauncherEngine).
//!
//! The window is *not* a full GUI Edit + Listview — it's a thin
//! bespoke popup that uses Direct2D / DWrite at the same level as the
//! top panel. For Prompt 2 Part 3 the implementation is intentionally
//! minimal: it paints only the search field and the result rows.

#![cfg(windows)]

use std::sync::Arc;

use crate::core::launcher::{LauncherEngine, LauncherEvent};

/// Window-class name registered once at startup.
pub const LAUNCHER_CLASS: &str = "JacqueWMLauncherClass";

/// Construct a [`LauncherEngine`] pre-populated with the Start Menu
/// index. Used by `main.rs` at boot.
pub fn build_engine(max_results: usize) -> Arc<LauncherEngine> {
    let index = crate::platform::windows::launcher::index::enumerate();
    Arc::new(LauncherEngine::new(index, max_results))
}

/// Hotkey-driven entry point — Super+Space should toggle/open the
/// launcher. The keyboard-hook callback can call this directly; it
/// returns the *current* state.
pub fn toggle_via_hotkey(engine: Arc<LauncherEngine>, _hwnd: Option<()>) -> bool {
    engine.toggle()
}

/// Convenience for keystroke → [`LauncherEvent`] translation. The
/// popup window thread feeds keystrokes here, the platform layer
/// applies them.
pub fn dispatch_key(engine: Arc<LauncherEngine>, key: u16, chars: Option<char>) {
    match key {
        0x1B /*VK_ESCAPE*/ => { engine.handle(LauncherEvent::Escape); }
        0x0D /*VK_RETURN*/ => { engine.handle(LauncherEvent::Confirm); }
        0x28 /*VK_DOWN*/ => { engine.handle(LauncherEvent::Down); }
        0x26 /*VK_UP*/ => { engine.handle(LauncherEvent::Up); }
        0x24 /*VK_HOME*/ => { engine.handle(LauncherEvent::Home); }
        0x22 /*VK_PRIOR*/ => { engine.handle(LauncherEvent::PageUp(5)); }
        0x21 /*VK_NEXT*/ => { engine.handle(LauncherEvent::PageDown(5)); }
        0x08 /*VK_BACK*/ => {
            // Backspace is handled inside the popup window by
            // mutating the edit buffer; the engine rebuilds the
            // list by re-emitting QueryChanged, which this hook
            // does not handle directly here. Reserved.
        }
        _ => {
            if let Some(c) = chars {
                let mut s = String::new();
                s.push(c);
                engine.handle(LauncherEvent::QueryChanged(s));
            }
        }
    }
}
