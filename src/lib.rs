//! JacqueWM — A modern Windows workspace manager.
//!
//! JacqueWM brings Linux-style, keyboard-first workspace navigation to
//! Windows 10. Internally it builds on native Windows Virtual Desktops,
//! exposes a strict five-subsystem architecture, and uses a custom COM
//! fixture that is fully isolated from the rest of the codebase so that
//! the system can be extended (tiling engines, installers, etc.) without
//! rewiring the foundation.
//!
//! ## Subsystems
//!
//! * [`core::config`] — TOML configuration management.
//! * [`core::logging`] — tracing initialisation with daily log rotation.
//! * [`core::virtual_desktop`] — abstraction over the platform adapter.
//! * [`core::workspaces`] — the workspace engine.
//! * [`core::windows`] — the window manager.
//! * [`core::hotkeys`] — the hotkey manager.
//! * [`core::startup`] — the boot / auto-start coordinator.
//!
//! ## Platform
//!
//! The only supported platform today is Windows 10 (x64). ARM builds are
//! expected to work without code changes because we depend only on
//! architecture-neutral Win32/COM APIs.

#![cfg(windows)]
// NOTE: The windows-rs 0.58 + recent rustc toolchain combination
// produces a long list of `unsafe_op_in_unsafe_fn` diagnostics for the
// manual vtable wrappers under `platform::windows::desktop::interfaces`.
// The right long-term fix is to wrap every raw-pointer deref + unsafe
// call in its own inner `unsafe { ... }` block. Until that pass is
// done, allow the lint at the crate root so CI is not blocked.
// (Tracked in CHANGELOG as a deferred cleanup.)
#![allow(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]
#![warn(rust_2018_idioms)]
#![warn(unreachable_pub)]

pub mod core;
pub mod error;
pub mod platform;

// =====================================================================
// Re-exports for ergonomic use throughout the codebase.
// =====================================================================

pub use crate::error::{JacqueError, Result};
