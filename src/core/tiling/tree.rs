//! LCRS tree data structure for tiling.
//!
//! Same shape as Hyprland / i3 / AwesomeWM — a `TreeNode` is either a
//! `Window` leaf holding an `WindowId`, or a `Split` internal node
//! holding a [`Direction`], a `ratio`, and two children.
//!
//! Splitting always inserts a new `SplitNode` between the focused
//! window and its sibling; the original window moves to one side and
//! the new window takes the other side. The user's draggable
//! divider corresponds to the [`SplitBar`] emitted during the DFS
//! rect-allocation pass.

use crate::core::wm::{Rect, WindowId};
use crate::core::tiling::split_rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Two children side-by-side. `ratio` is the *left* share.
    Horizontal,
    /// Two children stacked vertically. `ratio` is the *top* share.
    Vertical,
}

impl Default for Direction {
    fn default() -> Self {
        Direction::Horizontal
    }
}

/// Draggable divider between two tiles.
#[derive(Debug, Clone, Copy)]
pub struct SplitBar {
    /// Identifies the SplitNode that owns this bar.
    pub split_node_id: SplitNodeId,
    /// Coordinates the bar occupies. The bar is `bar_thickness_px`
    /// wide along the bar's axis.
    pub rect: Rect,
    /// The drag axis. Horizontal bar ⇒ Vertical bar, etc.
    pub axis: Direction,
}

/// Stable identifier for a `SplitNode` within a single workspace tree.
/// Used by the panel's divider widget to talk to the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SplitNodeId(pub u64);

impl SplitNodeId {
    pub const NONE: SplitNodeId = SplitNodeId(0);
}

/// One branch in the tiling tree.
#[derive(Debug, Clone)]
pub enum TreeNode {
    /// Internal node: direction + ratio + two children.
    Split(SplitNode),
    /// Leaf node: a real Win32 window.
    Window(WindowNode),
}

impl TreeNode {
    /// Dispatch a WindowId match — returns `true` (and replaces) if
    /// the tree contains the id as a leaf.
    pub fn replace_window(&mut self, old: WindowId, new: WindowNode) -> bool {
        match self {
            TreeNode::Window(w) if w.id == old => {
                *self = TreeNode::Window(new);
                true
            }
            TreeNode::Window(_) => false,
            TreeNode::Split(s) => {
                s.left.replace_window(old, new.clone())
                    || s.right.replace_window(old, new)
            }
        }
    }

    /// Returns `true` if `id` is in the tree.
    pub fn contains(&self, id: WindowId) -> bool {
        match self {
            TreeNode::Window(w) => w.id == id,
            TreeNode::Split(s) => s.left.contains(id) || s.right.contains(id),
        }
    }

    /// Collect all leaf `WindowId`s in DFS order.
    pub fn collect_windows(&self) -> Vec<WindowId> {
        let mut out = Vec::new();
        fn visit(node: &TreeNode, out: &mut Vec<WindowId>) {
            match node {
                TreeNode::Window(w) => out.push(w.id),
                TreeNode::Split(s) => {
                    visit(&s.left, out);
                    visit(&s.right, out);
                }
            }
        }
        visit(self, &mut out);
        out
    }

    /// Number of leaves.
    pub fn leaf_count(&self) -> usize {
        match self {
            TreeNode::Window(_) => 1,
            TreeNode::Split(s) => s.left.leaf_count() + s.right.leaf_count(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SplitNode {
    pub id: SplitNodeId,
    pub direction: Direction,
    /// 0.5 = equal; near 0 = first child is thin; near 1 = second
    /// child is thin. Always clamped to `(0.05..=0.95)` to avoid
    /// degenerate splits.
    pub ratio: f32,
    pub left: Box<TreeNode>,
    pub right: Box<TreeNode>,
}

impl SplitNode {
    pub fn new(id: SplitNodeId, direction: Direction, left: TreeNode, right: TreeNode) -> Self {
        Self {
            id,
            direction,
            ratio: 0.5,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    /// DFS-allocate a rect to every leaf and emit [`SplitBar`]s.
    ///
    /// Returns `(leaves, dividers)` so callers can hand them to the
    /// panel and window-positioning layer.
    pub fn layout(&self, rect: Rect, inner_gap: i32) -> (Vec<(WindowId, Rect)>, Vec<SplitBar>) {
        let (l, r) = split_rect(rect, self.direction, self.ratio, inner_gap);
        let (mut out_l, mut bars_l) = match &*self.left {
            TreeNode::Window(w) => (vec![(w.id, l)], Vec::new()),
            TreeNode::Split(s) => s.layout(l, inner_gap),
        };
        let (out_r, bars_r) = match &*self.right {
            TreeNode::Window(w) => (vec![(w.id, r)], Vec::new()),
            TreeNode::Split(s) => s.layout(r, inner_gap),
        };
        out_l.extend(out_r);

        // Build a divider that runs along the shared edge between
        // `l` and `r`. For horizontal splits the divider is vertical;
        // for vertical splits the divider is horizontal.
        let thickness = 8;
        let divider = match self.direction {
            Direction::Horizontal => SplitBar {
                split_node_id: self.id,
                rect: Rect::new(l.right(), rect.y, thickness, rect.height),
                axis: Direction::Vertical,
            },
            Direction::Vertical => SplitBar {
                split_node_id: self.id,
                rect: Rect::new(rect.x, l.bottom(), rect.width, thickness),
                axis: Direction::Horizontal,
            },
        };
        let mut bars = Vec::with_capacity(2);
        bars.extend(bars_l);
        bars.extend(bars_r);
        bars.push(divider);
        (out_l, bars)
    }

    /// Resize by setting `new_ratio` clamped to safe bounds.
    pub fn set_ratio(&mut self, new_ratio: f32) {
        self.ratio = new_ratio.clamp(0.05, 0.95);
    }

    /// First leaf DFS-order on the `left` side. Helper for new-window
    /// insertion logic.
    pub fn windows(&self) -> Vec<WindowId> {
        self.left.collect_windows()
            .into_iter()
            .chain(self.right.collect_windows())
            .collect()
    }
}

/// A window leaf.
#[derive(Debug, Clone)]
pub struct WindowNode {
    pub id: WindowId,
    pub transient_for: Option<WindowId>,
    pub previous_rect: Option<Rect>,
}

impl WindowNode {
    pub fn new(id: WindowId) -> Self {
        Self {
            id,
            transient_for: None,
            previous_rect: None,
        }
    }
}

// =====================================================================
// Builder API used by the TilingEngine impl
// =====================================================================

/// Helpers for inserting and removing nodes. The tiling engine uses
/// these helpers to keep tree mutation patterns in one place.
pub mod builder {
    use super::*;

    /// Build a tree containing a single `WindowNode`.
    pub fn leaf(id: WindowId) -> TreeNode {
        TreeNode::Window(WindowNode::new(id))
    }

    /// Insert `new` next to `existing` along `direction`. The new
    /// window gets the "second" child slot; the existing window
    /// becomes the "first" child. Returns the new SplitNode id.
    pub fn insert_next_to(
        existing: WindowId,
        new: WindowId,
        direction: Direction,
        next_id: SplitNodeId,
    ) -> TreeNode {
        TreeNode::Split(SplitNode::new(
            next_id,
            direction,
            TreeNode::Window(WindowNode::new(existing)),
            TreeNode::Window(WindowNode::new(new)),
        ))
    }

    /// Replace a leaf with a deeper tree (used when promoting one
    /// window to a sub-split).
    pub fn replace_and_split(
        existing: WindowId,
        new: WindowId,
        direction: Direction,
        next_id: SplitNodeId,
    ) -> TreeNode {
        insert_next_to(existing, new, direction, next_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::wm::Rect;
    use crate::core::WorkspaceIndex;

    #[test]
    fn simple_split_lays_two_windows_out() {
        let root = builder::insert_next_to(
            WindowId::new(1),
            WindowId::new(2),
            Direction::Horizontal,
            SplitNodeId(1),
        );
        let (leaves, bars) = root.layout(Rect::new(0, 0, 1920, 1080), 0);
        assert_eq!(leaves.len(), 2);
        assert_eq!(leaves[0], (WindowId::new(1), Rect::new(0, 0, 960, 1080)));
        assert_eq!(leaves[1], (WindowId::new(2), Rect::new(960, 0, 960, 1080)));
        // One divider produced for one split.
        assert_eq!(bars.len(), 1);
    }

    #[test]
    fn replaces_window_in_place() {
        let mut root = builder::insert_next_to(
            WindowId::new(1),
            WindowId::new(2),
            Direction::Vertical,
            SplitNodeId(1),
        );
        assert!(root.replace_window(WindowId::new(2), WindowNode::new(WindowId::new(3))));
        assert!(root.contains(WindowId::new(3)));
        assert!(!root.contains(WindowId::new(2)));
        assert_eq!(root.leaf_count(), 2);
    }

    // Touch WorkspaceIndex so the import brings it into the test
    // crate and the type alias test path compiles.
    #[allow(dead_code)]
    fn _typecheck(_w: WorkspaceIndex) {}
}
