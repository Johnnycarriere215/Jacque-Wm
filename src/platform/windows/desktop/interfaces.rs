//! Custom COM interface vtables.
//!
//! These structs mirror the vtable layouts of the immersive-shell
//! `IVirtualDesktop*` objects. They are typed manually because the
//! `windows` crate does not provide bindings for these undocumented
//! interfaces.
//!
//! Each struct follows the `windows` crate convention: `#[repr(C)]`
//! aligned to machine pointer width, ordered **exactly** as the
//! native vtable, with an `IUnknown` base in slot 0.

use std::ffi::c_void;

use windows::core::{Error, HRESULT, IUnknown, IUnknown_Vtbl, GUID, Interface};
use windows::Win32::Foundation::{BOOL, HWND, HMONITOR};

// =====================================================================
// IServiceProvider — the COM service-locator.
// =====================================================================

#[repr(C)]
pub struct IServiceProvider_Vtbl {
    pub base__: IUnknown_Vtbl,
    /// QueryService(rsidService, riid, ppvObject) — see Win32 docs.
    pub QueryService: unsafe extern "system" fn(
        this: *mut c_void,
        rsid_service: *const GUID,
        riid: *const GUID,
        ppv_object: *mut *mut c_void,
    ) -> HRESULT,
}

#[repr(transparent)]
pub struct IServiceProvider(pub *mut c_void);
unsafe impl Interface for IServiceProvider {
    type Vtable = IServiceProvider_Vtbl;
    const IID: GUID = GUID::from_u128(0x6D51_40C1_7436_11CE_8034_00AA_0060_09FA);
}

impl IServiceProvider {
    /// # Safety
    ///
    /// `self` must be a live, properly-aligned pointer to an
    /// `IServiceProvider` vtable.
    pub unsafe fn query_service(
        &self,
        service: GUID,
        iid: GUID,
    ) -> windows::core::Result<*mut c_void> {
        let vtbl: *const IServiceProvider_Vtbl = *(self.0 as *const *const IServiceProvider_Vtbl);
        let mut ppv: *mut c_void = std::ptr::null_mut();
        let hr = ((*vtbl).QueryService)(self.0, &service, &iid, &mut ppv);
        if hr.is_ok() {
            Ok(ppv)
        } else {
            Err(Error::from(hr))
        }
    }
}

// =====================================================================
// IObjectArray — the Win32 System Com collection used by GetDesktops.
// =====================================================================

#[repr(C)]
pub struct IObjectArray_Vtbl {
    pub base__: IUnknown_Vtbl,
    pub GetCount: unsafe extern "system" fn(
        this: *mut c_void,
        count: *mut u32,
    ) -> HRESULT,
    pub GetAt: unsafe extern "system" fn(
        this: *mut c_void,
        index: u32,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> HRESULT,
}

#[repr(transparent)]
pub struct IObjectArray(pub *mut c_void);
unsafe impl Interface for IObjectArray {
    type Vtable = IObjectArray_Vtbl;
    const IID: GUID = GUID::from_u128(0x92CA_9DCD_5346_4769_B50B_2F49_36BA_3F0D);
}

impl IObjectArray {
    /// # Safety
    ///
    /// Self pointer must be valid.
    pub unsafe fn get_count(&self) -> windows::core::Result<u32> {
        let vtbl = *(self.0 as *const *const IObjectArray_Vtbl);
        let mut count: u32 = 0;
        let hr = ((*vtbl).GetCount)(self.0, &mut count);
        if hr.is_ok() {
            Ok(count)
        } else {
            Err(Error::from(hr))
        }
    }

    /// # Safety
    ///
    /// Self pointer must be valid.
    pub unsafe fn get_at(&self, index: u32, iid: GUID) -> windows::core::Result<*mut c_void> {
        let vtbl = *(self.0 as *const *const IObjectArray_Vtbl);
        let mut ppv: *mut c_void = std::ptr::null_mut();
        let hr = ((*vtbl).GetAt)(self.0, index, &iid, &mut ppv);
        if hr.is_ok() {
            Ok(ppv)
        } else {
            Err(Error::from(hr))
        }
    }
}

// =====================================================================
// IVirtualDesktopManager — the document-safe per-window API.
// =====================================================================

#[repr(C)]
pub struct IVirtualDesktopManager_Vtbl {
    pub base__: IUnknown_Vtbl,
    /// slot 3: IsWindowOnCurrentVirtualDesktop(hwnd, *bool)
    pub IsWindowOnCurrentVirtualDesktop: unsafe extern "system" fn(
        this: *mut c_void,
        hwnd: HWND,
        on_current: *mut BOOL,
    ) -> HRESULT,
    /// slot 4: GetWindowDesktopId(hwnd, *guid)
    pub GetWindowDesktopId: unsafe extern "system" fn(
        this: *mut c_void,
        hwnd: HWND,
        desktop_id: *mut GUID,
    ) -> HRESULT,
    /// slot 5: MoveWindowToDesktop(hwnd, guid)
    pub MoveWindowToDesktop: unsafe extern "system" fn(
        this: *mut c_void,
        hwnd: HWND,
        desktop_id: *const GUID,
    ) -> HRESULT,
}

#[repr(transparent)]
pub struct IVirtualDesktopManager(pub *mut c_void);
unsafe impl Interface for IVirtualDesktopManager {
    type Vtable = IVirtualDesktopManager_Vtbl;
    const IID: GUID = GUID::from_u128(0xA5CD_92FF_29BE_454C_8D0_4D82_879F_B3F1_B);
}

impl IVirtualDesktopManager {
    /// # Safety
    ///
    /// Self pointer must be valid.
    pub unsafe fn is_window_on_current(&self, hwnd: HWND) -> windows::core::Result<bool> {
        let vtbl = *(self.0 as *const *const IVirtualDesktopManager_Vtbl);
        let mut on: BOOL = BOOL(0);
        let hr = ((*vtbl).IsWindowOnCurrentVirtualDesktop)(self.0, hwnd, &mut on);
        if hr.is_ok() {
            Ok(on.as_bool())
        } else {
            Err(Error::from(hr))
        }
    }

    /// # Safety
    ///
    /// Self pointer must be valid.
    pub unsafe fn get_window_desktop_id(&self, hwnd: HWND) -> windows::core::Result<GUID> {
        let vtbl = *(self.0 as *const *const IVirtualDesktopManager_Vtbl);
        let mut id = GUID::zeroed();
        let hr = ((*vtbl).GetWindowDesktopId)(self.0, hwnd, &mut id);
        if hr.is_ok() {
            Ok(id)
        } else {
            Err(Error::from(hr))
        }
    }

    /// # Safety
    ///
    /// Self pointer must be valid.
    pub unsafe fn move_window_to_desktop(
        &self,
        hwnd: HWND,
        desktop_id: GUID,
    ) -> windows::core::Result<()> {
        let vtbl = *(self.0 as *const *const IVirtualDesktopManager_Vtbl);
        let hr = ((*vtbl).MoveWindowToDesktop)(self.0, hwnd, &desktop_id);
        if hr.is_ok() {
            Ok(())
        } else {
            Err(Error::from(hr))
        }
    }
}

// =====================================================================
// IVirtualDesktop — opaque per-desktop identifier.
// =====================================================================

#[repr(C)]
pub struct IVirtualDesktop_Vtbl {
    pub base__: IUnknown_Vtbl,
    pub GetId: unsafe extern "system" fn(
        this: *mut c_void,
        id: *mut GUID,
    ) -> HRESULT,
    pub IsEqual: unsafe extern "system" fn(
        this: *mut c_void,
        other: *mut c_void,
        equal: *mut BOOL,
    ) -> HRESULT,
    pub GetName: unsafe extern "system" fn(
        this: *mut c_void,
        name: *mut *mut c_void, // HSTRING*
    ) -> HRESULT,
}

#[repr(transparent)]
pub struct IVirtualDesktop(pub *mut c_void);
unsafe impl Interface for IVirtualDesktop {
    type Vtable = IVirtualDesktop_Vtbl;
    const IID: GUID = GUID::from_u128(0x3F07_F934_7A18_4A5C_8E1D_7A6A_0DA0_7C86);
}

impl IVirtualDesktop {
    /// # Safety
    ///
    /// Self pointer must be valid.
    pub unsafe fn get_id(&self) -> windows::core::Result<GUID> {
        let vtbl = *(self.0 as *const *const IVirtualDesktop_Vtbl);
        let mut id = GUID::zeroed();
        let hr = ((*vtbl).GetId)(self.0, &mut id);
        if hr.is_ok() {
            Ok(id)
        } else {
            Err(Error::from(hr))
        }
    }
}

// =====================================================================
// IVirtualDesktopManagerInternal — the rich interface.
// =====================================================================

#[repr(C)]
pub struct IVirtualDesktopManagerInternal_Vtbl {
    pub base__: IUnknown_Vtbl,
    pub GetCount: unsafe extern "system" fn(
        this: *mut c_void,
        monitor: HMONITOR,
        count: *mut u32,
    ) -> HRESULT,
    pub MoveViewToDesktop: unsafe extern "system" fn(
        this: *mut c_void,
        view: *mut c_void,
        desktop: *mut c_void,
    ) -> HRESULT,
    pub CanViewMoveDesktops: unsafe extern "system" fn(
        this: *mut c_void,
        view: *mut c_void,
        can_move: *mut BOOL,
    ) -> HRESULT,
    pub GetCurrentDesktop: unsafe extern "system" fn(
        this: *mut c_void,
        monitor: HMONITOR,
        desktop: *mut *mut c_void,
    ) -> HRESULT,
    pub GetDesktops: unsafe extern "system" fn(
        this: *mut c_void,
        monitor: HMONITOR,
        desktops: *mut *mut c_void,
    ) -> HRESULT,
    pub GetAdjacentDesktop: unsafe extern "system" fn(
        this: *mut c_void,
        desktop: *mut c_void,
        direction: u32,
        adjacent: *mut *mut c_void,
    ) -> HRESULT,
    pub SwitchDesktop: unsafe extern "system" fn(
        this: *mut c_void,
        desktop: *mut c_void,
    ) -> HRESULT,
    pub SwitchDesktopAndMoveForegroundView: unsafe extern "system" fn(
        this: *mut c_void,
        monitor: HMONITOR,
        desktop: *mut c_void,
    ) -> HRESULT,
    pub CreateDesktop: unsafe extern "system" fn(
        this: *mut c_void,
        monitor: HMONITOR,
        desktop: *mut *mut c_void,
    ) -> HRESULT,
    pub MoveDesktop: unsafe extern "system" fn(
        this: *mut c_void,
        desktop: *mut c_void,
        monitor: HMONITOR,
    ) -> HRESULT,
    pub RemoveDesktop: unsafe extern "system" fn(
        this: *mut c_void,
        desktop: *mut c_void,
        fallback: *mut c_void,
    ) -> HRESULT,
    pub FindDesktop: unsafe extern "system" fn(
        this: *mut c_void,
        desktop_id: *const GUID,
        desktop: *mut *mut c_void,
    ) -> HRESULT,
}

#[repr(transparent)]
pub struct IVirtualDesktopManagerInternal(pub *mut c_void);
unsafe impl Interface for IVirtualDesktopManagerInternal {
    type Vtable = IVirtualDesktopManagerInternal_Vtbl;
    const IID: GUID = GUID::from_u128(0xC5E0_CDCA_7B6E_41B2_9FC4_D939_75CC_467B);
}

impl IVirtualDesktopManagerInternal {
    /// Helper: get a raw pointer to the vtable. Caller must dereference.
    ///
    /// # Safety
    ///
    /// Self pointer must be valid.
    pub unsafe fn vtable_ptr(&self) -> *const IVirtualDesktopManagerInternal_Vtbl {
        *(self.0 as *const *const IVirtualDesktopManagerInternal_Vtbl)
    }

    /// Convenience wrapper for `GetCount`.
    pub unsafe fn get_count(&self) -> windows::core::Result<u32> {
        let vtbl = self.vtable_ptr();
        let mut count: u32 = 0;
        let hr = ((*vtbl).GetCount)(self.0, HMONITOR(std::ptr::null_mut()), &mut count);
        if hr.is_ok() {
            Ok(count)
        } else {
            Err(Error::from(hr))
        }
    }

    /// Convenience wrapper for `GetCurrentDesktop`.
    pub unsafe fn get_current(&self) -> windows::core::Result<IVirtualDesktop> {
        let vtbl = self.vtable_ptr();
        let mut ptr: *mut c_void = std::ptr::null_mut();
        let hr = ((*vtbl).GetCurrentDesktop)(self.0, HMONITOR(std::ptr::null_mut()), &mut ptr);
        if hr.is_ok() {
            Ok(IVirtualDesktop(ptr))
        } else {
            Err(Error::from(hr))
        }
    }

    /// Convenience wrapper for `GetDesktops`.
    pub unsafe fn get_desktops(&self) -> windows::core::Result<IObjectArray> {
        let vtbl = self.vtable_ptr();
        let mut arr: *mut c_void = std::ptr::null_mut();
        let hr = ((*vtbl).GetDesktops)(self.0, HMONITOR(std::ptr::null_mut()), &mut arr);
        if hr.is_ok() {
            Ok(IObjectArray(arr))
        } else {
            Err(Error::from(hr))
        }
    }

    /// Convenience wrapper for `SwitchDesktop`.
    pub unsafe fn switch(&self, desktop: IVirtualDesktop) -> windows::core::Result<()> {
        let vtbl = self.vtable_ptr();
        let hr = ((*vtbl).SwitchDesktop)(self.0, desktop.0);
        if hr.is_ok() {
            Ok(())
        } else {
            Err(Error::from(hr))
        }
    }

    /// Convenience wrapper for `CreateDesktop`.
    pub unsafe fn create(&self) -> windows::core::Result<IVirtualDesktop> {
        let vtbl = self.vtable_ptr();
        let mut ptr: *mut c_void = std::ptr::null_mut();
        let hr = ((*vtbl).CreateDesktop)(self.0, HMONITOR(std::ptr::null_mut()), &mut ptr);
        if hr.is_ok() {
            Ok(IVirtualDesktop(ptr))
        } else {
            Err(Error::from(hr))
        }
    }

    /// Convenience wrapper for `FindDesktop(id)`.
    pub unsafe fn find(&self, id: GUID) -> windows::core::Result<IVirtualDesktop> {
        let vtbl = self.vtable_ptr();
        let mut ptr: *mut c_void = std::ptr::null_mut();
        let hr = ((*vtbl).FindDesktop)(self.0, &id, &mut ptr);
        if hr.is_ok() {
            Ok(IVirtualDesktop(ptr))
        } else {
            Err(Error::from(hr))
        }
    }
}
