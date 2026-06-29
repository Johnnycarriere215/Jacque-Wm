//! Windows implementation of the virtual-desktop abstraction.
//!
//! The internal layout is split into four files:
//!
//! * [`guids`]      — every `GUID` we hand out or consume: CLSIDs,
//!                    IIDs, and the diagnostic [`ComInterfaceId`] enum.
//! * [`interfaces`] — the `IVirtualDesktop*` vtable structures that
//!                    back the COM pointers we receive from the
//!                    immersive shell.
//! * [`adapter`]    — the [`WindowsVirtualDesktop`] struct that
//!                    implements [`crate::core::virtual_desktop::VirtualDesktopAdapter`].
//! * [`init`]       — discovers the immersive-shell service that
//!                    hosts the interfaces.
//!
//! Note that the layout is intentionally flat under `desktop/` rather
//! than a deeper tree; the requirement was to keep the public
//! directory tree of `src/platform/windows/` shallow.

pub mod adapter;
pub mod guids;
pub mod init;
pub mod interfaces;

pub use adapter::WindowsVirtualDesktop;
pub use init::acquire as acquire_adapter;
