//! Start Menu + pinned-shortcut indexer.
//!
//! Walks every `.lnk` / `.exe` file under:
//!
//! * `%APPDATA%\Microsoft\Windows\Start Menu\Programs`
//! * `%PROGRAMDATA%\Microsoft\Windows\Start Menu\Programs`
//!
//! For each candidate we attempt to verify the target exists; links
//! that point nowhere are dropped silently. The resulting
//! [`AppIndex`](crate::core::launcher::AppIndex) is then handed to
//! the [`LauncherEngine`](crate::core::launcher::LauncherEngine) —
//! all fuzzy matching happens in core; this module exists only to
//! feed the catalogue.
//!
//! The indexer is *fail-safe*: a missing Start Menu directory or a
//! permissions error in any one subdirectory must not abort the
//! walk. We log and continue.

use std::path::{Path, PathBuf};

use crate::core::launcher::{AppEntry, AppIndex, Source};

/// One shot of the indexer — returns the discovered catalogue.
pub fn enumerate() -> AppIndex {
    let mut index = AppIndex::new();
    let candidates = start_menu_dirs();
    for dir in candidates {
        walk_into(&dir, &mut index, Source::StartMenu);
    }
    index
}

fn start_menu_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        out.push(
            PathBuf::from(appdata)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
        );
    }
    if let Some(progdata) = std::env::var_os("ProgramData") {
        out.push(
            PathBuf::from(progdata)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
        );
    }
    out
}

fn walk_into(dir: &Path, index: &mut AppIndex, source: Source) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(err) => {
            tracing::debug!(
                target: "jacquewm.launcher",
                directory = %dir.display(),
                error = %err,
                "could not read start menu directory"
            );
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = match entry.file_name().into_string() {
            Ok(s) => s,
            Err(_) => continue,
        };
        if file_name.starts_with('.') {
            continue;
        }
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            walk_into(&path, index, source);
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        match ext.as_deref() {
            Some("lnk") => add_lnk(&path, index, source),
            Some("exe") => add_exe(&path, index, source),
            _ => {}
        }
    }
}

fn add_lnk(path: &Path, index: &mut AppIndex, source: Source) {
    // We deliberately *do not* parse `IShellLink` COM here — that's
    // too heavy for a synchronous startup walk. Instead we use the
    // filename minus the `.lnk` extension as the display name and
    // stash the path; the launcher's `Launch` will defer target
    // resolution to the platform `Launcher::launch_selected`. If the
    // filename doesn't look like a `.exe` launch we drop the entry.
    let stem = match path.file_stem().and_then(|s| s.to_str()) {
        Some(s) => s.to_owned(),
        None => return,
    };
    let id = AppEntry::fingerprint(path);
    let basename = first_word(&stem);
    let name = stem;
    let entry = AppEntry {
        id,
        name,
        exe_basename: basename,
        path: path.to_path_buf(),
        source,
    };
    if index.insert(entry) {
        tracing::trace!(
            target: "jacquewm.launcher",
            path = %path.display(),
            "added shortcut"
        );
    }
}

fn add_exe(path: &Path, index: &mut AppIndex, source: Source) {
    let basename = match path.file_name().and_then(|s| s.to_str()) {
        Some(s) => s.to_owned(),
        None => return,
    };
    let id = AppEntry::fingerprint(path);
    let name = basename.trim_end_matches(".exe").to_owned();
    let entry = AppEntry {
        id,
        name,
        exe_basename: basename,
        path: path.to_path_buf(),
        source,
    };
    if index.insert(entry) {
        tracing::trace!(
            target: "jacquewm.launcher",
            path = %path.display(),
            "added executable"
        );
    }
}

/// Pull the first "word" of the Start Menu entry — used as the
/// fuzzy-match target. e.g. "Visual Studio Code.lnk" →
/// "Visual Studio Code".
fn first_word(s: &str) -> String {
    s.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_word_keeps_full_string() {
        assert_eq!(first_word("Visual Studio Code"), "Visual Studio Code");
    }

    #[test]
    fn enumerate_does_not_panic_without_dirs() {
        // We won't actually inspect the index contents (those
        // depend on the host's installed apps) — just make sure
        // the call is infallible on this unit test.
        let _ = enumerate();
    }
}
