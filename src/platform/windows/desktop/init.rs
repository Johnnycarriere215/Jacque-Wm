//! COM service discovery.
//!
//! Acquires the immersive-shell COM pointers. The pointers are bound
//! to the lifetime of `Explorer.exe`: if Explorer restarts, they
//! become stale and the entire [`Acquired`] bundle must be
//! re-acquired.
//!
//! IMPORTANT: the pointers live in a single-threaded apartment on
//! the main thread. Sending them across thread boundaries produces
//! undefined behaviour (the shell will reject calls with
//! `RPC_E_WRONG_THREAD`).

use std::ptr::NonNull;

use windows::core::GUID;
use windows::Win32::System::Com::{
    CoCreateInstance, CLSCTX_INPROC_HANDLER, CLSCTX_INPROC_SERVER,
};

use crate::error::{JacqueError, Result};
use crate::platform::windows::api::com_init;
use crate::platform::windows::desktop::guids;
use crate::platform::windows::desktop::interfaces::{
    IServiceProvider, IVirtualDesktopManager, IVirtualDesktopManagerInternal,
};

/// Holds the COM pointers we acquire at startup.
///
/// **All three pointers must be used on the main STA thread.** This
/// type intentionally does neither `Send` nor `Sync`; if you ever
/// need to call methods from another thread, re-acquire the pointers
/// inside that thread's own COM apartment (which usually produces
/// `RPC_E_WRONG_THREAD` from the immersive shell).
pub struct Acquired {
    /// Opaque pointer to the immersive-shell service provider.
    pub service_provider: NonNull<std::ffi::c_void>,
    /// Public, per-window virtual-desktop manager.
    pub manager: NonNull<std::ffi::c_void>,
    /// Internal — used for create / enumerate / switch.
    pub manager_internal: NonNull<std::ffi::c_void>,
}

impl Acquired {
    /// Re-acquire after Explorer was restarted.
    pub fn re_acquire() -> Result<Self> {
        acquire()
    }
}

/// Walk the COM service-locator chain.
///
/// 1. `CoCreateInstance(CLSID_ImmersiveShell, IServiceProvider)`.
/// 2. `CoCreateInstance(CLSID_VirtualDesktopManager, IVDM)` — used
///    for per-window queries.
/// 3. `QueryService(CLSID_VirtualDesktopManagerInternal,
///    IID_IVDMInternal)` — used for everything else.
///
/// We never "release" the pointers explicitly — the immersive shell
/// owns the lifetime and JacqueWM holds them for the life of the
/// process. They are reset en-masse when Explorer restarts.
pub fn acquire() -> Result<Acquired> {
    com_init::init_sta()?;
    unsafe {
        let provider_raw: *mut std::ffi::c_void = CoCreateInstance(
            &guids::CLSID_IMMERSIVE_SHELL,
            None,
            CLSCTX_INPROC_SERVER | CLSCTX_INPROC_HANDLER,
            &guids::IID_ISERVICE_PROVIDER,
        )
        .map_err(|e| JacqueError::Com {
            interface: guids::ComInterfaceId::ServiceProvider,
            hr: e.code().0 as u32,
        })?;
        let provider = IServiceProvider(provider_raw);

        let manager_internal_raw = provider
            .query_service(
                guids::CLSID_VIRTUAL_DESKTOP_MANAGER_INTERNAL,
                guids::IID_VIRTUAL_DESKTOP_MANAGER_INTERNAL,
            )
            .map_err(|e| JacqueError::Com {
                interface: guids::ComInterfaceId::VirtualDesktopManagerInternal,
                hr: e.code().0 as u32,
            })?;
        let vdm_internal = IVirtualDesktopManagerInternal(manager_internal_raw);
        let count = vdm_internal.get_count().map_err(|e| JacqueError::Com {
            interface: guids::ComInterfaceId::VirtualDesktopManagerInternal,
            hr: e.code().0 as u32,
        })?;
        tracing::info!(
            target: "jacquewm.desktop",
            initial_count = count,
            "immersive-shell interfaces acquired"
        );

        let manager: *mut std::ffi::c_void = CoCreateInstance(
            &guids::CLSID_VIRTUAL_DESKTOP_MANAGER,
            None,
            CLSCTX_INPROC_SERVER,
            &guids::IID_IVIRTUAL_DESKTOP_MANAGER,
        )
        .map_err(|e| JacqueError::Com {
            interface: guids::ComInterfaceId::VirtualDesktopManager,
            hr: e.code().0 as u32,
        })?;

        let acquired = Acquired {
            service_provider: NonNull::new(provider_raw).ok_or(JacqueError::Com {
                interface: guids::ComInterfaceId::ServiceProvider,
                hr: 0,
            })?,
            manager: NonNull::new(manager).ok_or(JacqueError::Com {
                interface: guids::ComInterfaceId::VirtualDesktopManager,
                hr: 0,
            })?,
            manager_internal: NonNull::new(manager_internal_raw).ok_or(JacqueError::Com {
                interface: guids::ComInterfaceId::VirtualDesktopManagerInternal,
                hr: 0,
            })?,
        };
        // Intentionally leak references. The immersive shell hosts
        // the singletons; their lifetime ends when Explorer exits.
        std::mem::forget(provider);
        std::mem::forget(vdm_internal);
        Ok(acquired)
    }
}

/// Convenience: just the internal pointer, wrapped as a
/// `IVirtualDesktopManagerInternal`.
pub fn acquire_internal() -> Result<IVirtualDesktopManagerInternal> {
    let acquired = acquire()?;
    Ok(IVirtualDesktopManagerInternal(acquired.manager_internal.as_ptr()))
}

/// Convenience: just the public manager pointer, wrapped.
pub fn acquire_manager() -> Result<IVirtualDesktopManager> {
    let acquired = acquire()?;
    Ok(IVirtualDesktopManager(acquired.manager.as_ptr()))
}
