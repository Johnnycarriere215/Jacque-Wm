//! COM interface IDs and CLSIDs used by JacqueWM.
//!
//! Values match the immersive-shell reverse-engineered contracts used
//! by `MScholtes/VirtualDesktop`, `Grabacr07k/VirtualDesktop` and the
//! `VirtualDesktop` PowerShell module. They are stable across
//! Windows 10 22H2 (Build 19045) and Windows 11 22H2 (Build 22621).
//!
//! These GUIDs are **undocumented** and may change in future Windows
//! releases.

use windows::core::GUID;
use windows::core::Interface;

/// COM CLSID for the immersive-shell service provider.
///
/// Acquiring this object with `CoCreateInstance` yields an
/// `IServiceProvider` which we then ask for the virtual-desktop
/// interfaces.
pub const CLSID_IMMERSIVE_SHELL: GUID = GUID::from_u128(0xC2F03A33_21F5_47FA_B4BB_1562_90A2_0B9A);

/// COM CLSID for the basic virtual-desktop manager.
pub const CLSID_VIRTUAL_DESKTOP_MANAGER: GUID = GUID::from_u128(0xAA50_9086_5CA9_4F25_9BC8_B8AC_9BB6_8F50);

/// COM CLSID for the internal virtual-desktop manager.
pub const CLSID_VIRTUAL_DESKTOP_MANAGER_INTERNAL: GUID = GUID::from_u128(0xC5E0_CDCA_7B6E_41B2_9FC4_D939_75CC_467B);

/// IID for `IVirtualDesktopManager` (the document-safe per-window API).
pub const IID_IVIRTUAL_DESKTOP_MANAGER: GUID = GUID::from_u128(0xA5CD_92FF_29BE_454C_8D0_4D82_879F_B3F1_B);

/// IID for `IServiceProvider` (the COM service-locator interface).
pub const IID_ISERVICE_PROVIDER: GUID = GUID::from_u128(0x6D51_40C1_7436_11CE_8034_00AA_0060_09FA);

/// IID for `IObjectArray` (Win32 System Com enumeration).
pub const IID_IOBJECT_ARRAY: GUID = GUID::from_u128(0x92CA_9DCD_5346_4769_B50B_2F49_36BA_3F0D);

/// IID for `IVirtualDesktopManagerInternal`. Same as the CLSID.
pub const IID_VIRTUAL_DESKTOP_MANAGER_INTERNAL: GUID = CLSID_VIRTUAL_DESKTOP_MANAGER_INTERNAL;

/// IID for `IVirtualDesktop` — used as the opaque `IVirtualDesktop*`
/// parameter in many Manager-Internal methods.
pub const IID_IVIRTUAL_DESKTOP: GUID = GUID::from_u128(0x3F07_F934_7A18_4A5C_8E1D_7A6A_0DA0_7C86);

/// Diagnostic identifier placed in `JacqueError::Com::interface` so
/// failures can be attributed to the right interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ComInterfaceId {
    /// `IVirtualDesktop`
    VirtualDesktop,
    /// `IVirtualDesktopManager`
    VirtualDesktopManager,
    /// `IVirtualDesktopManagerInternal`
    VirtualDesktopManagerInternal,
    /// `IServiceProvider`
    ServiceProvider,
    /// `IObjectArray`
    ObjectArray,
    /// `IApplicationView`
    ApplicationView,
    /// Unknown / unspecified.
    Unknown,
}

impl ComInterfaceId {
    /// Returns the IID associated with this interface identifier.
    pub fn iid(&self) -> GUID {
        match self {
            ComInterfaceId::VirtualDesktop => IID_IVIRTUAL_DESKTOP,
            ComInterfaceId::VirtualDesktopManager => IID_IVIRTUAL_DESKTOP_MANAGER,
            ComInterfaceId::VirtualDesktopManagerInternal => {
                IID_VIRTUAL_DESKTOP_MANAGER_INTERNAL
            }
            ComInterfaceId::ServiceProvider => IID_ISERVICE_PROVIDER,
            ComInterfaceId::ObjectArray => IID_IOBJECT_ARRAY,
            ComInterfaceId::ApplicationView => {
                GUID::from_u128(0xD18A_74C8_1BB4_4FF4_B36D_5AF3_BC66_7AE9)
            }
            ComInterfaceId::Unknown => GUID::zeroed(),
        }
    }
}

/// Convert our opaque [`crate::core::virtual_desktop::DesktopId`]
/// into the GUID the immersive-shell COM API uses.
pub fn desktop_id_to_guid(id: crate::core::virtual_desktop::DesktopId) -> GUID {
    GUID::from_u128(u128::from_le_bytes(id.0))
}

/// Convert a GUID into our opaque [`crate::core::virtual_desktop::DesktopId`].
pub fn guid_to_desktop_id(g: GUID) -> crate::core::virtual_desktop::DesktopId {
    let v = u128::from(g);
    crate::core::virtual_desktop::DesktopId(v.to_le_bytes())
}
