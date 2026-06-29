//! Animation system used by the panel.
//!
//! Each [`Animation`] runs over a fixed duration from a wall-clock
//! `started_at` time. Easing is one of three curves: linear,
//! ease-in-out (smoothstep), and ease-out (cubic). The renderer
//! calls [`Animation::advance`] once per frame to obtain the
//! current value and a "completed" flag.

use std::time::{Duration, Instant};

/// Easing curve selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Easing {
    Linear,
    EaseInOut,
    EaseOut,
}

impl Easing {
    /// Sample the curve at `t` ∈ [0..=1].
    pub fn sample(self, t: f32) -> f32 {
        match self {
            Easing::Linear => t,
            Easing::EaseInOut => {
                // smoothstep: 3t² - 2t³.
                let t = t.clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            }
            Easing::EaseOut => {
                // 1 - (1 - t)^3.
                let t = t.clamp(0.0, 1.0);
                1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t)
            }
        }
    }
}

/// What the animation is animating. For the panel we only need
/// pill-fade but the enum is open for future expansion.
#[derive(Debug, Clone, Copy)]
pub enum AnimationKind {
    /// Fade the workspace pill at index `pill` from `from` to `to`.
    /// The renderer reads `from`/`to` and the eased progress.
    FadePill {
        /// 0..=8
        pill: usize,
        from: f32,
        to: f32,
    },
    /// Cross-fade the focused window title from one string to
    /// another. The renderer swaps the text after 50% progress.
    FadeTitle,
    /// Slide a new pill in from the right. Currently unused but
    /// reserved.
    SlideInRight,
}

/// One animation record. `started_at` is the wall-clock time which
/// the renderer anchors all easing calculations to.
#[derive(Debug, Clone, Copy)]
pub struct Animation {
    pub kind: AnimationKind,
    pub started_at: Instant,
    pub duration: Duration,
    pub easing: Easing,
}

impl Animation {
    pub fn new(kind: AnimationKind, started_at: Instant, duration: Duration) -> Self {
        Self {
            kind,
            started_at,
            duration,
            easing: Easing::EaseInOut,
        }
    }

    pub fn with_easing(mut self, easing: Easing) -> Self {
        self.easing = easing;
        self
    }

    /// Returns (progress 0..=1, completed). Callers should drop the
    /// animation after `completed == true`.
    pub fn advance(&self, now: Instant) -> (f32, bool) {
        let elapsed = now.saturating_duration_since(self.started_at);
        let total = self.duration.as_nanos() as f32;
        let raw = if total <= 0.0 {
            1.0
        } else {
            (elapsed.as_nanos() as f32 / total).min(1.0)
        };
        let eased = self.easing.sample(raw);
        (eased, raw >= 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_easing_is_identity() {
        assert_eq!(Easing::Linear.sample(0.0), 0.0);
        assert_eq!(Easing::Linear.sample(0.5), 0.5);
        assert_eq!(Easing::Linear.sample(1.0), 1.0);
    }

    #[test]
    fn ease_in_out_smooths_edges() {
        let m = Easing::EaseInOut.sample(0.5);
        assert!((m - 0.5).abs() < 0.001, "smoothstep midpoint should be 0.5");
        assert!(Easing::EaseInOut.sample(0.0) < 0.001);
        assert!(Easing::EaseInOut.sample(1.0) > 0.999);
    }

    #[test]
    fn ease_out_overshoots_neither_side() {
        assert_eq!(Easing::EaseOut.sample(0.0), 0.0);
        assert!(Easing::EaseOut.sample(1.0) >= 0.999);
        assert!(Easing::EaseOut.sample(0.5) > 0.5);
    }
}
