//! Platform layer.
//!
//! Wraps the OS-specific implementations. Future versions of JacqueWM
//! may add an `unix` module, but the only supported target today is
//! Windows.

#[cfg(windows)]
pub mod windows;
