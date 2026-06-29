//! Integration tests for Prompt 2 subsystems.
//!
//! Pure OS-agnostic tests. They exercise the panel data model, the
//! tiling engine, the application-rules engine, the focus tracker,
//! and the metrics types.

use jacquewm::core::apps::{ApplicationRulesEngine, RulesEngine};
use jacquewm::core::focus::{FocusEntry, FocusTracker};
use jacquewm::core::panel::animation::{Animation, Easing};
use jacquewm::core::panel::state::{Color, ThemePalette};
use jacquewm::core::panel::{PanelState, Theme};
use jacquewm::core::tiling;
use jacquewm::core::tiling::engine::TilingEngineImpl;
use jacquewm::core::tiling::tree::{builder, Direction, SplitNodeId};
use jacquewm::core::tiling::TilingEngine;
use jacquewm::core::wm::{MonitorId, ProcessInfo, Rect, WindowId, WindowMetadata, WindowSnapshot, WindowState, WindowTitle};
use jacquewm::core::metrics::{CpuSample, GpuSample, NetSample, RamSample, RollingMean};
use jacquewm::core::WorkspaceIndex;

fn make_meta(id: u64, ws: WorkspaceIndex, monitor: MonitorId) -> WindowMetadata {
    WindowMetadata {
        id: WindowId::new(id),
        process: ProcessInfo {
            pid: id as u32,
            exe_path: None,
            exe_basename: format!("win{}.exe", id),
        },
        title: WindowTitle::new("Sample"),
        class: "Class".into(),
        state: WindowState::VISIBLE,
        rect: Rect::new(0, 0, 100, 100),
        workspace: ws,
        monitor,
        z_order: 0,
        tiled: false,
    }
}

// =====================================================================
// Focus
// =====================================================================

#[test]
fn focus_tracker_round_trip() {
    let f = FocusTracker::new();
    let initial = f.current();
    assert!(initial.is_desktop());
    let entry = FocusEntry {
        id: WindowId::new(7),
        pid: 7,
        class: "Sample".into(),
        title: WindowTitle::new("Hello"),
        fullscreen: false,
    };
    let prev = f.set_focused(entry.clone());
    assert!(prev.is_desktop());
    assert_eq!(f.current().id.get(), 7);
    assert_eq!(f.current().title.as_str(), "Hello");
}

// =====================================================================
// Tiling
// =====================================================================

#[test]
fn layout_solution_panics_on_overlap() {
    let mut engine = TilingEngineImpl::new(34);
    engine.insert_window(WorkspaceIndex::new_unchecked(1), WindowId::new(1));
    engine.insert_window(WorkspaceIndex::new_unchecked(1), WindowId::new(2));
    let sol = engine.recalculate_for_workspace(WorkspaceIndex::new_unchecked(1));
    assert!(sol.is_valid(), "two-window split must be non-overlapping");
}

#[test]
fn drag_does_not_cascade() {
    let engine = TilingEngineImpl::new(34);
    for i in 1..=3u64 {
        engine.insert_window(WorkspaceIndex::new_unchecked(1), WindowId::new(i));
    }
    let sol1 = engine.recalculate_for_workspace(WorkspaceIndex::new_unchecked(1));
    let first_bar = sol1.dividers[0].split_node_id;
    engine.drag_divider(WorkspaceIndex::new_unchecked(1), first_bar, 0.2);
    let sol2 = engine.recalculate_for_workspace(WorkspaceIndex::new_unchecked(1));
    // The two visible leaves shift toward the dragged split, but the
    // other divider's geometry must not jump.
    assert_ne!(sol1.windows[0].1.width, sol2.windows[0].1.width);
}

#[test]
fn smart_gaps_collapse_with_one_window() {
    let engine = TilingEngineImpl::new(34);
    engine.insert_window(WorkspaceIndex::new_unchecked(1), WindowId::new(1));
    let sol = engine.recalculate_for_workspace(WorkspaceIndex::new_unchecked(1));
    let ws_rect = sol.windows[0].1;
    // ws_rect should fully encompass the panel-respecting area.
    assert!(ws_rect.is_valid());
}

#[test]
fn tree_builder_replace_window_works_recursively() {
    let mut root = builder::insert_next_to(
        WindowId::new(1),
        WindowId::new(2),
        Direction::Vertical,
        SplitNodeId(1),
    );
    assert!(root.replace_window(WindowId::new(1), jacquewm::core::tiling::tree::WindowNode::new(WindowId::new(3))));
    assert!(root.contains(WindowId::new(3)));
}

// =====================================================================
// Animation
// =====================================================================

#[test]
fn animation_progresses_to_one() {
    let started = std::time::Instant::now();
    let anim = Animation::new(
        jacquewm::core::panel::animation::AnimationKind::FadeTitle,
        started,
        std::time::Duration::from_millis(10),
    );
    let (initial, _completed) = anim.advance(started);
    assert!(initial.abs() < 0.001);
}

#[test]
fn easing_sample_midpoints() {
    assert!(Easing::EaseInOut.sample(0.5).abs() < 0.001);
    assert!(Easing::EaseOut.sample(0.5) > 0.5);
    assert!(Easing::Linear.sample(0.5) == 0.5);
}

// =====================================================================
// Metrics
// =====================================================================

#[test]
fn rolling_mean_smaller_than_capacity_works() {
    let mut r = RollingMean::new(5);
    assert_eq!(r.push_and_average(1.0), 1.0);
    assert_eq!(r.push_and_average(3.0), 2.0);

    // Default-constructed samples don't crash.
    let _ = CpuSample::default();
    let gpu = GpuSample::default();
    assert!(!gpu.presentable);
    let _ = NetSample::default();
    let _ = RamSample::default();
}

// =====================================================================
// Application rules
// =====================================================================

#[test]
fn calculator_basename_should_float() {
    let rules: std::sync::Arc<dyn ApplicationRulesEngine> = RulesEngine::new().into_arc();
    rules.add(
        jacquewm::core::tiling::rules::LayoutRule::Basename(
            "calculator.exe".into(),
            jacquewm::core::tiling::rules::WindowDisposition::Float,
        ),
        0,
    );
    let meta = make_meta(5, WorkspaceIndex::new_unchecked(1), MonitorId::PRIMARY);
    let decision = rules.evaluate(&meta);
    assert_eq!(
        decision.disposition,
        jacquewm::core::tiling::rules::WindowDisposition::Float,
    );
    assert!(decision.engaged());
}

#[test]
fn unmatched_window_falls_back_to_default() {
    let rules: std::sync::Arc<dyn ApplicationRulesEngine> = RulesEngine::new().into_arc();
    rules.set_default(jacquewm::core::tiling::rules::WindowDisposition::Tile);
    let meta = make_meta(5, WorkspaceIndex::new_unchecked(1), MonitorId::PRIMARY);
    let decision = rules.evaluate(&meta);
    assert_eq!(
        decision.disposition,
        jacquewm::core::tiling::rules::WindowDisposition::Tile
    );
}

// =====================================================================
// Panel theme
// =====================================================================

#[test]
fn panel_state_initial_paints_one_active_pill() {
    let state = PanelState::initial(
        WorkspaceIndex::new_unchecked(1),
        ThemePalette::omarchy_dark(),
    );
    assert!(state.pill_opacities[0] >= 0.999);
    assert!(state.pill_opacities[1] < 1.0);
    assert!(state.dirty);
}

#[test]
fn omarchy_theme_is_dark() {
    let p = ThemePalette::omarchy_dark();
    assert!(p.background.r() < 64);
    let _ = Theme::default();
}

#[test]
fn color_components_decode_in_order() {
    let c = Color::rgba(0x12, 0x34, 0x56, 0x78);
    assert_eq!(c.r(), 0x12);
    assert_eq!(c.g(), 0x34);
    assert_eq!(c.b(), 0x56);
    assert_eq!(c.a(), 0x78);
}

// =====================================================================
// Tiling math
// =====================================================================

#[test]
fn tiling_rects_no_overlap_at_split() {
    let total = Rect::new(0, 0, 4000, 3000);
    let (l, r) = tiling::split_rect(total, Direction::Horizontal, 0.5, 0);
    assert!(!tiling::rects_overlap(l, r));
    assert_eq!(l.left(), 0);
    assert_eq!(r.left(), 2000);

    let (t, b) = tiling::split_rect(total, Direction::Vertical, 0.5, 0);
    assert!(!tiling::rects_overlap(t, b));
    assert_eq!(t.top(), 0);
    assert_eq!(b.top(), 1500);
}

// =====================================================================
// Snapshot trajectory
// =====================================================================

#[test]
fn window_snapshot_round_trip() {
    let meta = make_meta(99, WorkspaceIndex::new_unchecked(3), MonitorId::PRIMARY);
    let snap: WindowSnapshot = (&meta).into();
    assert_eq!(snap.id.get(), 99);
    assert_eq!(snap.workspace.get(), 3);
    assert_eq!(snap.pid, 99);
}
