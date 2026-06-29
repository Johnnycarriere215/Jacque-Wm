//! Concrete [`crate::core::virtual_desktop::VirtualDesktopAdapter`]
//! implementation that talks to the immersive-shell COM interfaces.
//!
//! All methods on this struct **must be called from the main STA
//! thread** — the methods touch COM pointers and the immersive-shell
//! service rejects cross-thread calls.

use std::ptr::NonNull;
use std::sync::Arc;

use tracing::{debug, info, trace, warn};

use crate::core::virtual_desktop::{DesktopId, VirtualDesktopAdapter};
use crate::core::WorkspaceIndex;
use crate::error::{JacqueError, Result};
use crate::platform::windows::desktop::guids;
use crate::platform::windows::desktop::init::acquire;
use crate::platform::windows::desktop::interfaces::{
    IVirtualDesktop, IVirtualDesktopManager, IVirtualDesktopManagerInternal,
};

/// Windows-backed virtual-desktop adapter.
///
/// Holds raw COM pointers in `Option` so that the discovery loop can
/// transparently reinitialise after an Explorer restart.
pub struct WindowsVirtualDesktop {
    manager: Option<NonNull<std::ffi::c_void>>,
    manager_internal: Option<NonNull<std::ffi::c_void>>,
}

impl WindowsVirtualDesktop {
    /// Acquire the immersive-shell pointers from the COM service locator.
    pub fn acquire() -> Result<Arc<Self>> {
        let acquired = acquire()?;
        Ok(Arc::new(Self {
            manager: Some(acquired.manager),
            manager_internal: Some(acquired.manager_internal),
        }))
    }

    /// Re-acquire after an Explorer restart.
    pub fn refresh(&mut self) -> Result<()> {
        let acquired = acquire()?;
        self.manager = Some(acquired.manager);
        self.manager_internal = Some(acquired.manager_internal);
        info!(
            target: "jacquewm.desktop",
            "COM pointers refreshed"
        );
        Ok(())
    }

    fn manager(&self) -> Result<&IVirtualDesktopManager> {
        let Some(ptr) = self.manager else {
            return Err(JacqueError::DesktopEnumeration(
                "manager pointer is null — call refresh".into(),
            ));
        };
        Ok(unsafe { &*(ptr.as_ptr() as *const IVirtualDesktopManager) })
    }

    fn manager_internal(&self) -> Result<&IVirtualDesktopManagerInternal> {
        let Some(ptr) = self.manager_internal else {
            return Err(JacqueError::DesktopEnumeration(
                "manager_internal pointer is null — call refresh".into(),
            ));
        };
        Ok(unsafe { &*(ptr.as_ptr() as *const IVirtualDesktopManagerInternal) })
    }

    fn desktop_id_list(&self) -> Result<Vec<IVirtualDesktop>> {
        unsafe {
            let mgr = self.manager_internal()?;
            let arr = mgr.get_desktops()?;
            let count = arr.get_count()?;
            let mut out = Vec::with_capacity(count as usize);
            for i in 0..count {
                let ptr = arr.get_at(i, guids::IID_IVIRTUAL_DESKTOP)?;
                out.push(IVirtualDesktop(ptr));
            }
            Ok(out)
        }
    }

    /// Re-acquire pointers and report any desktop count drift. Called
    /// when the platform notice says `TaskbarCreated` was broadcast.
    pub fn on_explorer_restart(&mut self) -> Result<usize> {
        warn!(
            target: "jacquewm.desktop",
            "explorer restart detected; re-acquiring COM pointers"
        );
        self.refresh()?;
        let count = unsafe { self.manager_internal()?.get_count()? as usize };
        info!(target: "jacquewm.desktop", count = count, "post-restart desktop count");
        Ok(count)
    }
}

impl VirtualDesktopAdapter for WindowsVirtualDesktop {
    fn enumerate(&self) -> Result<Vec<DesktopId>> {
        unsafe {
            let desktops = self.desktop_id_list()?;
            let mut ids = Vec::with_capacity(desktops.len());
            for d in desktops {
                let guid = d.get_id()?;
                ids.push(guids::guid_to_desktop_id(guid));
            }
            debug!(target: "jacquewm.desktop", count = ids.len(), "desktop enumeration complete");
            Ok(ids)
        }
    }

    fn current(&self) -> Result<DesktopId> {
        unsafe {
            let mgr = self.manager_internal()?;
            let desktop = mgr.get_current()?;
            let guid = desktop.get_id()?;
            Ok(guids::guid_to_desktop_id(guid))
        }
    }

    fn switch_to(&self, index: WorkspaceIndex) -> Result<()> {
        unsafe {
            let mgr = self.manager_internal()?;
            let desktops = self.desktop_id_list()?;
            let pos = (index.get() - 1) as usize;
            let target = desktops.get(pos).ok_or_else(|| JacqueError::DesktopSwitch {
                index: index.get(),
                reason: format!("only {} desktops exist", desktops.len()),
            })?;
            mgr.switch(*target).map_err(|e| JacqueError::Com {
                interface: guids::ComInterfaceId::VirtualDesktopManagerInternal,
                hr: e.code().0 as u32,
            })?;
            info!(target: "jacquewm.desktop", target = index.get(), "SwitchDesktop called");
            Ok(())
        }
    }

    fn create(&self) -> Result<DesktopId> {
        unsafe {
            let mgr = self.manager_internal()?;
            let desktop = mgr.create().map_err(|e| JacqueError::Com {
                interface: guids::ComInterfaceId::VirtualDesktopManagerInternal,
                hr: e.code().0 as u32,
            })?;
            let guid = desktop.get_id()?;
            Ok(guids::guid_to_desktop_id(guid))
        }
    }

    fn move_window(&self, hwnd: u64, index: WorkspaceIndex) -> Result<()> {
        unsafe {
            let hwnd_val = windows::Win32::Foundation::HWND(hwnd as *mut std::ffi::c_void);
            let desktops = self.desktop_id_list()?;
            let pos = (index.get() - 1) as usize;
            let target_id = desktops.get(pos).ok_or_else(|| JacqueError::WindowMove {
                hwnd,
                index: index.get(),
                reason: format!("only {} desktops exist", desktops.len()),
            })?;
            let guid = target_id.get_id()?;
            self.manager()?
                .move_window_to_desktop(hwnd_val, guid)
                .map_err(|e| JacqueError::Com {
                    interface: guids::ComInterfaceId::VirtualDesktopManager,
                    hr: e.code().0 as u32,
                })?;
            trace!(target: "jacquewm.desktop", hwnd = hwnd, target = index.get(), "MoveWindowToDesktop called");
            Ok(())
        }
    }

    fn window_desktop(&self, hwnd: u64) -> Result<DesktopId> {
        unsafe {
            let hwnd_val = windows::Win32::Foundation::HWND(hwnd as *mut std::ffi::c_void);
            let guid = self.manager()?.get_window_desktop_id(hwnd_val)?;
            Ok(guids::guid_to_desktop_id(guid))
        }
    }

    fn count(&self) -> Result<usize> {
        unsafe { Ok(self.manager_internal()?.get_count()? as usize) }
    }
}
