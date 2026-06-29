# JacqueWM Architecture

> Prompt 1 (workspace engine, hotkeys, virtual desktops, config,
> logging, startup) + Prompt 2 Part 1 (top panel UI) + Prompt 2 Part 2
> (event-driven WindowManager, tiling engine, application rules) +
> Prompt 3 (launcher, settings, theme, tray, notifications) + Prompt
> 4 (installer, debug manager, plugin architecture).

---

## 1. High-Level Diagram

```
                +----------------------------+
                |       binary entry          |
                |  jacquewm.exe (main)        |
                |  jacquewm-installer.exe     |
                +-------------+--------------+
                              |
                              v
       +----------------------------------------------------+
       |                       j acque w m (lib)            |
       |   five Prompt-1 + five Prompt-2 + seven Prompt-3/4 |
       |   subsystems under core::                          |
       +-----------------+-----------------+----------------+
                         |                 |
                         v                 v
             +------------------------+   +-----------+
             | platform::windows::*   |   | bin/*     |
             +------------------------+   +-----------+
```

---

## 2. Subsystem Inventory

### Prompt 1 (stable)

| Module | Responsibility |
|--------|---------------|
| `core::config`  | TOML loader + live reload. |
| `core::logging` | `tracing` + daily-rotated file. |
| `core::virtual_desktop` | OS-agnostic trait + adapter contract. |
| `core::workspaces` | Engine that keeps the nine-desktop invariant. |
| `core::windows` | Thin “focused-window” helper used by the hotkey dispatcher. |
| `core::hotkeys` | Action enum, keymap, dispatcher. |
| `core::startup` | Lifecycle phase pointer. |

### Prompt 2 (Part 1 — Top panel)

| Module | Responsibility |
|--------|---------------|
| `core::panel` | Omarchy-style three-section top panel (data model + renderer host). |

### Prompt 2 (Part 2 — Tiling engine)

| Module | Responsibility |
|--------|---------------|
| `core::wm` | Event-driven WindowManager (typed `WindowId`, `WindowState`, full event history). |
| `core::tiling` | LCRS tree, recursive layout solver, smart-gaps, safe equal-grid fallback. |
| `core::apps` | Pipeline for Basename/Class/Title/Transient rules *before* the layout pass. |
| `core::focus` | FocusTracker consumed by the panel CENTER section. |
| `core::metrics` | `CpuSample` / `GpuSample` / `RamSample` / `NetSample` + rolling-mean helper. |

### Prompt 3 + 4 (this milestone)

| Module | Responsibility |
|--------|---------------|
| `core::settings`  | Live config reload (orchestrator + observers). |
| `core::theme`     | `ThemeManager` with category-targeted observers. |
| `core::launcher`  | In-memory app index + fuzzy matcher + `LauncherEngine`. |
| `core::notifications` | In-app toast queue (`NotificationManager`). |
| `core::tray`      | Tray-action enum + state holder. |
| `core::debug`     | Snapshot API gated by `config.debug.debug_mode`. |
| `core::plugins`   | Trait + manifest (design only). |
| `core::isolation` | `SubsystemHealth` registry + `safe_init` helper. |

---

## 3. Threading Model

| Thread | Owns | Pumps | Allowed to sleep? |
|--------|------|-------|-------------------|
| **main** | Workspace engine, COM STA pointer, legacy `HotkeyManager`, `WindowsKeyboardHook`, WinEventHook callbacks, message-only window, tray HWND | `GetMessageW` / `DispatchMessageW` | No |
| **panel** | `WindowsPanelHost`, Direct2D factory, DWrite factory | own `GetMessageW` | No |
| **launcher** | `LauncherEngine`, popup window | own `GetMessageW` | No |
| **notifications** | `WindowsNotificationHost`, popup windows | `WM_TIMER`-driven | No |
| **tray** | `Shell_NotifyIconW` icon (registration only) | none (state changes are tid) | No |
| **metrics** | `MetricsCollector` poller | `WM_TIMER` (1 Hz) | No |
| **settings watcher** | `notify::RecommendedWatcher` + debouncer | blocking `recv` from notify channel | No |
| **installer startup** | cache pre-warm | one-shot | OK |

Every worker thread is wrapped in `core::isolation::safe_init` so a
panic only kills that one thread and the `SubsystemHealth` registry
records `Health::Dead`. **No** cross-thread parent/child windows
exist (the panel is a top-level `WS_POPUP`) so we never call
`AttachThreadInput` (which would deadlock).

---

## 4. Data Flow

```
   +-------+      SetWinEventHook        +------------+
   | Win32 | --------------------------> | hook.rs    |
   +-------+      (main thread)          +-----+------+
                                              |
                                              v
                                       Registry::apply
                                              |
                                              v  fan-out
   +-------------+   snapshot    +-------------+    apply    +-------------+
   | Panel state | <------------ | focus.update|<-----------| wm.apply    |
   +-------------+               +-------------+            +-------------+
            |                                                     |
            v                                                     v
   +------------------+   redraw    +-------------+  layout  +-------------+
   | WindowsPanelHost | <--------- | panel.host  | <------- | tiling.eng   |
   +------------------+            +-------------+          +-------------+
```

Subsystems publish state through `Arc<RwLock<…>>`; the *platform*
layer translates those into Win32 calls. No callback mutates a
subsystem it didn't own.

---

## 5. Failure Isolation

`core::isolation::safe_init` wraps every worker thread closure in
`std::panic::catch_unwind(AssertUnwindSafe(...))`. On panic:

```text
1. Worker thread unwinds → Rust cleans thread-owned resources.
2. RAII drops run for any guarded handles (DComp device, HDC, …).
3. `SubsystemHealth::mark_dead(name, reason, panic_thread)`.
4. The main thread continues, never observes the panic directly.
5. DebugManager surfaces it in `--dump` output.
```

Nothing auto-restarts. Per the spec:

> "If ANY subsystem fails → degrade gracefully → log error → disable
> only its own feature set → the rest of JacqueWM must remain
> functional."

`Health::Dead` subsystems never auto-resurrect — a future config
edit plus a manual restart is the documented recovery path.

---

## 6. Hot-Path Bottlenecks

| Location | Today | Mitigations |
|----------|-------|-------------|
| WM event hook callback (`hook.rs`) | O(1) per event; builds `WindowMetadata` and applies | No allocation on the dirt paths. |
| Panel redraw | 0 if `dirty == false` | Driven by `mark_dirty()` only. |
| Launcher fuzzy match | O(N·len(query)) per keystroke where N = indexed app count | `take(max_results)` then sort. |
| Settings live-reload | 500 ms debouncer | `notify::debouncer` does the heavy lifting. |
| Tile layout | O(leaves) per recalc | DFS, no allocations after warmup. |

---

## 7. Plugin Architecture (design only)

`core::plugins` exposes:

```rust
pub trait JacquePlugin: Send + Sync {
    fn id(&self) -> &str;
    fn on_load(&self);
    fn on_unload(&self);
    fn on_workspace_change(&self, from: u8, to: u8) {}
    fn on_window_event(&self, kind: &WindowHookKind) {}
}
```

Plugins register at compile time via a future `PluginRegistry`. The
manifest is TOML:

```toml
id = "acme/jacqlint"
name = "Jacqlint"
author = "acme"
version = "0.1.0"
profile = "launcher"   # core | theme | launcher
permissions = ["read:workspaces"]
jacquewm_version = "^0.1.0"
```

**No** runtime library loading, **no** filesystem scanner, **no**
sandbox primitive. Rust’s borrow checker *is* the sandbox.

---

## 8. Installer Release

`src/bin/installer.rs` produces a *second* binary:

```
jacquewm-installer.exe
```

Subcommands:

* `install [--dir DIR] [--auto-start]`
* `portable [--dir DIR]`
* `uninstall [--dir DIR]`

It writes only to `HKEY_CURRENT_USER\…\Run`. The portable build is
the *primary* distribution method; the installer is convenience.

---

## 9. Lifecycle (Prompt 1 boot, with Prompt 3/4 hooks)

```text
PHASE 1 - pre_init → registry mode + priority class
PHASE 2 - wait_for_explorer → load config → load_settings_manager
PHASE 3 - com_init (STA)
PHASE 4 - acquire IVirtualDesktop* pointers
PHASE 5 - workspace engine ready
PHASE 6 - ensure nine desktops, switch to Desktop 1
PHASE 7 - load keymap, install keyboard hook (main thread)
PHASE 8 - rich WindowManager + tiling engine + rules engine + focus tracker
PHASE 9 - panel thread spawn (separate thread; rust_isolation::safe_init)
PHASE 10 - launcher / tray / notifications hooks (gated by config)
PHASE 11 - GETMESSAGEW loop drains hotkey channel + tray messages + settings changes
```

Each numbered phase is non-fatal; the next phase starts even if the
last one errored. Subsystems that fail to install mark themselves
`Health::Disabled` (config off) or `Health::Dead` (panic) in the
isolation registry.
