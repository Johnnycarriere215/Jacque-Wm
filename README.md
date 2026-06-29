# JacqueWM

> A modern Windows workspace manager. Hyprland-style keyboard navigation,
> native Windows Virtual Desktops, no shell replacement.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Windows 10+](https://img.shields.io/badge/Windows-10%2B-blue.svg)](https://www.microsoft.com/windows/)
[![Rust 1.75+](https://img.shields.io/badge/Rust-1.75%2B-orange.svg)](https://rust-lang.org/)
[![Portable](https://img.shields.io/badge/distribution-portable-brightgreen.svg)](docs/architecture.md)

JacqueWM delivers Linux-style, super-key-driven workspace navigation
to Windows 10 using the OS's *built-in* Virtual Desktops. No custom
shell, no replacement for `explorer.exe`, no virtualisation. The
result feels like Hyprland running natively on Windows.

---

## Highlights (Prompt 1 + 2 + 3 + 4)

| Subsystem | Status | Notes |
|-----------|--------|-------|
| 9 workspaces on native Virtual Desktops | Done | `Super`+`1`…`9` |
| Focused-window move without following | Done | `Super`+`Shift`+`1`…`9` |
| Event-driven WindowManager | Done | `SetWinEventHook` on main thread |
| Tiling engine (LCRS tree, Hyprland master+stack) | Done | Smart-gaps, divider drag |
| Application rules (`calculator.exe` → Float, …) | Done | Hot-reloadable |
| Omarchy-inspired top panel (32 px, three sections) | Done | Direct2D + DWrite |
| System metrics (CPU / GPU / RAM / Net) | Done | `IPHLPAPI` + DXGI |
| Live config reload | Done | `notify` + `notify-debouncer-full` |
| App launcher (`Super`+`Space`, fuzzy) | Done | Start Menu index |
| Settings / Theme observers | Done | Sub-section live-reload safe |
| Tray (Exit / Restart / Open Logs) | Done | Single `Shell_NotifyIconW` |
| In-app toast notifications | Done | No system toast override |
| Failure isolation (panic per-subsystem) | Done | `safe_init` helper |
| Installer (portable + optional HKCU auto-start) | Done | `jacquewm-installer.exe` |
| Plugin architecture (design only) | Done | Trait + manifest; no runtime loading |
| DebugManager (gated by `debug_mode = true`) | Done | Snapshot API |

Every feature above ships behind a `debug_mode = false` default so the
release binary remains quiet and clean.

---

## Hotkeys (default)

| Shortcut | Action |
|----------|--------|
| `Super` + `1`…`9` | Switch to workspace `N` |
| `Super` + `Shift` + `1`…`9` | Move focused window to workspace `N` |
| `Super` + `Space` | Toggle app launcher |

Promoted to TOML hot-reload in a future milestone; for now
`mod.rs::default_keymap()` is the canonical source.

---

## Installation

### Portable (recommended)

Download the ZIP from the
[releases page](https://github.com/jacquewm/jacquewm/releases),
unzip anywhere, double-click `jacquewm.exe`. Zero registry, zero
admin, zero uninstall pain — delete the folder when done.

### Installer

```pwsh
# Default install (copies into %APPDATA%\JacqueWM)
.\jacquewm-installer.exe install

# Custom directory + boot-on-login (opt-in HKCU Run key)
.\jacquewm-installer.exe install --dir C:\Tools\JacqueWM --auto-start

# Uninstall
.\jacquewm-installer.exe uninstall
```

The installer is a separate binary that ships with the same release
ZIP. It only writes to `HKEY_CURRENT_USER\…\Run` and never to
`HKEY_LOCAL_MACHINE` or system files. Portable build remains primary.

---

## Activating JacqueWM

> First launch, verification, and the most common "it's not working"
> fixes. Read this once. You probably won't need to again.

### 1. Pick a launch method

You have two main options; both are first-class and equivalent in
runtime behaviour. Pick portable if you don't know which to choose.

| Mode | What you do | What stays behind |
|------|-------------|--------------------|
| **Portable** | Double-click `jacquewm.exe` (or run it from a terminal) | Nothing — delete the folder to fully remove |
| **Installer** | Run `jacquewm-installer.exe install` once | Optional `HKCU\…\Run` value if you passed `--auto-start`; copy in the chosen `--dir` |

### 2. First-launch checklist

When `jacquewm.exe` starts, the 11-step boot sequence runs in this
order:

1. **Pre-init** — process priority + COM error mode are tuned.
2. **Wait for Explorer** — up to 30 s (configurable).
3. **Initialise logger** — writes daily-rotated logs under
   `%APPDATA%\JacqueWM\logs\`.
4. **Load config** — `config.toml` from `%APPDATA%\JacqueWM\` or the
   path in the `JACQUEWM_CONFIG` env var. Missing file → defaults
   are written and loaded.
5. **COM apartment** — STA registered on the main thread.
6. **Acquire task-view pointers** — `IVirtualDesktop*` interfaces.
7. **Build workspace engine** + ensure the nine desktops exist.
8. **Switch to Desktop 1** (your `startup_desktop` from config).
9. **Build window manager + dispatcher.**
10. **Install low-level keyboard hook.**
11. **Begin the message loop** — drains hotkeys between every
    Windows message.

A successful first run shows:

* A **30 px dark bar across the very top of every monitor** with
  nine workspace pills (the active one is white on dark; the rest
  are transparent with muted text).
* The title of your currently focused window in the centre of the
  bar, or `Desktop` if nothing has focus.
* CPU / GPU / RAM / Net + clock on the right.
* A single tray icon near the system clock.
* `jacquewm.exe` in Task Manager, idle at ~0 % CPU.

If you see all four: it's active.

### 3. Try the default hotkeys

```pwsh
Win + 1      # switch to workspace 1
Win + 2      # switch to workspace 2
Win + Shift + 3   # move the focused window TO workspace 3 (you stay put)
Win + Space  # open the app launcher
```

`Win` here is the keyboard `Super` key (the Windows key).

If the workspaces switch and the panel pill highlights the new
number, hotkeys are firing on the right thread. If not, jump to
**Troubleshooting** below.

### 4. Edits to config? They apply live.

`config.toml` is watched by a `notify`-based file watcher with a
500 ms debouncer. Save the file with your editor and the new values
take effect without a restart — *except* the top-level
`enable_logging` switch, the workspace count, and the
Windows-keymap, which require a manual restart (the boot sequence
only initialises them once). The reload is logged at info level:

```
INFO jacquewm.settings: settings watcher installed
INFO jacquewm.config:  configuration reloaded
```

A malformed file keeps the last-known-good config in memory and
shows a toast warning — the file on disk is *not* touched.

### 5. Stopping / uninstalling

* **Stop**: focus anywhere and run via the tray → Exit, or just
  right-click `jacquewm.exe` in Task Manager and End Task. Windows
  is left exactly as it was.
* **Portable uninstall**: delete the folder. Done.
* **Installer uninstall**:

  ```pwsh
  jacquewm-installer.exe uninstall --dir "C:\path\to\install"
  ```

  Removes the directory tree and the auto-start value (if it was
  registered). `HKLM`, services, drivers, and shell components are
  untouched.

### 6. Troubleshooting

| Symptom | Likely cause | Fix |
|---------|-------------|-----|
| Panel never appears | `enable_logging = false` AND `wait_for_explorer = true` AND Explorer did not become ready | Set `wait_for_explorer = false` in `config.toml`, or wait the full 30 s. |
| Hotkeys do nothing | Keyboard hook could not install (some anti-cheat drivers block it) | Run as your normal user again; check `%APPDATA%\JacqueWM\logs\` for `failed to install keyboard hook`. |
| Tray icon missing | Explorer's notification area is busy | Restart Explorer from the Task Manager; the icon will re-register next boot. |
| `jacquewm.exe` exits immediately | A subsystem panicked at boot | Read the most recent `jacquewm.YYYY-MM-DD.log` under `%APPDATA%\JacqueWM\logs\` — the `safe_init` helper logs the panic context next to the dead subsystem name. |
| "Could not acquire virtual desktop interfaces" | Build mismatch (Win10 < 1903 lacks the IIDs we hard-code) | Update Windows. Win10 1903+ is required. |
| Live reload "silently" ignored an edit | TOML parse error in the file | Open the file; check syntax. The toast popup describes the error and we keep the *last good* in memory. |
| LAN install on multiple machines | Use the portable ZIP + a single bash command per machine | Each machine runs the binary from its own folder; nothing shared. |

If `:9001`-ish errors appear, it is **not** JacqueWM — that port is
a Windows component. File an issue with the log attached.

### 7. Verify the safety guarantees still hold

Before you trust JacqueWM with a real session, spot-check that the
binary you ran really is the one from this repo:

```pwsh
# 1. Confirm the binary does not contain unexpected embedded data.
Get-FileHash jacquewm.exe -Algorithm SHA256
# 2. List the live registry entries JacqueWM actually wrote.
Get-ItemProperty "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" |
    Where-Object { $_.JacqueWM -ne $null }
# 3. List JacqueWM's extra processes — should be exactly one.
Get-Process jacquewm -ErrorAction SilentlyContinue
```

If there is **more than one** `jacquewm.exe` in Task Manager, you
have an old build still running — kill it, then start the new binary.

---

## Configuration

Default path: `%APPDATA%\JacqueWM\config.toml`. Every field
documents itself via comments in
[`examples/config.toml`](examples/config.toml).

Top-level fields:

```toml
startup_desktop       = 1
workspace_count       = 9
follow_moved_windows  = false
enable_logging        = true
log_filter            = ""
```

Sub-sections (`[panel]`, `[tiling]`, `[theme]`, `[launcher]`,
`[tray]`, `[notifications]`, `[startup]`, `[debug]`, `[plugins]`)
all default to safe values; you can omit any of them.

---

## CLI

```
jacquewm.exe                  # run normally
jacquewm.exe --register       # write HKCU\…\Run value
jacquewm.exe --unregister     # remove the Run value
jacquewm.exe --version        # print version
jacquewm-installer.exe install [--dir DIR] [--auto-start]
jacquewm-installer.exe portable [--dir DIR]
jacquewm-installer.exe uninstall [--dir DIR]
```

---

## Performance

| Metric | Target | Notes |
|--------|--------|-------|
| Idle CPU | ~0 % | Panel only repaints when state changes. |
| Idle RAM | < 30 MB additional | Excludes Windows baseline. |
| Total RAM (JacqueWM) | < 100 MB | Verified with built-in `MetricsCollector`. |
| Hotkey reaction | < 50 ms | Channel + `GetMessageW` drain. |
| Workspace switch | < 100 ms | Native `IVirtualDesktop` call. |
| Panel refresh | 60 FPS / native refresh | Dirty-flag driven, no busy loop. |

---

## Safety Guarantees (Prompt 1 + 2 + 3 + 4)

* **No Explorer replacement.** Explorer's process tree is left
  alone.
* **No DLL injection, no drivers, no kernel calls, no services.**
* **No admin elevation required.** All registry writes are
  `HKEY_CURRENT_USER` only.
* **Crash safety.** A panic in one subsystem (launcher / panel /
  tray / settings watcher) is contained to its thread; the rest of
  JacqueWM keeps working. Closing the binary leaves Windows exactly
  as it was. Deleting the install folder is the complete uninstall.

---

## Architecture

See [`docs/architecture.md`](docs/architecture.md) for the full
diagram and the per-module description. Five *core* subsystems plus
five *Prompt 3/4* subsystems, all running on the main STA thread or
dedicated worker threads (no cross-thread parent/child windows).

```text
+--- core (OS-agnostic) ---+
| config  logging  virtual_desktop  workspaces  windows  hotkeys  startup |
| apps  focus  metrics  panel  tiling  wm                                |
| debug  isolation  launcher  theme  notifications                       |
| plugins  settings  tray                                                |
+--------------------------------------------------------------------+
| platform/windows/* — Win32 + COM, Direct2D, hooks, file watcher       |
| bin/installer.rs — standalone safe installer                           |
+--------------------------------------------------------------------+
```

---

## Development

Requires [Rust 1.75+](https://rustup.rs/) and Windows 10+.

```pwsh
cargo build                # debug
cargo build --release      # size-optimised binary
cargo test                 # unit + integration tests
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
```

---

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for code-style rules and the
PR workflow.

## Changelog

See [`CHANGELOG.md`](CHANGELOG.md).

---

## License

[MIT](LICENSE).
