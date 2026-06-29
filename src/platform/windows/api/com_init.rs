//! COM apartment initialisation and lifecycle helpers.
//!
//! JacqueWM runs in a single-threaded apartment on the main thread.
//! All COM calls that touch the immersive-shell objects (which is
//! every VirtualDesktop call) must happen on a thread that called
//! `CoInitializeEx(COINIT_APARTMENTTHREADED)`. The pointer to
//! `IVirtualDesktopManagerInternal` cannot legally be used across
//! apartment or thread boundaries — see
//! <https://learn.microsoft.com/windows/win32/com/processes--threads-and-apartments>.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::System::Com::{
    CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED,
};

use crate::error::{JacqueError, Result};

static INITIALISED: AtomicBool = AtomicBool::new(false);
static LAST_ERROR: OnceLock<String> = OnceLock::new();

/// Initialise the calling thread's COM apartment as STA. Idempotent —
/// successive calls return `Ok(())` without re-initialising.
///
/// # Errors
///
/// Returns [`JacqueError::ComInit`] if `CoInitializeEx` returned
/// something other than `S_OK` or `S_FALSE`. `S_FALSE` indicates
/// "already initialised" and is therefore treated as a success.
pub fn init_sta() -> Result<()> {
    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        match hr {
            windows::Win32::Foundation::S_OK => {
                INITIALISED.store(true, Ordering::SeqCst);
                Ok(())
            }
            // S_FALSE == 0x00000001: apartment already initialised on
            // this thread. We treat that as success.
            r if r.0 == 1 => Ok(()),
            r if r.0 as u32 == 0x80010106 => {
                // RPC_E_CHANGED_MODE — the thread is already in MTA.
                Err(JacqueError::ComInit(
                    "thread already in MTA; cannot switch to STA".into(),
                ))
            }
            r => {
                let msg = format!("CoInitializeEx returned 0x{:08X}", r.0 as u32);
                let _ = LAST_ERROR.set(msg.clone());
                Err(JacqueError::ComInit(msg))
            }
        }
    }
}

/// Returns the last COM initialisation error, if any. Useful for
/// surfacing a useful message in logs.
pub fn last_error() -> Option<&'static str> {
    LAST_ERROR.get().map(String::as_str)
}

/// Uninitialise the COM apartment. Only call when shutting down.
pub fn shutdown() {
    if INITIALISED.swap(false, Ordering::SeqCst) {
        unsafe { CoUninitialize() };
    }
}
