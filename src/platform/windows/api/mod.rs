//! Windows-specific helpers shared by the rest of the platform layer.
//!
//! These helpers are deliberately tiny so they can live in a single
//! module without growing into a catch-all. Anything larger belongs
//! in its own file (see [`desktop`], [`hooks`], [`window_enum`],
//! [`startup`]).

pub mod com_init;
pub mod message_window;
pub mod process;
pub mod registry;
pub mod shell_wait;
