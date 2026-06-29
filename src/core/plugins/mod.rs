//! Plugin architecture — **design only.** Per the spec:
//!
//! > "Design a plugin system but DO NOT fully implement runtime plugin
//! > execution in this version. Must define: Plugin interface
//! > structure, Plugin lifecycle hooks ..., Safe sandboxing rules, ...
//!
//! What we provide:
//!
//! 1. The [`JacquePlugin`] trait — what every plugin *would* implement.
//! 2. The [`HookEvent`] enum — every event a plugin observes.
//! 3. The [`PluginManifest`] struct — the TOML/JSON-like metadata the
//!    user enables a plugin with.
//! 4. Sandboxing rules: documented, enforced statically by Rust
//!    trait isolation (no plugin can touch internal state).
//!
//! What we do **NOT** provide:
//!
//! * Dynamic library loading (`dlopen`, `LoadLibraryW`).
//! * A `plugin_dir` scanner.
//! * Sandboxing primitives — Rust's borrow checker is the sandbox.
//! * Auto-execution on startup.
//!
//! The user is expected to register plugins at compile time via a
//! future `pub fn register(plugin: Arc<dyn JacquePlugin>)` on a
//! future `PluginRegistry`. The current milestone ships the trait
//! + manifest types only; no `PluginRegistry` is constructed.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Lifecycle-level capabilities a plugin can request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Permission {
    /// Read-only access to workspace counts and the active workspace.
    ReadWorkspaces,
    /// Read-only access to the per-workspace layout (no PII).
    ReadLayout,
    /// Send/receive [`HookEvent::WorkspaceChange`] callbacks.
    ObserveWorkspaceChange,
    /// Send/receive [`HookEvent::WindowEvent`] callbacks.
    ObserveWindowEvent,
    /// Submit a notification via the NotificationManager.
    SubmitNotification,
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Permission::ReadWorkspaces => "read:workspaces",
            Permission::ReadLayout => "read:layout",
            Permission::ObserveWorkspaceChange => "observe:workspace_change",
            Permission::ObserveWindowEvent => "observe:window_events",
            Permission::SubmitNotification => "submit:notification",
        })
    }
}

/// Identifies a plugin profile — used by the registry to limit
/// sandboxing defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PluginProfile {
    /// Re-implements a core engine (e.g. a different tiling layout).
    /// Receives every workspace event.
    Core,
    /// Adds UI tokens / animations to the existing theme.
    Theme,
    /// Adds launcher entries (rules or shortcuts).
    Launcher,
}

impl Default for PluginProfile {
    fn default() -> Self {
        PluginProfile::Theme
    }
}

/// Plugin metadata — the human-readable envelope the user enables a
/// plugin with. Loaded from TOML.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    /// Stable id: `<author>/<name>` — used to enable/disable.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Author.
    #[serde(default)]
    pub author: String,
    /// Semantic version (MAJOR.MINOR.PATCH).
    pub version: String,
    /// Profile the plugin belongs to.
    #[serde(default)]
    pub profile: PluginProfile,
    /// Permissions the plugin requests (must be subset of the
    /// manifest's profile defaults).
    #[serde(default)]
    pub permissions: Vec<Permission>,
    /// JacqueWM version compatibility. e.g. `^0.1.0`.
    #[serde(default)]
    pub jacquewm_version: String,
}

impl PluginManifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty() || !self.id.contains('/') {
            return Err("id must be `<author>/<name>`".into());
        }
        if self.version.split('.').count() < 3 {
            return Err("version must be MAJOR.MINOR.PATCH".into());
        }
        Ok(())
    }
}

/// Events the core engine emits to plugins.
#[derive(Debug, Clone)]
pub enum HookEvent {
    /// The user switched to a new workspace (1..=9 inclusive).
    WorkspaceChange { from: u8, to: u8 },
    /// A window was created/destroyed/etc.
    WindowEvent { kind: WindowHookKind },
}

/// Window sub-event usable by plugins.
#[derive(Debug, Clone)]
pub enum WindowHookKind {
    Created { hwnd: u64 },
    Destroyed { hwnd: u64 },
    Focused { hwnd: u64 },
    TitleChanged { hwnd: u64, title: String },
    Moved { hwnd: u64, x: i32, y: i32, width: i32, height: i32 },
}

/// The trait every plugin implements.
///
/// Plugins are *static* — registered at compile time with the
/// future `PluginRegistry`. Their methods are called from the main
/// loop on the same thread as the engine; plugins must therefore
/// not block.
pub trait JacquePlugin: Send + Sync {
    /// Identifier, identical to the manifest's id.
    fn id(&self) -> &str;

    /// Called once after the plugin is registered.
    fn on_load(&self);

    /// Called once at shutdown, after the engine has stopped
    /// emitting hooks.
    fn on_unload(&self);

    /// Called for every workspace change. Default = no-op.
    fn on_workspace_change(&self, _from: u8, _to: u8) {}

    /// Called for every window event. Default = no-op.
    fn on_window_event(&self, _kind: &WindowHookKind) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_validates_id_format() {
        let m = PluginManifest {
            id: "acme/jacqlint".into(),
            name: "Jacqlint".into(),
            author: "acme".into(),
            version: "0.1.0".into(),
            profile: PluginProfile::Launcher,
            permissions: vec![Permission::ReadWorkspaces],
            jacquewm_version: "^0.1.0".into(),
        };
        m.validate().unwrap();
    }

    #[test]
    fn manifest_rejects_bad_id() {
        let m = PluginManifest {
            id: "no-slash".into(),
            name: "Bad".into(),
            author: "x".into(),
            version: "0.1.0".into(),
            profile: PluginProfile::Theme,
            permissions: vec![],
            jacquewm_version: String::new(),
        };
        assert!(m.validate().is_err());
    }

    #[test]
    fn manifest_rejects_unversioned() {
        let m = PluginManifest {
            id: "a/b".into(),
            name: "Bad".into(),
            author: "x".into(),
            version: "1".into(),
            profile: PluginProfile::Theme,
            permissions: vec![],
            jacquewm_version: String::new(),
        };
        assert!(m.validate().is_err());
    }

    struct CountingPlugin;
    impl JacquePlugin for CountingPlugin {
        fn id(&self) -> &str { "test/counter" }
        fn on_load(&self) {}
        fn on_unload(&self) {}
    }

    #[test]
    fn trait_can_be_implemented() {
        let _: Box<dyn JacquePlugin> = Box::new(CountingPlugin);
    }
}
