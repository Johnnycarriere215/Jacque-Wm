//! Windows-specific implementation of the platform traits.
//!
//! All Win32 / COM code lives under this module. The rest of JacqueWM
//! only talks to the trait objects declared under `core::*`.

pub mod api;
pub mod desktop;
pub mod hooks;
pub mod startup;
pub mod window_enum;

/// Common CRT / kernel entry point used by the binaries in the
/// `bin/` folder. Most of the work is delegated to the [`bin::main`]
/// function; this entry point exists so we can centralise Win32
/// pre-init (process priority class, error mode, etc.).
pub mod bin {
    use windows::Win32::System::Threading::{
        GetCurrentProcess, SetPriorityClass, BELOW_NORMAL_PRIORITY_CLASS,
    };
    use windows::Win32::Foundation::SetLastError;

    /// Apply process-level configuration that has to happen *before*
    /// `main` does any heavy work. Idempotent — safe to call twice.
    pub fn pre_init() {
        unsafe {
            // Lower our priority so that we never steal CPU from the
            // user's foreground apps.
            let _ = SetPriorityClass(GetCurrentProcess(), BELOW_NORMAL_PRIORITY_CLASS);
            // Ensure no surprise popups from COM errors.
            SetLastError(windows::Win32::Foundation::WIN32_ERROR(0));
        }
    }
}
