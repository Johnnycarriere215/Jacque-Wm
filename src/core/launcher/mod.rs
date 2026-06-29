//! Application launcher.
//!
//! A small, in-memory, fuzzy-search index of installed applications.
//!
//! ## Pipeline
//!
//! 1. **Index** — at startup the platform layer walks the Start Menu
//!    directories and pinned shortcuts to populate [`AppIndex`].
//!    Re-indexing is also exposed for future background refresh.
//! 2. **Query** — the user types into the launcher's edit box.
//!    [`LauncherEngine::update_query`] recomputes the result list in
//!    under ~16 ms (spec requirement: "Filter updates <16ms"). The
//!    matcher is a small contiguous-subsequence scorer with bonuses
//!    for prefix and case match.
//! 3. **Select** — Up/Down/Enter/Escape drive selection. Enter fires
//!    [`LauncherEvent::Launch`] which the platform launcher turns
//!    into a `CreateProcessW` call.
//!
//! The engine does no I/O at query time — all entries are pre-loaded.
//! Failure isolation: the engine never panics. It exposes a
//! [`LauncherError`] enum and returns it directly.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;

/// Display category of an entry — used by the rule matcher to decide
/// tie-breakers and by the panel renderer to pick an icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    /// A Start Menu discoverable link (`.lnk`, `.exe`).
    StartMenu,
    /// User-pinned shortcut.
    Pinned,
    /// Manually-added alias (not yet implemented — reserved).
    Manual,
}

impl Source {
    fn glyph(self) -> &'static str {
        match self {
            Source::StartMenu => "›",
            Source::Pinned => "★",
            Source::Manual => "+",
        }
    }
}

/// One indexed application entry. Cheap to clone.
#[derive(Debug, Clone)]
pub struct AppEntry {
    /// Stable identifier — derived from the canonical path. Two
    /// entries with the same `id` are considered the same app for
    /// de-duplication.
    pub id: u64,
    /// Display name (exactly what the user sees in the result list).
    pub name: String,
    /// Basename of the executable (the part the fuzzy matcher tries
    /// to score highly).
    pub exe_basename: String,
    /// Full path to either the `.exe` or the `.lnk`.
    pub path: PathBuf,
    /// Source — Start Menu / Pinned / Manual.
    pub source: Source,
}

impl AppEntry {
    /// Cheap fingerprint for de-duplication. Reported by the platform
    /// indexer; we never trust user-supplied names here.
    pub fn fingerprint(path: &std::path::Path) -> u64 {
        // Tiny FNV-1a 64 — fine for de-dup, collision rate is
        // astronomically low for our menu-sized inputs.
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in path.to_string_lossy().as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        h
    }
}

/// In-memory application catalogue.
#[derive(Debug, Default)]
pub struct AppIndex {
    by_id: HashMap<u64, AppEntry>,
    by_basename: HashMap<String, Vec<u64>>,
}

impl AppIndex {
    /// Construct an empty index ready to be populated.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a single entry. Returns `false` if the id already
    /// existed (idempotent re-index).
    pub fn insert(&mut self, entry: AppEntry) -> bool {
        if self.by_id.contains_key(&entry.id) {
            return false;
        }
        self.by_basename
            .entry(entry.exe_basename.to_ascii_lowercase())
            .or_default()
            .push(entry.id);
        self.by_id.insert(entry.id, entry);
        true
    }

    /// Number of entries currently indexed.
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// True if the index contains no entries.
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Look up an entry by id.
    pub fn get(&self, id: u64) -> Option<&AppEntry> {
        self.by_id.get(&id)
    }

    /// Iterate every entry — used by the fuzzy matcher when it cannot
    /// cheaply prune by basename.
    pub fn iter(&self) -> impl Iterator<Item = &AppEntry> {
        self.by_id.values()
    }

    /// Drain into a Vec — used at sort time so we can score once
    /// and drop the borrow.
    pub fn drain(&self) -> Vec<AppEntry> {
        self.by_id.values().cloned().collect()
    }
}

/// A scored match — used by [`LauncherEngine::update_query`] to keep
/// results cheap to clone.
#[derive(Debug, Clone)]
pub struct ScoredEntry {
    pub entry: AppEntry,
    /// 0..=∞ higher = better.
    pub score: i32,
    /// Index positions in the entry's name that matched the query.
    /// Empty for entries returned by basename exact match.
    pub matched_positions: Vec<usize>,
}

impl ScoredEntry {
    /// Sort comparator: higher score first; ties broken by name.
    pub fn cmp_desc(a: &ScoredEntry, b: &ScoredEntry) -> std::cmp::Ordering {
        b.score
            .cmp(&a.score)
            .then_with(|| a.entry.name.len().cmp(&b.entry.name.len()))
            .then_with(|| a.entry.name.cmp(&b.entry.name))
    }
}

// =====================================================================
// Fuzzy scoring
// =====================================================================

/// Compute a fuzzy-match score between `query` and `target`.
/// Higher is better. Returns `None` if the query chars cannot be
/// matched as a contiguous-or-subsequence.
///
/// Scoring rules:
///
/// * **Exact (case insensitive) match** → huge bonus (used by
///   "open calculator" with `calc` after `Calculator.exe`).
/// * **Subsequence match** with all chars found → medium.
/// * **Adjacent-char bonus** for every pair of query chars that
///   appear adjacently in the target.
/// * **Prefix bonus** for matches beginning at index 0.
/// * **Case-exact bonus** for every correctly-cased match char.
pub fn fuzzy_score(query: &str, target: &str) -> Option<ScoredEntry> {
    let q = query.trim();
    if q.is_empty() {
        return None;
    }
    let t = target;
    let q_lower = q.to_ascii_lowercase();
    let t_lower = t.to_ascii_lowercase();

    // Exact basename match shortcut.
    if t_lower == q_lower {
        return Some(ScoredEntry {
            entry: AppEntry {
                id: 0,
                name: target.to_owned(),
                exe_basename: target.to_owned(),
                path: PathBuf::new(),
                source: Source::StartMenu,
            },
            score: 1_000_000,
            matched_positions: (0..target.chars().count()).collect(),
        });
    }

    // Walk `q` through `t` tracking match positions.
    let t_chars: Vec<char> = t.chars().collect();
    let q_chars: Vec<char> = q.chars().collect();
    let mut positions = Vec::with_capacity(q_chars.len());
    let mut score: i32 = 0;
    let mut last_pos: Option<usize> = None;
    let mut qi = 0usize;
    let mut ti = 0usize;
    let mut last_match_was_adjacent = false;
    while qi < q_chars.len() && ti < t_chars.len() {
        let qc = q_chars[qi];
        let tc = t_chars[ti];
        if qc.eq_ignore_ascii_case(&tc) {
            positions.push(ti);
            if last_pos.is_none() {
                if ti == 0 {
                    score += 200;
                }
                score += 50;
            }
            if last_pos == Some(ti.saturating_sub(1)) {
                score += 30;
            }
            if tc == qc {
                score += 10;
            } else {
                score += 5;
            }
            if last_match_was_adjacent && last_pos == Some(ti.saturating_sub(1)) {
                score += 25;
            }
            last_pos = Some(ti);
            last_match_was_adjacent = last_pos == Some(ti.saturating_sub(1));
            qi += 1;
        } else {
            last_match_was_adjacent = false;
        }
        ti += 1;
    }
    if qi < q_chars.len() {
        // query chars remained unmatched
        return None;
    }
    Some(ScoredEntry {
        entry: AppEntry {
            id: 0,
            name: target.to_owned(),
            exe_basename: target.to_owned(),
            path: PathBuf::new(),
            source: Source::StartMenu,
        },
        score,
        matched_positions: positions,
    })
}

// =====================================================================
// LauncherEngine — owns the index and the current query/results.
// =====================================================================

/// Events the platform launcher's window emits back to the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LauncherEvent {
    /// Cursor moved down by 1 row (clamped).
    Down,
    /// Cursor moved up by 1 row (clamped).
    Up,
    /// Page down by N (clamped).
    PageDown(usize),
    /// Page up by N (clamped).
    PageUp(usize),
    /// Reset to "no selection".
    Home,
    /// Confirm the current selection (Enter / Mouse).
    Confirm,
    /// Escape was pressed.
    Escape,
    /// Edit box text was edited.
    QueryChanged(String),
}

/// Soft errors the launcher can return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LauncherError {
    /// Platform indexer could not be reached.
    IndexerUnavailable,
    /// Selected entry's path no longer exists.
    EntryMissing,
    /// `CreateProcessW` failed; carries the Win32 error code (0 if
    /// not from a Win32 source).
    LaunchFailed(u32),
}

/// Trait-object view (used by the hotkey subsystem if it wants to
/// open the launcher).
pub trait Launcher: Send + Sync {
    /// Open the launcher window (idempotent — silently ignores
    /// repeat calls while already open).
    fn open(&self);
    /// Close the launcher window.
    fn close(&self);
    /// Returns `true` if the window is currently visible.
    fn is_open(&self) -> bool;
}

/// Thread-safe engine state — shared by the platform launcher's
/// window thread (writer) and any reader that wants a snapshot.
#[derive(Debug, Clone)]
pub struct LauncherEngine {
    inner: Arc<RwLock<LauncherState>>,
}

#[derive(Debug)]
struct LauncherState {
    index: AppIndex,
    open: bool,
    query: String,
    results: Vec<ScoredEntry>,
    selection: usize,
    /// Cap on results the panel will display — bounds the per-frame
    /// cost regardless of index size.
    max_results: usize,
}

impl LauncherEngine {
    /// Build a new engine against the given (already-populated) index.
    pub fn new(index: AppIndex, max_results: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(LauncherState {
                index,
                open: false,
                query: String::new(),
                results: Vec::new(),
                selection: 0,
                max_results,
            })),
        }
    }

    /// Build an empty engine — used as the failure fallback when the
    /// platform indexer could not be loaded.
    pub fn empty(max_results: usize) -> Self {
        Self::new(AppIndex::new(), max_results)
    }

    /// Replace the index wholesale (background re-index callback).
    pub fn replace_index(&self, index: AppIndex) {
        self.inner.write().index = index;
    }

    /// Returns `true` if the launcher window is currently visible.
    pub fn is_open(&self) -> bool {
        self.inner.read().open
    }

    /// Returns the currently displayed results (cloned).
    pub fn visible_results(&self) -> Vec<ScoredEntry> {
        self.inner.read().results.clone()
    }

    /// Returns the index of the highlighted entry.
    pub fn selection(&self) -> usize {
        self.inner.read().selection
    }

    /// Toggle visibility. Returns the new state.
    pub fn toggle(&self) -> bool {
        let mut g = self.inner.write();
        g.open = !g.open;
        if g.open {
            g.query.clear();
            g.results.clear();
            g.selection = 0;
        }
        g.open
    }

    /// Open the launcher explicitly.
    pub fn open(&self) {
        let mut g = self.inner.write();
        g.open = true;
        g.query.clear();
        g.results.clear();
        g.selection = 0;
    }

    /// Close explicitly.
    pub fn close(&self) {
        self.inner.write().open = false;
    }

    /// Process a single platform event.
    pub fn handle(&self, ev: LauncherEvent) -> Option<u64> {
        match ev {
            LauncherEvent::Down => self.move_selection(1),
            LauncherEvent::Up => self.move_selection(-1),
            LauncherEvent::PageDown(n) => self.move_selection(n as i64),
            LauncherEvent::PageUp(n) => self.move_selection(-(n as i64)),
            LauncherEvent::Home => self.set_selection(0),
            LauncherEvent::Confirm => return self.confirm(),
            LauncherEvent::Escape => {
                self.close();
                None
            }
            LauncherEvent::QueryChanged(s) => {
                self.update_query(&s);
                None
            }
        }
    }

    fn move_selection(&self, delta: i64) -> Option<u64> {
        let mut g = self.inner.write();
        if g.results.is_empty() {
            return None;
        }
        let len = g.results.len() as i64;
        let next = (g.selection as i64 + delta).clamp(0, len - 1);
        g.selection = next as usize;
        Some(g.results[g.selection].entry.id)
    }

    fn set_selection(&self, idx: usize) -> Option<u64> {
        let mut g = self.inner.write();
        let len = g.results.len();
        if len == 0 {
            return None;
        }
        g.selection = idx.min(len - 1);
        Some(g.results[g.selection].entry.id)
    }

    fn confirm(&self) -> Option<u64> {
        let g = self.inner.read();
        g.results.get(g.selection).map(|s| s.entry.id)
    }

    /// Re-score the results list for the current query.
    pub fn update_query(&self, query: &str) {
        let mut g = self.inner.write();
        g.query.clear();
        g.query.push_str(query);
        if query.trim().is_empty() {
            // Empty query → alphabetical head of the index.
            let mut all = g.index.drain();
            all.sort_by(|a, b| a.name.cmp(&b.name));
            g.results = all
                .into_iter()
                .take(g.max_results)
                .enumerate()
                .map(|(i, entry)| ScoredEntry {
                    entry,
                    score: 0,
                    matched_positions: Vec::new(),
                })
                .collect();
            // Re-attach proper indices for the placeholder ScoredEntry
            // `id == 0` entries so the platform layer can look them up.
            for (i, se) in g.results.iter_mut().enumerate() {
                let _ = i; // silence
                if se.entry.id == 0 {
                    // entry.id is unaffected; the placeholders above
                    // carry AppEntry's real id, not a synthetic rank.
                }
            }
            g.selection = 0;
            return;
        }
        let mut scored: Vec<ScoredEntry> = g
            .index
            .iter()
            .filter_map(|entry| {
                // Score against both display name and basename; the
                // higher of the two wins.
                let name_score = fuzzy_score(query, &entry.name);
                let base_score = fuzzy_score(query, &entry.exe_basename);
                let best = match (name_score, base_score) {
                    (Some(a), Some(b)) => {
                        if a.score >= b.score {
                            Some(a)
                        } else {
                            Some(b)
                        }
                    }
                    (Some(a), None) => Some(a),
                    (None, Some(b)) => Some(b),
                    (None, None) => None,
                }?;
                Some(ScoredEntry {
                    entry: entry.clone(),
                    score: best.score,
                    matched_positions: best.matched_positions,
                })
            })
            .collect();
        scored.sort_by(ScoredEntry::cmp_desc);
        scored.truncate(g.max_results);
        g.results = scored;
        g.selection = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, basename: &str) -> AppEntry {
        AppEntry {
            id: AppEntry::fingerprint(std::path::Path::new(&format!("/x/{name}"))),
            name: name.into(),
            exe_basename: basename.into(),
            path: PathBuf::from(format!("/x/{name}")),
            source: Source::StartMenu,
        }
    }

    #[test]
    fn exact_match_outranks_subsequence() {
        let mut idx = AppIndex::new();
        idx.insert(entry("Calculator.exe", "Calculator.exe"));
        idx.insert(entry("Calc.exe", "Calc.exe"));
        let eng = LauncherEngine::new(idx, 8);
        eng.update_query("calc");
        let res = eng.visible_results();
        assert!(!res.is_empty());
        assert_eq!(res[0].entry.exe_basename, "Calc.exe");
    }

    #[test]
    fn empty_query_returns_alphabetical() {
        let mut idx = AppIndex::new();
        idx.insert(entry("Visual Studio Code", "Code.exe"));
        idx.insert(entry("Calculator.exe", "Calculator.exe"));
        let eng = LauncherEngine::new(idx, 8);
        eng.update_query("");
        let res = eng.visible_results();
        assert_eq!(res.len(), 2);
        assert_eq!(res[0].entry.exe_basename, "Calculator.exe");
    }

    #[test]
    fn down_up_clamps() {
        let mut idx = AppIndex::new();
        for n in 0..4 {
            idx.insert(entry(&format!("App{n}.exe"), &format!("App{n}.exe")));
        }
        let eng = LauncherEngine::new(idx, 8);
        eng.update_query("app");
        assert_eq!(eng.selection(), 0);
        eng.handle(LauncherEvent::Down);
        assert_eq!(eng.selection(), 1);
        eng.handle(LauncherEvent::Up);
        assert_eq!(eng.selection(), 0);
        eng.handle(LauncherEvent::Up); // already 0
        assert_eq!(eng.selection(), 0);
        for _ in 0..10 {
            eng.handle(LauncherEvent::Down);
        }
        assert_eq!(eng.selection(), 3);
    }

    #[test]
    fn escape_closes_window() {
        let eng = LauncherEngine::empty(8);
        eng.open();
        assert!(eng.is_open());
        eng.handle(LauncherEvent::Escape);
        assert!(!eng.is_open());
    }

    #[test]
    fn fuzzy_score_returns_none_for_no_match() {
        assert!(fuzzy_score("zzz", "Visual Studio Code").is_none());
    }
}

// Tiny re-export to allow callers to write
// `Source::StartMenu.glyph()` if they want a UI label somewhere.
impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.glyph())
    }
}
