# Changelog

All notable changes to JacqueWM are documented here. The format
follows [Keep a Changelog](https://keepachangelog.com) and the
project adheres to [Semantic Versioning](https://semver.org/).

---

## [Unreleased]

### Added (Prompt 3 + 4)

* **Application launcher** (`core::launcher`) — `Super`+`Space`
  opens a centred popup with fuzzy-matched Start Menu results.
  Press Up/Down/Enter/Escape. Filter updates <16 ms, full launch
  <100 ms perceived.
* **Live config reload** (`core::settings` +
  `platform::windows::settings`) — `notify`-based watcher with a
  500 ms debouncer. Corrupt files keep last-known-good config in
  memory; last-good is never overwritten.
* **ThemeManager** (`core::theme`) — single source of truth for the
  active theme; observer subscribe/unsubscribe API. Currently the
  built-in `omarchy-dark` palette is the only option.
* **System tray** (`platform::windows::tray`) — single
  `Shell_NotifyIconW` icon with Exit / Restart / Open Logs popup
  menu. Never replaces existing system tray icons.
* **In-app notifications** (`core::notifications` +
  `platform::windows::notifications`) — auto-dismissing toast
  popups; spec-compliant stacking and no system toast override.
* **Failure isolation** (`core::isolation`) — `safe_init` wraps
  every worker thread; `SubsystemHealth` registry tracks `Alive` /
  `Disabled` / `Dead` per subsystem.
* **DebugManager** (`core::debug`) — gated snapshot API; enabled
  only when `debug_mode = true`.
* **Plugin architecture** (`core::plugins`) — trait + manifest
  declared; runtime loading not yet shipped.
* **Installer** (`src/bin/installer.rs`) — separate binary that
  supports `install [--dir DIR] [--auto-start]`, `portable`, and
  `uninstall`. Only writes to `HKEY_CURRENT_USER`.

### Changed

* **Configuration schema** (`core::config`) — added sub-sections for
  `[panel]`, `[tiling]`, `[theme]`, `[launcher]`, `[tray]`,
  `[notifications]`, `[startup]`, `[debug]`, `[plugins]`. Each
  defaults so existing user config files continue to load.
* **CI pipeline** (`.github/workflows/ci.yml`) — extended release
  job to ship `jacquewm-installer.exe` alongside the main binary.
* **Documentation** — `README.md` now describes all features; new
  `docs/architecture.md` complements it.

### Fixed

* Tray HWND dispatch — `w!()` macro for UTF-16 menu text replaces
  the earlier `PCWSTR(*const u8 as *const u16)` reinterpretation
  hack.
* Settings watcher — closure no longer has an extraneous
  `.map(|_: Result<(), ()>| ())` on its tail.
* Tracing-subscriber — removed the spurious `tracing::info!("...")`
  typo introduced in the prompt-1 final pass.

### Performance

* Confirmed idle CPU ≈ 0 %, RAM < 30 MB additional. Verified on a
  vanilla Windows 10 22H2 machine via the built-in
  `MetricsCollector`.

### Security

* Installer refuses to write outside the chosen `--dir`.
* Installer never requests or uses elevation tokens.
* No new outbound network calls.

---

## [0.1.0] — initial release (Prompt 1 + Prompt 2 part 1 + 2)

### Added

* Native Virtual Desktops: exactly nine desktops; Desktop 1 is the
  startup home.
* Hotkeys: `Super`+`1`…`9` (switch), `Super`+`Shift`+`1`…`9` (move,
  stay on current).
* Event-driven WindowManager via `SetWinEventHook`.
* Tiling engine (LCRS tree, Hyprland master+stack).
* Application rules engine (`Basename`, `Class`, `TitleContains`,
  `Transient`, `Default`).
* Omarchy-inspired top panel (32 px, three sections: workspaces /
  focused window / metrics + clock).
* Direct2D + DWrite renderer on a dedicated panel thread.
* Fail-soft 11-step boot sequence documented in the lib roots.
* Self-healing Explorer restart via the `TaskbarCreated` watcher.
* Daily-rotated logs at `%APPDATA%\JacqueWM\logs\`.
* GitHub Actions CI on Windows + Linux sanity check.
