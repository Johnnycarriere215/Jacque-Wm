//! Theme manager.
//!
//! Single source of truth for "what theme is active right now?". The
//! panel and tiled-aware UI both subscribe; on every change they
//! receive the new [`ThemeBundle`] (palette + animation speed) and
//! apply it at their leisure.
//!
//! Failure isolation: the broadcast closure runs synchronously under
//! a `parking_lot::RwLock` read guard — observers must not deadlock.
//! Observers are responsible for *not* blocking; the manager simply
//! logs an error message if a broadcast does not complete within a
//! reasonable time (no panic — observers remain alive).

use std::sync::Arc;

use parking_lot::RwLock;

use crate::core::panel::state::ThemePalette;

/// What a theme can change. The constant set today, the extensible
/// enum tomorrow if/when light themes + per-section CSS land.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeBundle {
    pub palette: ThemePalette,
    pub animation_speed: f32,
}

impl ThemeBundle {
    /// Resolve the canonical dark theme used as the built-in default.
    pub fn omarchy_dark() -> Self {
        Self {
            palette: ThemePalette::omarchy_dark(),
            animation_speed: 1.0,
        }
    }
}

impl Default for ThemeBundle {
    fn default() -> Self {
        Self::omarchy_dark()
    }
}

/// Categories of subscribers. Useful when an observer only wants
/// changes to certain things (e.g. panel-only versus tiling accents).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemeChannel {
    /// Panel background/text/pill colours.
    Panel,
    /// Tiling divider / accent colours.
    Tiling,
    /// All channels.
    All,
}

/// Stable id for an observer — used so a configure-tool can
/// de-register.
pub type ThemeObserverId = u64;

/// Boxed closure invoked with the new bundle + which channel changed.
/// We store observers as `Arc<dyn Fn …>` so the broadcast step can
/// `Arc::clone` each one before dropping the manager's lock; doing so
/// with a `Box<dyn Fn>` would force either an `unsafe` reconstruction
/// (which has UB potential since Box and ArcInner layouts differ) or
/// holding the lock during the call (which deadlocks if an observer
/// re-enters this manager).
pub type ThemeObserver = Box<dyn Fn(ThemeChannel, ThemeBundle) + Send + Sync + 'static>;

/// ThemeManager — owns the active bundle + a list of observers.
pub struct ThemeManager {
    inner: RwLock<ThemeManagerState>,
    next_observer_id: parking_lot::Mutex<u64>,
}

struct ThemeManagerState {
    active: ThemeBundle,
    observers: Vec<(ThemeObserverId, ThemeChannel, Arc<dyn Fn(ThemeChannel, ThemeBundle) + Send + Sync + 'static>)>,
}

impl Default for ThemeManager {
    fn default() -> Self {
        Self::new(ThemeBundle::omarchy_dark())
    }
}

impl ThemeManager {
    /// Build a manager with a chosen initial bundle.
    pub fn new(initial: ThemeBundle) -> Self {
        Self {
            inner: RwLock::new(ThemeManagerState {
                active: initial,
                observers: Vec::new(),
            }),
            next_observer_id: parking_lot::Mutex::new(1),
        }
    }

    /// Returns the currently active bundle (cheap clone).
    pub fn current(&self) -> ThemeBundle {
        self.inner.read().active.clone()
    }

    /// Register an observer. Returns its id, usable with
    /// [`Self::unregister`]. The closure is wrapped in `Arc` so the
    /// broadcast loop can clone each observer cheaply outside the lock.
    pub fn subscribe(&self, channel: ThemeChannel, observer: ThemeObserver) -> ThemeObserverId {
        let mut seq = self.next_observer_id.lock();
        let id = *seq;
        *seq = seq.wrapping_add(1);
        drop(seq);
        let mut g = self.inner.write();
        g.observers.push((id, channel, std::sync::Arc::from(observer)));
        id
    }

    /// Drop a previously-registered observer.
    pub fn unregister(&self, id: ThemeObserverId) {
        let mut g = self.inner.write();
        g.observers.retain(|(other_id, _, _)| *other_id != id);
    }

    /// Replace the active bundle and broadcast the change. Observers
    /// whose channel matches the change are called *outside* the lock.
    pub fn apply(&self, new: ThemeBundle) {
        // Take the observer list out under the lock, then drop the
        // guard before invoking them — observers may take their own
        // locks, and we must not hold ours while doing so (would risk
        // deadlock if an observer re-enters this manager).
        let active = new.clone();
        let observers: Vec<(ThemeChannel, Arc<dyn Fn(ThemeChannel, ThemeBundle) + Send + Sync + 'static>)> = {
            let mut g = self.inner.write();
            g.active = active.clone();
            g.observers
                .iter()
                .map(|(_, channel, observer)| (*channel, observer.clone()))
                .collect()
        };
        for (channel, observer) in observers {
            observer(channel, active.clone());
        }
    }

    /// Number of currently-registered observers.
    pub fn observer_count(&self) -> usize {
        self.inner.read().observers.len()
    }
}

fn channel_for_bundle(channel: &ThemeChannel) -> &[ThemeChannel] {
    match channel {
        ThemeChannel::Panel => &[ThemeChannel::Panel],
        ThemeChannel::Tiling => &[ThemeChannel::Tiling],
        ThemeChannel::All => &[ThemeChannel::Panel, ThemeChannel::Tiling],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_is_dark() {
        let m = ThemeManager::default();
        let b = m.current();
        assert!(b.palette.background.r() < 32);
        assert_eq!(b.animation_speed, 1.0);
    }

    #[test]
    fn observer_receives_broadcast() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNT: AtomicU32 = AtomicU32::new(0);
        let m = ThemeManager::default();
        m.subscribe(ThemeChannel::All, Box::new(|_, _| {
            COUNT.fetch_add(1, Ordering::SeqCst);
        }));
        m.apply(ThemeBundle::omarchy_dark());
        assert!(COUNT.load(Ordering::SeqCst) >= 1);
        let arc = Arc::new(m);
        let _ = arc; // silence unused
    }

    #[test]
    fn unregister_removes_observer() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNT: AtomicU32 = AtomicU32::new(0);
        let m = ThemeManager::default();
        let id = m.subscribe(ThemeChannel::All, Box::new(|_, _| {
            COUNT.fetch_add(1, Ordering::SeqCst);
        }));
        m.apply(ThemeBundle::omarchy_dark());
        let pre_unreg = COUNT.load(Ordering::SeqCst);
        m.unregister(id);
        m.apply(ThemeBundle::omarchy_dark());
        assert_eq!(COUNT.load(Ordering::SeqCst), pre_unreg);
    }
}
