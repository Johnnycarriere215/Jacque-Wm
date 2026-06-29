//! Actually launch the selected entry.
//!
//! Cold path — only invoked when the user presses Enter. We use
//! `ShellExecuteW` so that the launcher's stdout/handoff behaves
//! exactly like a user double-click. Resolution of `.lnk` is the
//! default behaviour of `ShellExecuteW`; we don't have to parse
//! the link ourselves.

#![cfg(windows)]

use crate::core::launcher::AppEntry;

/// Launch the entry. Returns Err if the path is unrecognised.
pub fn launch(entry: &AppEntry) -> Result<(), u32> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let path_w: Vec<u16> = entry
        .path
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let result = ShellExecuteW(
            HWND(std::ptr::null_mut()),
            windows::w!("open"),
            windows::PCWSTR(path_w.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        );
        // ShellExecuteW returns an HINSTANCE; values <= 32 are error codes.
        let code = result.0 as isize;
        if code <= 32 {
            return Err(code as u32);
        }
    }
    Ok(())
}
