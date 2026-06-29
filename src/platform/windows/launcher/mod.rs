//! Windows-side launcher implementation.

pub mod index;
pub mod window;

/// Re-export the launcher-engine entry-point for main.rs.
pub use window::build_engine;
pub use window::LAUNCHER_CLASS;
