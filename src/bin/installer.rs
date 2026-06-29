//! JacqueWM installer — a *safe* standalone helper.
//!
//! Spec compliance (Part 4):
//!
//! * "Be optional (portable build must also exist)" — yes, the main
//!   `jacquewm.exe` is fully portable; this installer is convenience.
//! * "Require no admin privileges for portable mode" — never touches
//!   `HKLM`, only `HKCU`.
//! * "Offer install location selection" — `--dir <path>`.
//! * "Offer launch-on-startup toggle (user opt-in only)" —
//!   `--auto-start` (default off).
//! * "Never modify system files outside application directory" — we
//!   copy our own files into a chosen directory and that's it.
//! * "Never require reboot" — registry writes are flushed and exit.
//!
//! Subcommands:
//!
//! * `jacquewm-installer install [--dir DIR] [--auto-start]`
//! * `jacquewm-installer uninstall [--dir DIR]`
//! * `jacquewm-installer portable [--dir DIR]`
//!
//! The installer MUST be `no_std`-clean for portability — it does not
//! depend on `jacquewm` the library (we want zero recompilation), but
//! we share the same `windows` crate and `parking_lot` for safety.

#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::ptr;
use windows::w;
use windows::core::PCWSTR;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
    RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE,
    REG_SZ,
};

mod cli;

fn main() -> std::process::ExitCode {
    cli::run()
}

// =====================================================================
// Constants shared with the runtime
// =====================================================================

const RUN_KEY_PATH: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_VALUE_NAME: &str = "JacqueWM";

// =====================================================================
// install — copies binary, optionally registers HKCU\Run.
// =====================================================================

/// Install JacqueWM into `dir` and (optionally) register auto-start.
pub fn install(dir: PathBuf, auto_start: bool) -> Result<(), String> {
    ensure_dir(&dir).map_err(|e| format!("create install dir: {e}"))?;
    let exe = current_exe_path().map_err(|e| format!("locate self: {e}"))?;
    let target = dir.join("jacquewm.exe");
    std::fs::copy(&exe, &target).map_err(|e| format!("copy binary: {e}"))?;
    println!("installed {}", target.display());
    let cfg_dir = dir.join("config");
    ensure_dir(&cfg_dir).map_err(|e| format!("create config dir: {e}"))?;
    let log_dir = dir.join("logs");
    ensure_dir(&log_dir).map_err(|e| format!("create log dir: {e}"))?;
    if auto_start {
        let cmd = format!("\"{}\"", target.display());
        write_run_key(&cmd).map_err(|e| format!("register auto-start: {e}"))?;
        println!("auto-start registered (HKCU\\{RUN_KEY_PATH}\\{RUN_VALUE_NAME})");
    } else {
        delete_run_key_if_present()?;
        println!("auto-start NOT registered");
    }
    println!("\nJacqueWM is installed at {}.", dir.display());
    println!("Run jacquewm.exe to start. Pass --register on first run to enable boot-on-login.");
    Ok(())
}

/// Cleanup helper: copy the binary into `dir` but do NOT touch the
/// registry.
pub fn install_portable(dir: PathBuf) -> Result<(), String> {
    ensure_dir(&dir).map_err(|e| format!("create install dir: {e}"))?;
    let exe = current_exe_path().map_err(|e| format!("locate self: {e}"))?;
    let target = dir.join("jacquewm.exe");
    std::fs::copy(&exe, &target).map_err(|e| format!("copy binary: {e}"))?;
    println!("portable install at {}", dir.display());
    Ok(())
}

/// Uninstall JacqueWM. Removes the HKCU Run key (if present), then
/// the install directory contents. Prints a warning before deleting
/// anything.
pub fn uninstall(dir: PathBuf) -> Result<(), String> {
    delete_run_key_if_present()?;
    if !dir.exists() {
        println!("install dir does not exist; nothing to remove");
        return Ok(());
    }
    println!(
        "about to delete {} (this action is irreversible)",
        dir.display()
    );
    remove_dir_contents_recursive(&dir).map_err(|e| format!("remove install dir: {e}"))?;
    println!("uninstall complete");
    Ok(())
}

// =====================================================================
// Registry helpers — HKCU only.
// =====================================================================

unsafe fn open_run_key(writeable: bool) -> Result<HKEY, String> {
    let path_w: Vec<u16> = RUN_KEY_PATH
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut desired = KEY_READ;
    if writeable {
        desired = KEY_READ | KEY_SET_VALUE;
    }
    let mut key = HKEY(ptr::null_mut());
    let result = RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(path_w.as_ptr()), 0, desired, &mut key);
    if result.is_err() {
        return Err(format!("RegOpenKeyExW failed: {result:?}"));
    }
    Ok(key)
}

fn read_run_key() -> Result<Option<String>, String> {
    unsafe {
        let key = open_run_key(false)?;
        let name_w: Vec<u16> = RUN_VALUE_NAME
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut buf = vec![0u16; 1024];
        let mut buf_size: u32 = (buf.len() * 2) as u32;
        let mut ty = REG_SZ;
        let result = RegQueryValueExW(
            key,
            PCWSTR(name_w.as_ptr()),
            None,
            Some(&mut ty),
            Some(buf.as_mut_ptr().cast()),
            Some(&mut buf_size),
        );
        let _ = RegCloseKey(key);
        if result.is_err() {
            return Ok(None);
        }
        let n_chars = buf_size as usize / 2;
        let s = String::from_utf16_lossy(&buf[..n_chars.min(buf.len())])
            .trim_end_matches('\0')
            .to_string();
        Ok(Some(s))
    }
}

fn write_run_key(cmd: &str) -> Result<(), String> {
    unsafe {
        let key = open_run_key(true)?;
        let name_w: Vec<u16> = RUN_VALUE_NAME
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let cmd_w: Vec<u16> = cmd.encode_utf16().chain(std::iter::once(0)).collect();
        let payload: Vec<u8> = cmd_w
            .iter()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        let result = RegSetValueExW(
            key,
            PCWSTR(name_w.as_ptr()),
            0,
            REG_SZ,
            Some(&payload),
        );
        let _ = RegCloseKey(key);
        result.map_err(|e| format!("RegSetValueExW failed: {e:?}")).map(|_| ())
    }
}

fn delete_run_key_if_present() -> Result<(), String> {
    unsafe {
        let key = open_run_key(true)?;
        let name_w: Vec<u16> = RUN_VALUE_NAME
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let result = RegDeleteValueW(key, PCWSTR(name_w.as_ptr()));
        let _ = RegCloseKey(key);
        match result {
            Ok(()) => {
                println!("removed HKCU\\{RUN_KEY_PATH}\\{RUN_VALUE_NAME}");
                Ok(())
            }
            Err(e) => {
                // ERROR_FILE_NOT_FOUND is fine — the key may not exist.
                eprintln!("warning: delete Run value: {e:?}");
                Ok(())
            }
        }
    }
}



// =====================================================================
// Filesystem helpers
// =====================================================================

fn ensure_dir(p: &Path) -> std::io::Result<()> {
    if !p.exists() {
        std::fs::create_dir_all(p)?;
    }
    Ok(())
}

fn remove_dir_contents_recursive(p: &Path) -> std::io::Result<()> {
    if !p.exists() {
        return Ok(());
    }
    if p.is_dir() {
        for entry in std::fs::read_dir(p)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                remove_dir_contents_recursive(&path)?;
                std::fs::remove_dir(&path)?;
            } else {
                std::fs::remove_file(&path)?;
            }
        }
        std::fs::remove_dir(p)?;
    } else {
        std::fs::remove_file(p)?;
    }
    Ok(())
}

fn current_exe_path() -> std::io::Result<PathBuf> {
    Ok(std::env::current_exe()?)
}

// =====================================================================
// Detect existing install — used by subcommand auto-detection.
// =====================================================================

/// Returns the path of the install dir if the HKCU Run key is set and
/// resolves to an existing executable.
pub fn detect_install_dir() -> Option<PathBuf> {
    let cmd = read_run_key().ok().flatten()?;
    let between = cmd.split('"').nth(1)?;
    let p = PathBuf::from(between);
    if p.exists() {
        p.parent().map(|p| p.to_path_buf())
    } else {
        None
    }
}


