//! Process-level helpers.

use std::path::PathBuf;

use windows::core::HSTRING;

/// Return the path of the running executable.
pub fn current_exe() -> std::io::Result<PathBuf> {
    std::env::current_exe()
}

/// Return canonical argv[0] for use with shell-execute calls.
pub fn module_name() -> String {
    current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_string()))
        .unwrap_or_else(|| "jacquewm".to_string())
}

/// Convert a Rust path into a wide NULL-terminated string suitable for
/// Win32 APIs.
pub fn wide(s: &std::path::Path) -> windows::core::Result<HSTRING> {
    HSTRING::try_from(s.as_os_str())
        .map_err(|e| windows::core::Error::new(windows::core::HRESULT(e.win32_error().unwrap_or(0) as u32), e.to_string()))
}
