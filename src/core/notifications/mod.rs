//! In-app notifications.
//!
//! Spec says: "Toast notifications, low visual noise, auto-dismiss
//! after configurable time, stacking multiple notifications cleanly,
//! no animation overload, no flashing, no sound requirement by
//! default." — We therefore use lightweight in-app popup windows
//! (designed to look the same as the panel) instead of Win10's
//! toast system, which would override the user's existing toast
//! preferences and break the "never override system UI" rule.
//!
//! The core exposes a `NotificationManager` trait object used by the
//! rest of the codebase; the platform layer implements it with a
//! small Win32 popup window.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

/// User-visible severity. Drives colour only — no sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    /// Standard informational — grey.
    Info,
    /// Success — soft green accent.
    Success,
    /// Warning — soft amber accent.
    Warning,
    /// Failure — soft red accent.
    Error,
}

impl Severity {
    /// Glyph used in the popup corner.
    pub fn glyph(self) -> &'static str {
        match self {
            Severity::Info => "·",
            Severity::Success => "✓",
            Severity::Warning => "!",
            Severity::Error => "✗",
        }
    }
}

/// Caller-supplied request describing one toast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationRequest {
    /// Title bar — short, ≤40 chars.
    pub title: String,
    /// Body — ≤200 chars or so. The platform layer truncates further
    /// at render time.
    pub body: String,
    pub severity: Severity,
    /// Optional auto-dismiss override (ms). `None` = use manager's
    /// default duration.
    pub timeout_ms: Option<u32>,
    /// Optional stable id — used to *replace* an existing toast.
    /// `None` = always create a new one.
    pub id: Option<u64>,
}

impl NotificationRequest {
    /// Construct an info toast.
    pub fn info(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            severity: Severity::Info,
            timeout_ms: None,
            id: None,
        }
    }

    /// Construct a success toast.
    pub fn success(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            severity: Severity::Success,
            timeout_ms: None,
            id: None,
        }
    }

    /// Construct a warning toast.
    pub fn warning(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            severity: Severity::Warning,
            timeout_ms: None,
            id: None,
        }
    }

    /// Construct an error toast.
    pub fn error(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            severity: Severity::Error,
            timeout_ms: None,
            id: None,
        }
    }
}

/// One toast currently being shown — used by the platform layer's
/// popup window for state.
#[derive(Debug, Clone)]
pub struct ActiveNotification {
    pub id: u64,
    pub request: NotificationRequest,
    pub born_at_ms: u64,
    pub slots_until_dismiss_ms: u32,
}

/// Trait-object view of the manager.
pub trait NotificationSink: Send + Sync {
    /// Submit a new (or replacement) notification. Returns the id
    /// assigned.
    fn submit(&self, req: NotificationRequest) -> u64;
    /// Dismiss a notification explicitly (e.g. user clicked "X").
    fn dismiss(&self, id: u64);
}

/// Thread-safe manager that the platform layer implements.
#[derive(Clone)]
pub struct NotificationManager {
    inner: Arc<RwLock<NotificationState>>,
    default_duration_ms: u32,
    max_visible: usize,
    seq: Arc<parking_lot::Mutex<u64>>,
}

#[derive(Debug, Default)]
struct NotificationState {
    active: HashMap<u64, ActiveNotification>,
    by_age: Vec<u64>,
}

impl NotificationManager {
    /// Build a manager with the configured defaults.
    pub fn new(default_duration_ms: u32, max_visible: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(NotificationState::default())),
            default_duration_ms,
            max_visible,
            seq: Arc::new(parking_lot::Mutex::new(1)),
        }
    }

    /// Submit a request. Returns the assigned id (the caller-supplied
    /// `id`, if any, else the manager's autoincrement).
    pub fn submit_internal(&self, req: NotificationRequest) -> u64 {
        let mut g = self.inner.write();
        let id = match req.id {
            Some(id) => id,
            None => {
                let mut seq = self.seq.lock();
                let v = *seq;
                *seq = seq.wrapping_add(1);
                v
            }
        };
        let duration = req.timeout_ms.unwrap_or(self.default_duration_ms);
        let active = ActiveNotification {
            id,
            request: req,
            born_at_ms: now_ms(),
            slots_until_dismiss_ms: duration,
        };
        g.active.insert(id, active);
        g.by_age.retain(|sid| *sid != id);
        g.by_age.push(id);

        // Cap visible count.
        while g.by_age.len() > self.max_visible {
            let oldest = g.by_age.remove(0);
            g.active.remove(&oldest);
        }
        id
    }

    /// Dismiss a notification by id.
    pub fn dismiss(&self, id: u64) {
        let mut g = self.inner.write();
        g.active.remove(&id);
        g.by_age.retain(|sid| *sid != id);
    }

    /// Drain expired notifications. Returns the dismissed ids so the
    /// platform layer can clean up its window list.
    pub fn sweep_expired(&self) -> Vec<u64> {
        let now = now_ms();
        let mut g = self.inner.write();
        let expired: Vec<u64> = g
            .active
            .values()
            .filter(|n| now.saturating_sub(n.born_at_ms) >= n.slots_until_dismiss_ms as u64)
            .map(|n| n.id)
            .collect();
        for id in &expired {
            g.active.remove(id);
        }
        g.by_age.retain(|sid| !expired.contains(sid));
        expired
    }

    /// Snapshot all currently active notifications.
    pub fn snapshot(&self) -> Vec<ActiveNotification> {
        let g = self.inner.read();
        let mut out: Vec<ActiveNotification> = g.active.values().cloned().collect();
        out.sort_by_key(|n| n.born_at_ms);
        out
    }
}

/// Trait-object bridge — the platform layer's concrete type wraps
/// `NotificationManager` and forwards.
impl NotificationSink for NotificationManager {
    fn submit(&self, req: NotificationRequest) -> u64 {
        self.submit_internal(req)
    }
    fn dismiss(&self, id: u64) {
        self.dismiss(id);
    }
}

/// Cheap wall-clock millis since an arbitrary epoch. Doesn't matter
/// because consumers only need monotonic ordering.
fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifetime_cap_drops_oldest() {
        let m = NotificationManager::new(5000, 2);
        m.submit_internal(NotificationRequest {
            id: Some(1),
            ..NotificationRequest::info("a", "b")
        });
        m.submit_internal(NotificationRequest {
            id: Some(2),
            ..NotificationRequest::info("a", "b")
        });
        m.submit_internal(NotificationRequest {
            id: Some(3),
            ..NotificationRequest::info("a", "b")
        });
        let snap = m.snapshot();
        assert_eq!(snap.len(), 2);
        assert!(snap.iter().any(|n| n.id == 2));
        assert!(snap.iter().any(|n| n.id == 3));
        assert!(!snap.iter().any(|n| n.id == 1));
    }

    #[test]
    fn same_id_replaces_existing() {
        let m = NotificationManager::new(5000, 4);
        m.submit_internal(NotificationRequest {
            id: Some(42),
            ..NotificationRequest::info("first", "body")
        });
        m.submit_internal(NotificationRequest {
            id: Some(42),
            ..NotificationRequest::info("second", "body")
        });
        let snap = m.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].request.title, "second");
    }
}
