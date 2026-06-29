//! Registry helpers.
//!
//! Allows JacqueWM to register itself under
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` so it starts
//! on login. All operations are scoped to the current user — no
//! elevation is required.

use windows::core::HSTRING;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ,
};

use crate::error::{JacqueError, Result};

/// Path to the Run key under `HKCU`.
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE_NAME: &str = "JacqueWM";

/// Status of a single Run-key value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupEntry {
    /// The registry contains a JacqueWM entry pointing at `path`.
    Registered(String),
    /// The registry contains an entry, but it points at a different path.
    Mismatched(String),
    /// No JacqueWM entry exists.
    Absent,
}

/// Convert a `&str` constant to a wide UTF-16 string.
fn wide(s: &str) -> windows::core::Result<HSTRING> {
    HSTRING::try_from(s)
}

/// Open the `Run` subkey with the requested access mask.
fn open_run(access: u32) -> Result<HKEY> {
    unsafe {
        let mut hkey = HKEY(std::ptr::null_mut());
        let key_w = wide(RUN_KEY).map_err(|e| JacqueError::AutoStart(e.to_string()))?;
        let status = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            &key_w,
            Some(0),
            KEY_SET_VALUE | KEY_QUERY_VALUE | access,
            &mut hkey,
        );
        if status.is_err() {
            return Err(JacqueError::AutoStart(format!(
                "could not open Run key (status=0x{:08X})",
                status.0 as u32
            )));
        }
        Ok(hkey)
    }
}

/// Register JacqueWM in the Run key. `exe_path` should be the
/// canonical path of the binary, e.g.
/// `C:\Program Files\JacqueWM\jacquewm.exe`.
pub fn register_auto_start(exe_path: &str) -> Result<()> {
    let hkey = open_run(KEY_SET_VALUE.0)?;
    // The data is REG_SZ: a UTF-16LE buffer followed by a UTF-16 NUL.
    let display = format!("\"{}\"", exe_path);
    let mut utf16: Vec<u16> = display.encode_utf16().collect();
    utf16.push(0); // terminate

    // Now convert to a UTF-16LE byte buffer (little-endian on Windows).
    let bytes: Vec<u8> = utf16.iter().flat_map(|c| c.to_le_bytes()).collect();
    let value_w = wide(VALUE_NAME).map_err(|e| JacqueError::AutoStart(e.to_string()))?;

    let result = unsafe { RegSetValueExW(hkey, &value_w, Some(0), REG_SZ, Some(&bytes)) };
    let _ = unsafe { RegCloseKey(hkey) };
    if let Err(err) = result {
        return Err(JacqueError::AutoStart(format!(
            "RegSetValueExW failed: {err}"
        )));
    }
    Ok(())
}

/// Remove the JacqueWM entry from the Run key.
pub fn unregister_auto_start() -> Result<()> {
    let hkey = open_run(KEY_SET_VALUE.0)?;
    let value_w = wide(VALUE_NAME).map_err(|e| JacqueError::AutoStart(e.to_string()))?;
    let status = unsafe { RegDeleteValueW(hkey, &value_w) };
    let _ = unsafe { RegCloseKey(hkey) };
    if status.is_err() && status.0 as u32 != 0x80070002 {
        return Err(JacqueError::AutoStart(format!(
            "RegDeleteValueW failed: 0x{:08X}",
            status.0 as u32
        )));
    }
    Ok(())
}

/// Inspect the current Run key. Returns [`StartupEntry`] describing
/// what is currently stored.
pub fn query_auto_start() -> Result<StartupEntry> {
    let hkey = open_run(KEY_QUERY_VALUE.0)?;
    let value_w = wide(VALUE_NAME).map_err(|e| JacqueError::AutoStart(e.to_string()))?;

    const INITIAL_CAP: usize = 1024;
    let mut buffer = vec![0u8; INITIAL_CAP];
    let mut buffer_size = buffer.len() as u32;

    let status = unsafe {
        RegQueryValueExW(
            hkey,
            &value_w,
            None,
            None,
            Some(buffer.as_mut_ptr()),
            Some(&mut buffer_size),
        )
    };
    let _ = unsafe { RegCloseKey(hkey) };

    match status.0 as u32 {
        0 => {
            // Decode the UTF-16LE byte buffer up to `buffer_size` bytes.
            let utf16: Vec<u16> = buffer
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .take(buffer_size as usize / 2)
                .collect();
            let s = String::from_utf16_lossy(&utf16)
                .trim_end_matches('\0')
                .trim_matches('"')
                .to_string();
            Ok(StartupEntry::Registered(s))
        }
        0x80070002 => Ok(StartupEntry::Absent),
        _ => Err(JacqueError::AutoStart(format!(
            "RegQueryValueExW failed: 0x{:08X}",
            status.0 as u32
        ))),
    }
}
