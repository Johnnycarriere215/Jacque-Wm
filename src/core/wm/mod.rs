//! Event-driven Window Manager.
//!
//! New in Prompt 2 — supersedes the thin "find foreground + move"
//! helper from Prompt 1 (`crate::core::windows`). The two coexist:
//! `crate::core::windows::WindowManager` is still used by the hotkey
//! dispatcher for the simple "move focused window" path.
//!
//! `WindowManager` here tracks every managed window, the monitor it
//! lives on, its workspace assignment, and the focus order. Every
//! change is driven by [`WindowEvent`] values emitted from the
//! platform layer (via [`crate::platform::windows::wm::hook`]).
//!
//! Implementation notes:
//!
//! * The tracker is single-threaded — it lives on the main STA
//!   thread that registered `SetWinEventHook(WINEVENT_OUTOFCONTEXT)`.
/// * Reads from non-main threads must go through an `Arc<WindowManager>`
///   snapshot accessor; the tracker does not expose an external
///   mutex because concurrent writes are impossible.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use bitflags::bitflags;
use parking_lot::RwLock;

use crate::core::WorkspaceIndex;

// =====================================================================
// Types
// =====================================================================

/// Strongly typed HWND wrapper. Raw value `0` represents "no window"
/// (the desktop placeholder). Casting a real Win32 HWND to [`u64`] is
/// always valid because HWND is `isize`-sized on all x86/x64 Windows
/// builds we target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowId(u64);

impl WindowId {
    /// Sentinel "no window" value.
    pub const NONE: WindowId = WindowId(0);

    /// Wrap an HWND value. `Some(0)` → `WindowId::NONE`.
    #[inline]
    pub fn new(hwnd: u64) -> Self {
        if hwnd == 0 {
            WindowId::NONE
        } else {
            WindowId(hwnd)
        }
    }

    /// Raw `u64` value (matches `HWND as u64`).
    #[inline]
    pub fn get(self) -> u64 {
        self.0
    }

    /// `true` if this is the sentinel.
    #[inline]
    pub fn is_none(self) -> bool {
        self == Self::NONE
    }
}

impl std::fmt::Display for WindowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "hwnd#{}", self.0)
    }
}

/// A win32 window rect. We use plain `i32` rather than the
/// `windows::Win32::Foundation::RECT` to stay platform-agnostic at
/// the core layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    /// Construct a rect.
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self { x, y, width, height }
    }

    /// Construct from `(left, top, right, bottom)` tuples.
    pub fn from_ltrb(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        }
    }

    pub fn left(&self) -> i32 {
        self.x
    }
    pub fn top(&self) -> i32 {
        self.y
    }
    pub fn right(&self) -> i32 {
        self.x + self.width
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.height
    }
    pub fn area(&self) -> i64 {
        self.width as i64 * self.height as i64
    }
    pub fn is_valid(&self) -> bool {
        self.width > 0 && self.height > 0
    }
}

/// Window title with bounded length. The platform layer caps the
/// length; the panel truncates further if necessary.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WindowTitle(String);

impl WindowTitle {
    pub fn new<S: Into<String>>(s: S) -> Self {
        let raw = s.into();
        let truncated: String = if raw.chars().count() > 512 {
            // Trim to 512 chars and append an ellipsis sentinel.
            raw.chars().take(511).collect()
        } else {
            raw
        };
        Self(truncated)
    }
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
    pub fn is_empty(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl std::fmt::Display for WindowTitle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

bitflags! {
    /// Trackable window state flags — emitted by the event loop and
    /// observed by the tiling engine.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct WindowState: u32 {
        const VISIBLE        = 0b0000_0001;
        const MINIMIZED      = 0b0000_0010;
        const MAXIMIZED      = 0b0000_0100;
        const RESIZABLE      = 0b0000_1000;
        const TILED          = 0b0001_0000;
        const FLOATING       = 0b0010_0000;
        const FULLSCREEN     = 0b0100_0000;
        const ATTACHED       = 0b1000_0000;
        /// Internal hint — covered-up windows are filtered out of
        /// the panel's "currently visible windows" listing. Not
        /// visible to the platform layer.
        const HIDDEN_FROM_PANEL = 0b1_0000_0000;
    }
}

/// Stable process identity. We store only what the rules engine and
/// panel need; no fancy caching.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProcessInfo {
    pub pid: u32,
    pub exe_path: Option<String>,
    pub exe_basename: String,
}

/// A single tracked window. Owned by [`Registry`], accessed through
/// [`WindowSnapshot`] for cheap reads.
#[derive(Debug, Clone)]
pub struct WindowMetadata {
    pub id: WindowId,
    pub process: ProcessInfo,
    pub title: WindowTitle,
    pub class: String,
    pub state: WindowState,
    pub rect: Rect,
    pub workspace: WorkspaceIndex,
    pub monitor: MonitorId,
    pub z_order: u32,
    pub tiled: bool,
}

/// Cheap, copy-friendly snapshot used by readers (panel, tiling).
#[derive(Debug, Clone)]
pub struct WindowSnapshot {
    pub id: WindowId,
    pub pid: u32,
    pub title: WindowTitle,
    pub class: String,
    pub state: WindowState,
    pub rect: Rect,
    pub workspace: WorkspaceIndex,
    pub monitor: MonitorId,
    pub z_order: u32,
}

impl From<&WindowMetadata> for WindowSnapshot {
    fn from(meta: &WindowMetadata) -> Self {
        Self {
            id: meta.id,
            pid: meta.process.pid,
            title: meta.title.clone(),
            class: meta.class.clone(),
            state: meta.state,
            rect: meta.rect,
            workspace: meta.workspace,
            monitor: meta.monitor,
            z_order: meta.z_order,
        }
    }
}

/// A monitor index. We never assume monitor order is stable across
/// hot-plugs; the WM keeps a [`MonitorStore`] that maps stable ids to
/// current screen geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MonitorId(pub u32);

impl MonitorId {
    pub const PRIMARY: MonitorId = MonitorId(0);
    pub fn new(raw: u32) -> Self {
        Self(raw)
    }
}

/// Stable monitor definition persisted across hot-plugs (id by
/// EDID-derived serial, falling back to device path).
#[derive(Debug, Clone)]
pub struct MonitorDef {
    pub id: MonitorId,
    pub friendly_name: String,
    pub rect: Rect,
    pub dpi: u32,
    pub primary: bool,
}

/// Window events. The platform hook serializes these onto the main
/// queue and the manager applies them one by one.
#[derive(Debug, Clone)]
pub enum WindowEvent {
    Created(WindowMetadata),
    Destroyed { id: WindowId },
    Focused { id: WindowId },
    TitleChanged { id: WindowId, title: WindowTitle },
    MovedOrResized { id: WindowId, rect: Rect },
    Minimized { id: WindowId },
    Restored { id: WindowId },
    Maximized { id: WindowId },
    WorkspacedChanged { id: WindowId, from: WorkspaceIndex, to: WorkspaceIndex },
    MonitorChanged { id: WindowId, from: MonitorId, to: MonitorId },
    Hidden { id: WindowId },
    Shown { id: WindowId },
}

/// Owned trait object for "give me the current window list". The
/// panel uses this to read state without holding a lock for the
/// entire paint cycle.
pub trait WindowManager: Send + Sync {
    /// Returns a snapshot of all tracked windows for `workspace`.
    fn snapshot(&self, workspace: WorkspaceIndex) -> Vec<WindowSnapshot>;

    /// Returns a snapshot of the focused window, if any.
    fn focused(&self) -> Option<WindowSnapshot>;

    /// Returns the primary monitor's rect.
    fn primary_monitor(&self) -> Option<MonitorDef>;

    /// Apply a [`WindowEvent`] to the tracker. Called from the main
    /// thread's message loop after the platform hook delivers the
    /// event.
    fn apply(&self, event: WindowEvent);

    /// Replace the monitor store wholesale (hot-plug event).
    fn replace_monitors(&self, monitors: Vec<MonitorDef>);

    /// Mark a window as hidden-from-panel (covered, etc.) — the
    /// panel will hide it from the workspace listing.
    fn mark_hidden_from_panel(&self, id: WindowId, hidden: bool);

    /// Returns `true` if `WindowId` is currently tracked.
    fn contains(&self, hwnd: u64) -> bool {
        self.focused().map(|w| w.id.get() == hwnd).unwrap_or(false)
    }
}

/// Thread-safe registry that backs the [`WindowManager`] trait.
pub struct Registry {
    state: RwLock<RegistryState>,
}

#[derive(Debug, Default)]
struct RegistryState {
    windows: HashMap<WindowId, WindowMetadata>,
    focus: Option<WindowId>,
    z_counter: u32,
    monitors: HashMap<MonitorId, MonitorDef>,
    primary: Option<MonitorId>,
    pending: VecDeque<WindowEvent>,
    last_event_count: u64,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    /// Build an empty registry.
    pub fn new() -> Self {
        Self {
            state: RwLock::new(RegistryState::default()),
        }
    }

    /// Wrap self in an `Arc` so the panel and main thread can share it.
    pub fn into_arc(self) -> Arc<Self> {
        Arc::new(self)
    }

    /// Drain all events that have been applied since the last drain.
    pub fn drain_events(&self) -> Vec<WindowEvent> {
        let mut g = self.state.write();
        let drained: Vec<WindowEvent> = g.pending.drain(..).collect();
        g.last_event_count = g.last_event_count.wrapping_add(drained.len() as u64);
        drained
    }

    /// Read-only snapshot for window `workspace`.
    pub fn snapshot_for(&self, workspace: WorkspaceIndex) -> Vec<WindowSnapshot> {
        self.state
            .read()
            .windows
            .values()
            .filter(|m| !m.state.contains(WindowState::HIDDEN_FROM_PANEL) && m.workspace == workspace)
            .map(WindowSnapshot::from)
            .collect()
    }
}

impl WindowManager for Registry {
    fn snapshot(&self, workspace: WorkspaceIndex) -> Vec<WindowSnapshot> {
        self.snapshot_for(workspace)
    }

    fn focused(&self) -> Option<WindowSnapshot> {
        let g = self.state.read();
        g.focus.and_then(|id| g.windows.get(&id)).map(WindowSnapshot::from)
    }

    fn primary_monitor(&self) -> Option<MonitorDef> {
        let g = self.state.read();
        g.primary.and_then(|id| g.monitors.get(&id)).cloned()
    }

    fn apply(&self, event: WindowEvent) {
        let mut g = self.state.write();
        g.pending.push_back(event.clone());
        match event {
            WindowEvent::Created(meta) => {
                g.z_counter = g.z_counter.wrapping_add(1);
                let mut meta = meta;
                meta.z_order = g.z_counter;
                let id = meta.id;
                g.windows.insert(id, meta);
                if g.focus.is_none() {
                    // Newly-created window becomes the focused one
                    // until told otherwise.
                    g.focus = Some(id);
                }
            }
            WindowEvent::Destroyed { id } => {
                g.windows.remove(&id);
                if g.focus == Some(id) {
                    g.focus = g
                        .windows
                        .values()
                        .max_by_key(|m| m.z_order)
                        .map(|m| m.id);
                }
            }
            WindowEvent::Focused { id } => {
                g.focus = Some(id);
            }
            WindowEvent::TitleChanged { id, title } => {
                if let Some(m) = g.windows.get_mut(&id) {
                    m.title = title;
                }
            }
            WindowEvent::MovedOrResized { id, rect } => {
                if let Some(m) = g.windows.get_mut(&id) {
                    m.rect = rect;
                }
            }
            WindowEvent::Minimized { id } => {
                if let Some(m) = g.windows.get_mut(&id) {
                    m.state.set(WindowState::MINIMIZED, true);
                }
            }
            WindowEvent::Restored { id } => {
                if let Some(m) = g.windows.get_mut(&id) {
                    m.state.set(WindowState::MINIMIZED, false);
                }
            }
            WindowEvent::Maximized { id } => {
                if let Some(m) = g.windows.get_mut(&id) {
                    m.state.set(WindowState::MAXIMIZED, true);
                }
            }
            WindowEvent::WorkspacedChanged { id, to, .. } => {
                if let Some(m) = g.windows.get_mut(&id) {
                    m.workspace = to;
                }
            }
            WindowEvent::MonitorChanged { id, to, .. } => {
                if let Some(m) = g.windows.get_mut(&id) {
                    m.monitor = to;
                }
            }
            WindowEvent::Hidden { id } => {
                if let Some(m) = g.windows.get_mut(&id) {
                    m.state.set(WindowState::VISIBLE, false);
                }
            }
            WindowEvent::Shown { id } => {
                if let Some(m) = g.windows.get_mut(&id) {
                    m.state.set(WindowState::VISIBLE, true);
                }
            }
        }
    }

    fn replace_monitors(&self, monitors: Vec<MonitorDef>) {
        let mut g = self.state.write();
        g.monitors.clear();
        for m in monitors {
            if m.primary {
                g.primary = Some(m.id);
            }
            g.monitors.insert(m.id, m);
        }
    }

    fn mark_hidden_from_panel(&self, id: WindowId, hidden: bool) {
        if let Some(m) = self.state.write().windows.get_mut(&id) {
            m.state.set(WindowState::HIDDEN_FROM_PANEL, hidden);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_meta(id: u64, title: &str, ws: WorkspaceIndex) -> WindowMetadata {
        WindowMetadata {
            id: WindowId::new(id),
            process: ProcessInfo {
                pid: id as u32,
                exe_path: None,
                exe_basename: format!("proc{}.exe", id),
            },
            title: WindowTitle::new(title),
            class: "Foo".into(),
            state: WindowState::VISIBLE,
            rect: Rect::new(0, 0, 100, 100),
            workspace: ws,
            monitor: MonitorId::PRIMARY,
            z_order: 0,
            tiled: false,
        }
    }

    #[test]
    fn registry_tracks_create_destroy() {
        let r = Registry::new();
        r.apply(WindowEvent::Created(make_meta(101, "A", WorkspaceIndex::new_unchecked(1))));
        r.apply(WindowEvent::Created(make_meta(102, "B", WorkspaceIndex::new_unchecked(1))));
        assert_eq!(r.snapshot_for(WorkspaceIndex::new_unchecked(1)).len(), 2);
        r.apply(WindowEvent::Destroyed { id: WindowId::new(101) });
        let snap = r.snapshot_for(WorkspaceIndex::new_unchecked(1));
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].id.get(), 102);
    }

    #[test]
    fn focused_pointer_follows_event_order() {
        let r = Registry::new();
        r.apply(WindowEvent::Created(make_meta(10, "A", WorkspaceIndex::new_unchecked(1))));
        r.apply(WindowEvent::Created(make_meta(20, "B", WorkspaceIndex::new_unchecked(1))));
        r.apply(WindowEvent::Focused { id: WindowId::new(20) });
        assert_eq!(r.focused().unwrap().id.get(), 20);
        // If the focused window is destroyed, focus drops to the
        // highest z-order window.
        r.apply(WindowEvent::Destroyed { id: WindowId::new(20) });
        assert_eq!(r.focused().unwrap().id.get(), 10);
    }

    #[test]
    fn window_id_handles_zero() {
        assert!(WindowId::new(0).is_none());
        assert_eq!(WindowId::new(0), WindowId::NONE);
    }

    #[test]
    fn hidden_from_panel_filters_snapshot() {
        let r = Registry::new();
        r.apply(WindowEvent::Created(make_meta(11, "A", WorkspaceIndex::new_unchecked(1))));
        r.mark_hidden_from_panel(WindowId::new(11), true);
        assert!(r.snapshot_for(WorkspaceIndex::new_unchecked(1)).is_empty());
    }

    #[test]
    fn window_title_truncates_long_input() {
        let long = "x".repeat(800);
        let t = WindowTitle::new(long);
        assert!(t.as_str().chars().count() <= 512);
    }
}
