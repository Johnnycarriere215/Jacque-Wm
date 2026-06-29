//! Tiling engine — decides where every managed window lives.
//!
//! Independent from both the [`crate::core::workspaces::WorkspaceEngine`]
//! and [`crate::core::wm::WindowManager`]. The pipeline is:
//!
//! 1. The WM produces `WindowEvent`s.
//! 2. The [`crate::core::apps::ApplicationRulesEngine`] rewrites the
//!    event (float / move / fullscreen) before any layout calculation.
//! 3. [`TilingEngine`] listens to the *post-rules* event stream and
//!    recalculates the affected workspace.
//!
//! Mathematical layout uses the classic left-child-right-sibling tree
//! as in Hyprland / i3, plus a [`SplitBar`] primitive so the user can
//! drag a divider and re-shape the proportion.
//!
//! Floating windows are removed from the tiling tree; their geometry
//! becomes their own record and is not part of the recursive layout
//! pass.

use std::collections::HashMap;

use crate::core::wm::{MonitorId, Rect, WindowId, WindowMetadata, WindowSnapshot};

pub mod engine;
pub mod rules;
pub mod tree;

pub use engine::{TilingEngine, TilingEngineImpl};
pub use rules::{LayoutRule, RuleDecision};
pub use tree::{Direction, SplitBar, SplitNode, TreeNode, WindowNode};

// =====================================================================
// Public geometry helpers
// =====================================================================

/// Recursive layout result — one rect for every leaf plus the
/// `(split_node_index, rect)` for any divider the user can drag.
#[derive(Debug, Clone, Default)]
pub struct LayoutSolution {
    /// Rects for every tiled window on the workspace.
    pub windows: Vec<(WindowId, Rect)>,
    /// Divider handles drawn over the workspace.
    pub dividers: Vec<SplitBar>,
    /// The workspace rectangle the layout was solved for. Useful for
    /// debugging.
    pub workspace_rect: Rect,
}

impl LayoutSolution {
    /// `true` if the layout is valid (no overlaps, no negative area).
    pub fn is_valid(&self) -> bool {
        for (_, r) in &self.windows {
            if !r.is_valid() {
                return false;
            }
        }
        // Cheap overlap check — `O(n^2)` is fine because workspaces
        // top out around 10-30 windows.
        for i in 0..self.windows.len() {
            for j in (i + 1)..self.windows.len() {
                if rects_overlap(self.windows[i].1, self.windows[j].1) {
                    return false;
                }
            }
        }
        true
    }
}

/// Two rectangles overlap iff both axes intersect with positive
/// extent.
pub fn rects_overlap(a: Rect, b: Rect) -> bool {
    a.left() < b.right()
        && a.right() > b.left()
        && a.top() < b.bottom()
        && a.bottom() > b.top()
}

// =====================================================================
// Gaps & smart-gaps policy
// =====================================================================

/// Gap configuration. `outer_gap` is shared between the workspace and
/// the first/recent windows; `inner_gap` is the gap between tiled
/// siblings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gaps {
    pub outer_top: i32,
    pub outer_right: i32,
    pub outer_bottom: i32,
    pub outer_left: i32,
    pub inner: i32,
    /// When only one tiled window exists on a workspace the outer
    /// gaps may be removed.
    pub smart_gaps: bool,
}

impl Default for Gaps {
    fn default() -> Self {
        Self {
            outer_top: 0,
            outer_right: 0,
            outer_bottom: 0,
            outer_left: 0,
            inner: 6,
            // We populate the panel offset deeper in the layout pass
            // so the panel does not occlude the top of windows.
            smart_gaps: true,
        }
    }
}

impl Gaps {
    /// Build the default Hyprland-style gap profile. The panel
    /// layer will subtract its height from the outer top at layout
    /// time.
    pub fn hyprland_defaults() -> Self {
        Self {
            outer_top: 0, // panel handles the top region separately
            outer_right: 6,
            outer_bottom: 6,
            outer_left: 6,
            inner: 6,
            smart_gaps: true,
        }
    }
}

// =====================================================================
// TiledWindow record — what every leaf in the tree holds
// =====================================================================

/// Persistent record of one tiled window. Survives across multiple
/// `LayoutSolution` calculations.
#[derive(Debug, Clone)]
pub struct TiledWindow {
    pub id: WindowId,
    pub rect: Rect,
    pub fullscreen: bool,
}

// =====================================================================
// Per-workspace state stored by the TilingEngine implementation
// =====================================================================

#[derive(Debug, Default)]
pub struct WorkspaceTreeState {
    pub root: Option<TreeNode>,
    pub floats: Vec<TiledWindow>,
    pub gaps: Gaps,
    pub monitor: Option<MonitorId>,
}

impl WorkspaceTreeState {
    pub fn new_with_gaps(gaps: Gaps) -> Self {
        Self {
            root: None,
            floats: Vec::new(),
            gaps,
            monitor: None,
        }
    }

    /// Returns `true` if the workspace currently contains any tiled
    /// leaf node — used by smart-gaps to decide if outer gaps should
    /// collapse.
    pub fn has_tiled(&self) -> bool {
        match &self.root {
            None => false,
            Some(TreeNode::Window(_)) => true,
            Some(TreeNode::Split(s)) => !s.windows().is_empty(),
        }
    }
}

// =====================================================================
// Internal layout helpers exposed for testing
// =====================================================================

/// Compute the rect that a `TreeNode` should occupy given the
/// available `workspace` rect, gaps policy, and current `panel_height`.
///
/// `panel_height` is subtracted from the top so that the topmost
/// panel always wins the top stripe.
pub fn compute_workspace_origin(workspace: Rect, gaps: &Gaps, panel_height: i32) -> Rect {
    // Reserve the top stripe for the panel.
    let after_panel = Rect::new(
        workspace.x,
        workspace.y + panel_height,
        workspace.width,
        (workspace.height - panel_height).max(0),
    );
    Rect::new(
        after_panel.x + gaps.outer_left,
        after_panel.y + gaps.outer_top,
        (after_panel.width - gaps.outer_left - gaps.outer_right).max(0),
        (after_panel.height - gaps.outer_top - gaps.outer_bottom).max(0),
    )
}

/// Allocate a rect for a child by applying `direction` + `ratio` and
/// subtracting `inner_gap` from the shared edge.
///
/// Layout rule: compute one side via `(total * ratio).floor()`, then
/// derive the other as `total - left` to avoid floating-point
/// sub-pixel drift.
pub fn split_rect(parent: Rect, direction: Direction, ratio: f32, inner_gap: i32) -> (Rect, Rect) {
    let total = match direction {
        Direction::Horizontal => (parent.width - inner_gap) as f32,
        Direction::Vertical => (parent.height - inner_gap) as f32,
    };
    let first_side = (total * ratio).floor() as i32;
    match direction {
        Direction::Horizontal => {
            let left = Rect::new(parent.x, parent.y, first_side.max(0), parent.height);
            let right = Rect::new(
                parent.x + first_side + inner_gap,
                parent.y,
                (parent.width - first_side - inner_gap).max(0),
                parent.height,
            );
            (left, right)
        }
        Direction::Vertical => {
            let top = Rect::new(parent.x, parent.y, parent.width, first_side.max(0));
            let bottom = Rect::new(
                parent.x,
                parent.y + first_side + inner_gap,
                parent.width,
                (parent.height - first_side - inner_gap).max(0),
            );
            (top, bottom)
        }
    }
}

// =====================================================================
// Default policy for window assignment
// =====================================================================

/// Decide whether a newly-discovered window should enter the tiling
/// tree, the float pool, or be marked transient.
/// Implemented in [`crate::core::apps`] but the engine consumes the
/// decision so we re-export it.
pub use crate::core::apps::WindowDisposition as Disposition;

/// Engine-side helper: convert a list of tracked windows into the
/// working set used during a single recalculation.
pub fn working_set<'a>(
    snapshots: impl IntoIterator<Item = &'a WindowSnapshot>,
) -> Vec<&'a WindowMetadata> {
    snapshots
        .into_iter()
        .map(|s| WindowMetadata {
            id: s.id,
            process: crate::core::wm::ProcessInfo {
                pid: s.pid,
                exe_path: None,
                exe_basename: String::new(),
            },
            title: s.title.clone(),
            class: s.class.clone(),
            state: s.state,
            rect: s.rect,
            workspace: s.workspace,
            monitor: s.monitor,
            z_order: s.z_order,
            tiled: false,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_overlap_when_split_is_clean() {
        let total = Rect::new(0, 0, 1920, 1080);
        let (l, r) = split_rect(total, Direction::Horizontal, 0.5, 0);
        assert!(!rects_overlap(l, r));
        assert_eq!(l.left(), 0);
        assert_eq!(l.right(), 960);
        assert_eq!(r.left(), 960);
        assert_eq!(r.right(), 1920);
    }

    #[test]
    fn inner_gaps_do_not_leak_pixels() {
        let total = Rect::new(0, 0, 1920, 1080);
        let (l, r) = split_rect(total, Direction::Horizontal, 0.5, 6);
        assert!(!rects_overlap(l, r));
        assert_eq!(l.left(), 0);
        assert_eq!(l.right(), 957);
        assert_eq!(r.left(), 963);
        assert_eq!(r.right(), 1920);
    }

    #[test]
    fn compute_workspace_origin_reserves_panel_stripe() {
        let rect = Rect::new(0, 0, 1920, 1080);
        let out = compute_workspace_origin(rect, &Gaps::hyprland_defaults(), 34);
        assert_eq!(out.x, 6);
        assert_eq!(out.y, 34);
        assert_eq!(out.width, 1908);
        assert_eq!(out.height, 1040);
    }
}
