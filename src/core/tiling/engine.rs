//! The TilingEngine implementation.
//!
//! Owns one [`WorkspaceTreeState`] per (workspace, monitor) pair.
//! Each `recalculate_for_workspace(ws)` pass computes a
//! [`LayoutSolution`] using the tree's LCRS structure plus the
//! [`Gaps`] policy.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::core::tiling::compute_workspace_origin;
use crate::core::tiling::rules::WindowDisposition;
use crate::core::tiling::tree::{
    builder, Direction, SplitBar, SplitNodeId, TreeNode, WindowNode,
};
use crate::core::tiling::{Gaps, LayoutSolution, WorkspaceTreeState};
use crate::core::wm::{MonitorDef, MonitorId, Rect, WindowId, WindowSnapshot};
use crate::core::WorkspaceIndex;

/// Trait-object view used by consumers (panel, hotkey targets).
pub trait TilingEngine: Send + Sync {
    /// Recalculate the layout for a single workspace. Returns the
    /// new `LayoutSolution`.
    fn recalculate_for_workspace(&self, ws: WorkspaceIndex) -> LayoutSolution;

    /// Insert a fresh window into the tiling tree of `ws`.
    fn insert_window(&self, ws: WorkspaceIndex, window: WindowId);

    /// Remove a window from any workspace and re-layout its
    /// origin workspace.
    fn remove_window(&self, window: WindowId);

    /// Update the user's drag of a divider bar. Caller passes the
    /// new ratio (0..=1).
    fn drag_divider(&self, ws: WorkspaceIndex, bar: SplitNodeId, new_ratio: f32);

    /// Adjust gap configuration at runtime.
    fn set_gaps(&self, ws: WorkspaceIndex, gaps: Gaps);

    /// Replace monitor list (hot-plug). All windows on removed
    /// monitors are moved to `primary`.
    fn replace_monitors(&self, monitors: Vec<MonitorDef>);

    /// Returns a snapshot of the current solution for a workspace
    /// (no side effects). Used by the panel to read live geometry
    /// without a recalculation pass.
    fn snapshot(&self, ws: WorkspaceIndex) -> LayoutSolution;
}

/// Concrete implementation. We keep the state in a single
/// `RwLock<EngineState>` so all consumers (`Arc<dyn TilingEngine>`)
/// see consistent state.
pub struct TilingEngineImpl {
    state: RwLock<EngineState>,
    panel_height: i32,
}

#[derive(Debug, Default)]
struct EngineState {
    trees: HashMap<(WorkspaceIndex, MonitorId), WorkspaceTreeState>,
    next_split_id: u64,
    workspace_owners: HashMap<WindowId, (WorkspaceIndex, MonitorId)>,
}

impl TilingEngineImpl {
    /// Build a new engine. `panel_height` is subtracted from the
    /// top of every workspace rect.
    pub fn new(panel_height: i32) -> Self {
        Self {
            state: RwLock::new(EngineState {
                trees: HashMap::new(),
                next_split_id: 1,
                workspace_owners: HashMap::new(),
            }),
            panel_height,
        }
    }

    /// Convert self into an `Arc<dyn TilingEngine>`.
    pub fn into_arc(self) -> Arc<dyn TilingEngine> {
        Arc::new(self)
    }

    fn next_id(&self) -> SplitNodeId {
        let mut g = self.state.write();
        g.next_split_id = g.next_split_id.wrapping_add(1);
        SplitNodeId(g.next_split_id)
    }

    fn get_or_init_tree(
        &self,
        ws: WorkspaceIndex,
        monitor: MonitorId,
        gaps: Gaps,
    ) -> WorkspaceTreeState {
        let mut g = self.state.write();
        g.trees
            .entry((ws, monitor))
            .or_insert_with(|| WorkspaceTreeState::new_with_gaps(gaps))
            .clone()
    }

    /// Run a recompute pass for `(ws, monitor)`. The function picks
    /// the primary monitor if the key is unknown.
    pub fn compute_for(
        &self,
        ws: WorkspaceIndex,
        monitor: MonitorId,
        workspace_rect: Rect,
    ) -> LayoutSolution {
        let mut g = self.state.write();
        let key = (ws, monitor);
        let tree = g.trees.entry(key).or_insert_with(|| {
            WorkspaceTreeState {
                monitor: Some(monitor),
                ..WorkspaceTreeState::new_with_gaps(Gaps::hyprland_defaults())
            }
        });
        tree.monitor = Some(monitor);
        let gaps = tree.gaps;
        let panel_height = self.panel_height;

        // Smart-gaps: only one tiled window → drop outer gaps.
        let effective_outer = if gaps.smart_gaps && tree.has_tiled() && tree.root.as_ref().is_some_and(|r| {
            matches!(r, TreeNode::Window(_))
        }) {
            Gaps {
                outer_top: 0,
                outer_right: 0,
                outer_bottom: 0,
                outer_left: 0,
                inner: gaps.inner,
                smart_gaps: gaps.smart_gaps,
            }
        } else {
            gaps
        };

        let origin = compute_workspace_origin(workspace_rect, &effective_outer, panel_height);

        // DFS-allocate.
        let (mut leaves, mut dividers) = match &tree.root {
            Some(TreeNode::Window(w)) => (vec![(w.id, origin)], Vec::new()),
            Some(TreeNode::Split(s)) => s.layout(origin, gaps.inner),
            None => (Vec::new(), Vec::new()),
        };

        // Floating windows always claim their stored rect (no
        // re-allocation unless explicitly set by the user, which is
        // out of scope for Prompt 2).
        leaves.extend(tree.floats.iter().map(|f| (f.id, f.rect)));
        // Floating windows do not produce SplitBars, hence none added.

        // Sort dividers for stable render order.
        dividers.sort_by_key(|b| (b.rect.x, b.rect.y));

        LayoutSolution {
            windows: leaves,
            dividers,
            workspace_rect: origin,
        }
    }

    /// Validate and emit a SafeEqualGrid fallback for a workspace —
    /// used when the LCRS layout produced something invalid.
    fn safe_equal_grid(
        &self,
        workspace_rect: Rect,
        n: usize,
        panel_height: i32,
    ) -> LayoutSolution {
        let n = n.max(1) as i32;
        let origin = compute_workspace_origin(
            workspace_rect,
            &Gaps::hyprland_defaults(),
            panel_height,
        );
        let per = (origin.width - ((n - 1).max(0) * 6)) / n;
        let mut leaves = Vec::new();
        for i in 0..n {
            let x = origin.x + i * (per + 6);
            leaves.push((
                WindowId::new(0), // placeholder id; caller should remap
                Rect::new(x, origin.y, per.max(0), origin.height),
            ));
        }
        LayoutSolution {
            windows: leaves,
            dividers: Vec::new(),
            workspace_rect: origin,
        }
    }
}

impl TilingEngine for TilingEngineImpl {
    fn recalculate_for_workspace(&self, ws: WorkspaceIndex) -> LayoutSolution {
        let primary = self
            .state
            .read()
            .trees
            .keys()
            .find_map(|(w, m)| if w == &ws { Some(*m) } else { None })
            .unwrap_or(MonitorId::PRIMARY);
        // Caller supplies the monitor rect at layout-application time
        // via `apply_monitors`. We use a 1920x1080 fallback here.
        let fallback = Rect::new(0, 0, 1920, 1080);
        let mut solution = self.compute_for(ws, primary, fallback);
        if !solution.is_valid() {
            tracing::warn!(
                target: "jacquewm.tiling",
                workspace = ws.get(),
                "layout is invalid; falling back to safe equal grid"
            );
            solution = self.safe_equal_grid(fallback, solution.windows.len(), self.panel_height);
        }
        solution
    }

    fn insert_window(&self, ws: WorkspaceIndex, window: WindowId) {
        let id = self.next_id();
        let mut g = self.state.write();
        g.workspace_owners.insert(window, (ws, MonitorId::PRIMARY));
        let tree = g
            .trees
            .entry((ws, MonitorId::PRIMARY))
            .or_insert_with(|| WorkspaceTreeState {
                monitor: Some(MonitorId::PRIMARY),
                ..WorkspaceTreeState::new_with_gaps(Gaps::hyprland_defaults())
            });

        tree.root = match tree.root.take() {
            None => Some(builder::leaf(window)),
            Some(existing) => {
                // Always insert as a vertical split following
                // Hyprland-like default layout: existing goes on the
                // left/top, new on the right/bottom.
                Some(builder::insert_next_to(
                    {
                        match &existing {
                            TreeNode::Window(w) => w.id,
                            TreeNode::Split(s) => s.windows().first().copied().unwrap_or(window),
                        }
                    },
                    window,
                    Direction::Horizontal,
                    id,
                ))
            }
        };
    }

    fn remove_window(&self, window: WindowId) {
        let mut g = self.state.write();
        let owner = g.workspace_owners.remove(&window);
        if let Some((ws, mon)) = owner {
            if let Some(tree) = g.trees.get_mut(&(ws, mon)) {
                if let Some(root) = tree.root.take() {
                    tree.root = collapse_after_remove(root, window);
                }
            }
        }
    }

    fn drag_divider(&self, ws: WorkspaceIndex, bar: SplitNodeId, new_ratio: f32) {
        let mut g = self.state.write();
        let key = (ws, MonitorId::PRIMARY);
        if let Some(tree) = g.trees.get_mut(&key) {
            apply_ratio(tree.root.as_mut(), bar, new_ratio);
        }
    }

    fn set_gaps(&self, ws: WorkspaceIndex, gaps: Gaps) {
        let mut g = self.state.write();
        for tree in g.trees.values_mut() {
            // Apply to every tree on the same workspace across
            // monitors — keeps London-style per-workspace gap policy.
            if let Some(entry) = g.trees.get_mut(&(ws, tree.monitor.unwrap_or(MonitorId::PRIMARY))) {
                entry.gaps = gaps;
            }
        }
    }

    fn replace_monitors(&self, monitors: Vec<MonitorDef>) {
        let mut g = self.state.write();
        // Determine primary.
        let primary_id = monitors
            .iter()
            .find(|m| m.primary)
            .map(|m| m.id)
            .unwrap_or(MonitorId::PRIMARY);

        // Move windows on now-disconnected monitors onto the
        // primary. We do this *conservatively* — we change the
        // monitor on the workspace tree, not the windows
        // themselves.
        let stale: Vec<MonitorId> = g
            .trees
            .keys()
            .map(|(_ws, m)| *m)
            .filter(|m| !monitors.iter().any(|mm| mm.id == *m))
            .collect();
        for m_id in stale {
            let mut to_move: Vec<WindowId> = Vec::new();
            // Collect any windows owned by the stale monitor.
            for (ws, mon) in g.workspace_owners.values() {
                if *mon == m_id {
                    if let Some(tr) = g.trees.get(&(*ws, m_id)) {
                        to_move.extend(tr.root.as_ref().map(|r| r.collect_windows()).unwrap_or_default());
                        to_move.extend(tr.floats.iter().map(|f| f.id));
                    }
                }
            }
            for window in to_move {
                if let Some(slot) = g.workspace_owners.get_mut(&window) {
                    *slot = (slot.0, primary_id);
                }
            }
        }
    }

    fn snapshot(&self, ws: WorkspaceIndex) -> LayoutSolution {
        let monitor = self
            .state
            .read()
            .trees
            .keys()
            .find_map(|(w, m)| if w == &ws { Some(*m) } else { None })
            .unwrap_or(MonitorId::PRIMARY);
        let fallback = Rect::new(0, 0, 1920, 1080);
        self.compute_for(ws, monitor, fallback)
    }
}

/// Promote a single-window-child SplitNode to that window after the
/// sibling was removed.
fn collapse_after_remove(node: TreeNode, removed: WindowId) -> Option<TreeNode> {
    match node {
        TreeNode::Window(w) if w.id == removed => None,
        TreeNode::Window(_) => Some(node),
        TreeNode::Split(s) => {
            let (left_collapsed, right_collapsed) = (
                collapse_after_remove((*s.left).clone(), removed),
                collapse_after_remove((*s.right).clone(), removed),
            );
            match (left_collapsed, right_collapsed) {
                (None, Some(r)) => Some(r),
                (Some(l), None) => Some(l),
                (Some(l), Some(r)) => Some(TreeNode::Split(SplitNode {
                    id: s.id,
                    direction: s.direction,
                    ratio: s.ratio,
                    left: Box::new(l),
                    right: Box::new(r),
                })),
                (None, None) => None,
            }
        }
    }
}

fn apply_ratio(node: Option<&mut TreeNode>, target: SplitNodeId, ratio: f32) {
    if let Some(TreeNode::Split(s)) = node {
        if s.id == target {
            s.set_ratio(ratio);
            return;
        }
        apply_ratio(Some(&mut s.left), target, ratio);
        apply_ratio(Some(&mut s.right), target, ratio);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::wm::WindowId;

    #[test]
    fn insert_first_window_creates_leaf() {
        let engine = TilingEngineImpl::new(34);
        engine.insert_window(WorkspaceIndex::new_unchecked(1), WindowId::new(7));
        let sol = engine.recalculate_for_workspace(WorkspaceIndex::new_unchecked(1));
        assert_eq!(sol.windows.len(), 1);
        assert_eq!(sol.windows[0].0.get(), 7);
        assert!(sol.dividers.iter().any(|b| b.axis == Direction::Vertical));
    }

    #[test]
    fn remove_collapses_split_to_sibling() {
        let engine = TilingEngineImpl::new(34);
        engine.insert_window(WorkspaceIndex::new_unchecked(1), WindowId::new(7));
        engine.insert_window(WorkspaceIndex::new_unchecked(1), WindowId::new(8));
        let sol_before = engine.recalculate_for_workspace(WorkspaceIndex::new_unchecked(1));
        assert_eq!(sol_before.windows.len(), 2);
        engine.remove_window(WindowId::new(8));
        let sol_after = engine.recalculate_for_workspace(WorkspaceIndex::new_unchecked(1));
        assert_eq!(sol_after.windows.len(), 1);
        assert_eq!(sol_after.windows[0].0.get(), 7);
        assert!(sol_after.dividers.is_empty());
    }

    #[test]
    fn replace_monitors_moves_to_primary() {
        let engine = TilingEngineImpl::new(34);
        engine.insert_window(WorkspaceIndex::new_unchecked(1), WindowId::new(1));
        engine.insert_window(WorkspaceIndex::new_unchecked(1), WindowId::new(2));
        engine.replace_monitors(vec![MonitorDef {
            id: MonitorId::PRIMARY,
            friendly_name: "Primary".into(),
            rect: Rect::new(0, 0, 1920, 1080),
            dpi: 96,
            primary: true,
        }]);
        // After monitor replacement, layout still works.
        let sol = engine.recalculate_for_workspace(WorkspaceIndex::new_unchecked(1));
        assert_eq!(sol.windows.len(), 2);
    }

    #[test]
    fn drag_changes_ratio() {
        let engine = TilingEngineImpl::new(34);
        engine.insert_window(WorkspaceIndex::new_unchecked(1), WindowId::new(1));
        engine.insert_window(WorkspaceIndex::new_unchecked(1), WindowId::new(2));
        // Find the bar id.
        let sol_before = engine.recalculate_for_workspace(WorkspaceIndex::new_unchecked(1));
        let bar = sol_before.dividers[0];
        engine.drag_divider(WorkspaceIndex::new_unchecked(1), bar.split_node_id, 0.75);
        let sol_after = engine.recalculate_for_workspace(WorkspaceIndex::new_unchecked(1));
        // First window gets 75% of the width.
        assert!(sol_after.windows[0].1.width > sol_after.windows[1].1.width);
    }
}

// Provide a type alias for the "current window to associate" — let
// the engine caller express intent.
pub trait WindowToAssociate {
    fn workspace(&self) -> WorkspaceIndex;
    fn id(&self) -> WindowId;
}

impl WindowToAssociate for WindowSnapshot {
    fn workspace(&self) -> WorkspaceIndex {
        self.workspace
    }
    fn id(&self) -> WindowId {
        self.id
    }
}

impl WindowToAssociate for (WindowId, WorkspaceIndex) {
    fn workspace(&self) -> WorkspaceIndex {
        self.1
    }
    fn id(&self) -> WindowId {
        self.0
    }
}
