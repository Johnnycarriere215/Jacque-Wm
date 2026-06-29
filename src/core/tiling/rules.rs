//! Layout decisions produced by the rules engine.
//!
//! These are the *engine-side* enums describing what should happen
//! when a new window is presented. The actual rule-matching code
//! lives in [`crate::core::apps`], but the layout engine consumes
//! the decision here.

use crate::core::wm::WindowId;
use crate::core::WorkspaceIndex;

/// What should happen to a window before the layout pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowDisposition {
    /// Window joins the tiled tree at its current workspace.
    Tile,
    /// Window is removed from the tree; the user is free to drag it.
    Float,
    /// Window covers the whole workspace; the existing tiled tree
    /// is preserved in a snapshot for restoration on fullscreen-exit.
    Fullscreen,
    /// Window is moved to a specific workspace then tiled.
    TileOn(WorkspaceIndex),
    /// Window is moved to a specific workspace then floated.
    FloatOn(WorkspaceIndex),
}

/// Plain old data-rule used by the rules engine.
#[derive(Debug, Clone)]
pub enum LayoutRule {
    /// Match by executable basename (case-insensitive, ASCII-folds).
    Basename(String, WindowDisposition),
    /// Match by window class name.
    Class(String, WindowDisposition),
    /// Match by title substring.
    TitleContains(String, WindowDisposition),
    /// Match transient windows (dialogs).
    Transient(WindowDisposition),
    /// Default when no rule matched.
    Default(WindowDisposition),
}

/// Outcome of evaluating one window's rules.
#[derive(Debug, Clone)]
pub struct RuleDecision {
    pub window: WindowId,
    pub disposition: WindowDisposition,
    /// The matched rule, kept for debugging the rules engine.
    pub matched_rule: Option<LayoutRule>,
}

impl RuleDecision {
    /// Convenience: did the rules engine participate?
    pub fn engaged(&self) -> bool {
        self.matched_rule.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_decision_reports_engaged() {
        let d = RuleDecision {
            window: WindowId::new(5),
            disposition: WindowDisposition::Float,
            matched_rule: Some(LayoutRule::Transient(WindowDisposition::Float)),
        };
        assert!(d.engaged());
    }
}
