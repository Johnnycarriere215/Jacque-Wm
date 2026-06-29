//! Application rules engine.
//!
//! Re-exported here as [`WindowDisposition`] consumed by the tiling
//! engine. The concrete rule evaluator lives in [`crate::platform`]
//! implementations; this module provides the trait and a default
//! in-memory rule list.
//!
//! Rule execution model:
//!
//! 1. The WM produces a [`WindowEvent::Created`].
//! 2. The rules engine looks up the process info (exe + class).
//! 3. The first matching rule's [`WindowDisposition`] is applied.
//! 4. If nothing matches, the default disposition is used.
//!
//! Per the spec — *flag applications can be advanced later.* This
//! implementation supports an in-memory rule list for now.

use std::sync::Arc;

use parking_lot::RwLock;

use crate::core::tiling::rules::{LayoutRule, RuleDecision, WindowDisposition};
use crate::core::wm::{WindowId, WindowMetadata, WindowSnapshot};

/// Trait-object view used by the tiling engine.
pub trait ApplicationRulesEngine: Send + Sync {
    /// Resolve a disposition for `window`. Returns the matched rule
    /// (for diagnostics) as well as the disposition.
    fn evaluate(&self, window: &WindowMetadata) -> RuleDecision;

    /// Add a new rule. Future matches use `order` to determine
    /// precedence: lower order values run first.
    fn add(&self, rule: LayoutRule, order: i32);

    /// Returns the rules in evaluation order (read-only).
    fn rules(&self) -> Vec<(i32, LayoutRule)>;

    /// Replace the entire rule set.
    fn replace(&self, rules: Vec<(i32, LayoutRule)>);

    /// Replace the default disposition (the rule applied when
    /// nothing else matches).
    fn set_default(&self, d: WindowDisposition);
}

/// In-memory rule evaluator.
pub struct RulesEngine {
    inner: RwLock<RulesState>,
}

#[derive(Debug, Default)]
struct RulesState {
    rules: Vec<(i32, LayoutRule)>,
    default: WindowDisposition,
}

impl Default for RulesEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl RulesEngine {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(RulesState {
                // Default Hyprland-like behaviour: float dialogs and
                // transient windows, tile everything else.
                rules: vec![
                    (
                        -100,
                        LayoutRule::Transient(WindowDisposition::Float),
                    ),
                ],
                default: WindowDisposition::Tile,
            }),
        }
    }

    pub fn into_arc(self) -> Arc<dyn ApplicationRulesEngine> {
        Arc::new(self)
    }
}

impl ApplicationRulesEngine for RulesEngine {
    fn evaluate(&self, window: &WindowMetadata) -> RuleDecision {
        let g = self.inner.read();
        let mut sorted: Vec<(i32, &LayoutRule)> =
            g.rules.iter().map(|(o, r)| (*o, r)).collect();
        sorted.sort_by_key(|(o, _)| *o);

        for (_o, rule) in sorted {
            if let Some(disp) = match_rule(rule, window) {
                return RuleDecision {
                    window: window.id,
                    disposition: disp,
                    matched_rule: Some(rule.clone()),
                };
            }
        }
        RuleDecision {
            window: window.id,
            disposition: g.default,
            matched_rule: Some(LayoutRule::Default(g.default)),
        }
    }

    fn add(&self, rule: LayoutRule, order: i32) {
        let mut g = self.inner.write();
        g.rules.push((order, rule));
        g.rules.sort_by_key(|(o, _)| *o);
    }

    fn rules(&self) -> Vec<(i32, LayoutRule)> {
        self.inner.read().rules.clone()
    }

    fn replace(&self, rules: Vec<(i32, LayoutRule)>) {
        let mut g = self.inner.write();
        g.rules = rules;
        g.rules.sort_by_key(|(o, _)| *o);
    }

    fn set_default(&self, d: WindowDisposition) {
        self.inner.write().default = d;
    }
}

fn match_rule(rule: &LayoutRule, meta: &WindowMetadata) -> Option<WindowDisposition> {
    match rule {
        LayoutRule::Basename(needle, disp) => {
            if meta
                .process
                .exe_basename
                .eq_ignore_ascii_case(needle)
            {
                Some(*disp)
            } else {
                None
            }
        }
        LayoutRule::Class(needle, disp) => {
            if meta.class.eq_ignore_ascii_case(needle) {
                Some(*disp)
            } else {
                None
            }
        }
        LayoutRule::TitleContains(needle, disp) => {
            if meta.title.as_str().contains(needle) {
                Some(*disp)
            } else {
                None
            }
        }
        LayoutRule::Transient(disp) => {
            // We don't yet track transients in WindowMetadata. The
            // platform layer signals transients via WindowEvent; the
            // engine here returns *maybe* but only if the metadata
            // already encodes a transient hint.
            if meta.class.is_empty() {
                Some(*disp)
            } else {
                None
            }
        }
        LayoutRule::Default(disp) => Some(*disp),
    }
}

// =====================================================================
// Convenience: turn a WindowSnapshot into WindowMetadata for the rule
// evaluator.
// =====================================================================

impl From<&WindowSnapshot> for WindowMetadata {
    fn from(snap: &WindowSnapshot) -> Self {
        WindowMetadata {
            id: snap.id,
            process: crate::core::wm::ProcessInfo {
                pid: snap.pid,
                exe_path: None,
                exe_basename: String::new(),
            },
            title: snap.title.clone(),
            class: snap.class.clone(),
            state: snap.state,
            rect: snap.rect,
            workspace: snap.workspace,
            monitor: snap.monitor,
            z_order: snap.z_order,
            tiled: bool::default(),
        }
    }
}

/// Trait-object wrapper of the engine by id. Useful when callers
/// don't want to hold the trait object directly but need an
/// id-indexed evaluator.
pub trait DecisionLookup {
    fn id(&self) -> WindowId;
}

impl DecisionLookup for (&WindowId, &RuleDecision) {
    fn id(&self) -> WindowId {
        *self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::wm::{ProcessInfo, WindowMetadata, WindowState};
    use crate::core::WorkspaceIndex;

    fn meta(name: &str, class: &str) -> WindowMetadata {
        WindowMetadata {
            id: WindowId::new(99),
            process: ProcessInfo {
                pid: 1,
                exe_path: None,
                exe_basename: name.into(),
            },
            title: crate::core::wm::WindowTitle::new("Untitled"),
            class: class.into(),
            state: WindowState::VISIBLE,
            rect: crate::core::wm::Rect::new(0, 0, 100, 100),
            workspace: WorkspaceIndex::new_unchecked(1),
            monitor: crate::core::wm::MonitorId::PRIMARY,
            z_order: 0,
            tiled: false,
        }
    }

    #[test]
    fn default_tile_when_no_rule_matches() {
        let e = RulesEngine::new();
        let d = e.evaluate(&meta("code.exe", "Window"));
        assert_eq!(d.disposition, WindowDisposition::Tile);
    }

    #[test]
    fn basename_match_floats_calculator() {
        let e = RulesEngine::new();
        e.add(
            LayoutRule::Basename("calculator.exe".into(), WindowDisposition::Float),
            0,
        );
        let d = e.evaluate(&meta("Calculator.exe", "Calc"));
        assert_eq!(d.disposition, WindowDisposition::Float);
        assert!(d.engaged());
    }
}
