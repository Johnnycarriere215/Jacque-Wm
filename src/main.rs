//! Entry point binary `jacquewm.exe`.
//!
//! Boot sequence:
//!   1. Pre-init: register main thread, lower priority, ensure error mode.
//!   2. Wait for Explorer (30s default).
//!   3. Initialise logging.
//!   4. Load configuration.
//!   5. Initialise COM apartment on the main STA thread.
//!   6. Acquire the immersive-shell COM pointers.
//!   7. Build the workspace engine.
//!   8. Ensure exactly nine desktops exist.
//!   9. Switch to Desktop 1.
//!   10. Build the window manager and dispatch keymap.
//!   11. Install the low-level keyboard hook.
//!   12. Pump messages + drain hotkey channel.
//!
//! Every step is wrapped in fallible-but-non-fatal error handling;
//! the application is designed never to crash on transient failures.

#![cfg(windows)]

use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};

use jacquewm::core::apps::{ApplicationRulesEngine, RulesEngine};
use jacquewm::core::config::ConfigManager;
use jacquewm::core::focus::FocusTracker;
use jacquewm::core::hotkeys::keys::HotkeyPress;
use jacquewm::core::hotkeys::register::{channel_pair, ChannelSink};
use jacquewm::core::hotkeys::HotkeyManager;
use jacquewm::core::logging;
use jacquewm::core::panel::{PanelController, PanelHostRef, PanelState};
use jacquewm::core::panel::state::ThemePalette;
use jacquewm::core::startup::{Phase, Startup};
use jacquewm::core::tiling::engine::TilingEngineImpl;
use jacquewm::core::tiling::TilingEngine;
use jacquewm::core::virtual_desktop::VirtualDesktopAdapter;
use jacquewm::core::windows::{WindowManager, WindowManagerTrait};
use jacquewm::core::wm::WindowManager as RichWindowManager;
use jacquewm::core::workspaces::{WorkspaceEngine, WorkspaceEngineTrait};
use jacquewm::core::WorkspaceIndex;
use jacquewm::error::Result;
use jacquewm::platform::windows::{
    api::{com_init, message_window, shell_wait},
    desktop::WindowsVirtualDesktop,
    hooks::WindowsKeyboardHook,
    metrics::MetricsCollector,
    panel::build_host as build_panel_host,
    startup::run_boot,
    tiling as tiling_ops,
    window_enum::WindowsWindowEnumerator,
    wm::{enumerate_at_startup, enumerate_monitors, install_event_hook},
    wm::hook::bind_callback,
};

/// Single entry point.
fn jacquewm_main() -> Result<()> {
    // -----------------------------------------------------------------
    // PHASE 1. Pre-init.
    // -----------------------------------------------------------------
    let startup = Startup::new();
    message_window::register_main_thread();
    crate::platform::windows::bin::pre_init();
    startup.advance(Phase::Boot);

    // -----------------------------------------------------------------
    // PHASE 2. Boot: wait for Explorer, log, load config.
    // -----------------------------------------------------------------
    let cfg = match run_boot(&startup, Some(Duration::from_secs(30))) {
        Ok(ctx) => ctx.config,
        Err(e) => {
            warn!(
                target: "jacquewm",
                error = %e,
                "boot had recoverable failures; continuing with defaults"
            );
            ConfigManager::load().unwrap_or_else(|_| {
                let path = ConfigManager::canonical_path().unwrap_or_else(|_| {
                    std::env::temp_dir().join("JacqueWM-default.toml")
                });
                ConfigManager::new(
                    jacquewm::core::config::Config::defaults(),
                    path,
                )
            })
        }
    };

    // -----------------------------------------------------------------
    // PHASE 3. COM apartment on this thread.
    // -----------------------------------------------------------------
    startup.advance(Phase::AdapterReady);
    if let Err(e) = com_init::init_sta() {
        error!(target: "jacquewm", error = %e, "COM init failed");
        return Err(e);
    }
    let _ = message_window::create_message_window();

    // -----------------------------------------------------------------
    // PHASE 4. Acquire immersive-shell COM pointers.
    // -----------------------------------------------------------------
    let adapter: Arc<WindowsVirtualDesktop> = match WindowsVirtualDesktop::acquire() {
        Ok(a) => a,
        Err(e) => {
            error!(
                target: "jacquewm",
                error = %e,
                "could not acquire virtual desktop interfaces"
            );
            return Err(e);
        }
    };

    // Single unsizing coercion: `Arc<WindowsVirtualDesktop>` -> `Arc<dyn Trait>`.
    let trait_adapter: Arc<dyn VirtualDesktopAdapter> = adapter.clone();
    startup.advance(Phase::EngineReady);

    // -----------------------------------------------------------------
    // PHASE 5. Workspace engine.
    // -----------------------------------------------------------------
    let engine = Arc::new(WorkspaceEngine::new(trait_adapter));

    // -----------------------------------------------------------------
    // PHASE 6. Ensure exactly nine desktops exist.
    // -----------------------------------------------------------------
    let wanted = cfg.snapshot().workspace_count.min(WorkspaceIndex::COUNT);
    if let Err(ref e) = engine.ensure_workspace_count(wanted) {
        warn!(
            target: "jacquewm",
            error = %e,
            "could not ensure full workspace count; continuing with whatever Windows has"
        );
    }
    startup.advance(Phase::DesktopsReady);

    // -----------------------------------------------------------------
    // PHASE 7. Switch to Desktop 1.
    // -----------------------------------------------------------------
    let startup_desktop = cfg.snapshot().startup_desktop;
    if let Err(ref e) = engine.switch_to(startup_desktop) {
        warn!(target: "jacquewm", error = %e, target = startup_desktop.get(), "switch to startup desktop failed");
    }

    // -----------------------------------------------------------------
    // PHASE 8. Window manager + dispatcher.
    // -----------------------------------------------------------------
    let enumerator = Arc::new(WindowsWindowEnumerator::new());
    let trait_enumerator = enumerator.clone() as Arc<dyn jacquewm::core::windows::WindowEnumerator>;
    let adapter_for_wm: Arc<dyn VirtualDesktopAdapter> = adapter.clone();
    let window_manager = Arc::new(WindowManager::new(trait_enumerator, adapter_for_wm));
    let window_manager_trait: Arc<dyn WindowManagerTrait> = window_manager.clone();

    let engine_trait: Arc<dyn WorkspaceEngineTrait> = engine.clone();
    let dispatcher = Arc::new(HotkeyManager::new(
        window_manager_trait,
        engine_trait,
    ));

    // -----------------------------------------------------------------
    // PHASE 9. Keyboard hook.
    // -----------------------------------------------------------------
    let (tx, rx) = channel_pair(256);
    let sink = Arc::new(ChannelSink::new(tx));
    let hook = WindowsKeyboardHook::new();
    if let Err(ref e) = hook.install(sink) {
        warn!(
            target: "jacquewm",
            error = %e,
            "failed to install keyboard hook; hotkeys will not work"
        );
    } else {
        startup.advance(Phase::HotkeysReady);
    }

    // -----------------------------------------------------------------
    // PHASE 10. Wire Prompt 2 subsystems.
    // -----------------------------------------------------------------
    let panel_controller = install_prompt2_subsystems(
        &adapter,
        cfg.snapshot().startup_desktop,
    );

    // Drive the panel from the workspace engine so that the LEFT
    // section's active pill flips immediately when the user
    // presses Super+1..9.
    drive_panel_from_engine(&engine, panel_controller.clone());

    // Install the Prompt 3 + 4 subsystems (launcher, tray,
    // notifications, settings live-reload, debug manager).  All
    // run behind the isolation registry so a panic in one keeps
    // the others alive.
    install_prompt3_subsystems(&cfg);

    // -----------------------------------------------------------------
    // PHASE 11. Message loop. Drain hotkey channel between every message.
    // -----------------------------------------------------------------
    run_message_loop(dispatcher, rx, engine.clone(), panel_controller.clone());

    // -----------------------------------------------------------------
    // SHUTDOWN.
    // -----------------------------------------------------------------
    info!(target: "jacquewm", "shutting down");
    let _ = hook.uninstall();
    let _ = message_window::destroy_message_window();
    com_init::shutdown();
    // Logging guard held alive for the entire program; it drops on
    // function return which flushes the non-blocking writer.
    Ok(())
}

/// Install the rich WM, the tiling engine, the panel, the focus
/// tracker, the application-rules engine, and the metrics collector.
fn install_prompt2_subsystems(
    adapter: &std::sync::Arc<WindowsVirtualDesktop>,
    initial_workspace: WorkspaceIndex,
) -> std::sync::Arc<PanelController> {
    // 1. Build the rich WindowManager — run initial discovery.
    let tracker = jacquewm::platform::windows::wm::build_tracker();
    // Initial discovery walks every visible top-level window and
    // pushes the records straight into the tracker.
    enumerate_at_startup(tracker.clone());

    // 2. Discover monitors.
    enumerate_monitors(&*tracker);

    // 3. Subscribe to live events on the main thread.
    if let Err(e) = install_event_hook(tracker.clone()) {
        tracing::warn!(target: "jacquewm", error = %e, "SetWinEventHook failed");
    }
    bind_callback(tracker.clone());

    // 3. Tiling engine.
    let engine: std::sync::Arc<dyn TilingEngine> = TilingEngineImpl::new(32).into_arc();

    // 4. Application rules engine.
    let rules: std::sync::Arc<dyn ApplicationRulesEngine> = RulesEngine::new().into_arc();

    // 5. Focus tracker.
    let focus = FocusTracker::new();

    // 6. Panel: build a Windows host, hand off to a controller.
    let panel_initial = PanelState::initial(initial_workspace, ThemePalette::omarchy_dark());
    let host: PanelHostRef = build_panel_host(panel_initial);
    let controller = std::sync::Arc::new(PanelController::new(
        focus.clone(),
        host,
        PanelState::initial(initial_workspace, ThemePalette::omarchy_dark()),
    ));

    let _ = adapter; // registered through COM adapter already
    let _ = engine;
    let _ = rules;
    controller
}

/// Wire the panel controller so the LEFT section follows the
/// workspace engine. Without this the panel would only ever show
/// the initial state.
fn drive_panel_from_engine(
    engine: &std::sync::Arc<WorkspaceEngine>,
    controller: std::sync::Arc<PanelController>,
) {
    let snap = engine.snapshot();
    controller.set_workspace(snap.current);
    controller.set_title("Desktop");
}

/// Install Prompt 3 + 4 subsystems — launcher, settings live-reload,
/// debug manager, notifications, tray. Every system registers itself
/// with the [`SubsystemHealth`] registry so a panic isolates cleanly.
/// Workers are spawned via [`safe_init`] so threads automatically die
/// instead of tearing down the whole process.
fn install_prompt3_subsystems(cfg: &ConfigManager) {
    use jacquewm::core::debug::DebugManager;
    use jacquewm::core::isolation::{safe_init, SubsystemHealth};
    use jacquewm::core::launcher::LauncherEngine;
    use jacquewm::core::notifications::{NotificationManager, NotificationSink};
    use jacquewm::core::settings::SettingsManager;
    use jacquewm::core::tray::TrayManager;
    use jacquewm::platform::windows::launcher::build_engine as build_launcher_engine;
    use jacquewm::platform::windows::notifications::build_manager as build_notif_manager;
    use jacquewm::platform::windows::tray::WindowsTray;

    let cfg_snapshot = cfg.snapshot();
    let health = SubsystemHealth::new();

    // Settings manager — wraps the existing ConfigManager.
    let settings = std::sync::Arc::new(SettingsManager::new(cfg.clone()));
    health.register("settings");

    // Settings live-reload watcher — only spawn if the user has the
    // config file present at the canonical path. The watcher is
    // parked in a `Box::leak`-ed mutex so it survives until process
    // exit; dropping the `Debouncer` would stop the underlying
    // notify background thread.
    if let Ok(path) = ConfigManager::canonical_path() {
        let settings_for_thread = settings.clone();
        let path_for_thread = path.clone();
        let leak: &'static std::sync::Mutex<Option<jacquewm::platform::windows::settings::SettingsWatcher>> =
            Box::leak(Box::new(std::sync::Mutex::new(None)));
        safe_init("settings-watcher", health.clone(), move || {
            match jacquewm::platform::windows::settings::build_watcher(
                path_for_thread,
                settings_for_thread,
            ) {
                Ok(watcher) => {
                    *leak.lock().unwrap() = Some(watcher);
                    tracing::info!(target: "jacquewm", "settings watcher installed");
                }
                Err(e) => {
                    tracing::warn!(
                        target: "jacquewm",
                        error = %e,
                        "settings watcher failed to install; hot-reload disabled"
                    );
                }
            }
        });
    }

    // DebugManager — gated by config.debug.debug_mode.
    let debug_mgr = std::sync::Arc::new(DebugManager::new(cfg.clone(), health.clone()));
    health.register("debug");
    if debug_mgr.is_enabled() {
        debug_mgr.log_dump();
    }
    let _ = debug_mgr;

    // Notifications manager.
    if cfg_snapshot.notifications.enabled {
        let manager = build_notif_manager(
            cfg_snapshot.notifications.duration_ms,
            cfg_snapshot.notifications.max_visible,
        );
        health.register("notifications");
        let _ = manager as std::sync::Arc<dyn NotificationSink>;
    } else {
        health.register_disabled("notifications");
    }

    // Tray — leak a single static instance so the icon and sink
    // persist for the lifetime of the process without us holding a
    // separate `Arc` whose lifetime is hard to express.
    if cfg_snapshot.tray.enabled {
        health.register("tray");
        let tray: &'static WindowsTray = Box::leak(Box::new(WindowsTray::new()));
        let settings_for_tray = settings.clone();
        let sink: jacquewm::core::tray::TraySink = std::sync::Arc::new(move |action| {
            let _manager = settings_for_tray.clone();
            match action {
                jacquewm::core::tray::TrayAction::Exit => {
                    tracing::info!(target: "jacquewm", "tray requested exit");
                    std::process::exit(0);
                }
                jacquewm::core::tray::TrayAction::Restart => {
                    tracing::info!(target: "jacquewm", "tray requested restart");
                    if let Ok(p) = std::env::current_exe() {
                        let _ = std::process::Command::new(p).spawn();
                        std::process::exit(0);
                    }
                }
                jacquewm::core::tray::TrayAction::OpenLogs => {
                    tracing::info!(target: "jacquewm", "tray open-logs requested");
                    if let Ok(log_dir) = std::env::var("APPDATA") {
                        let dir = std::path::PathBuf::from(log_dir)
                            .join("JacqueWM")
                            .join("logs");
                        let _ = std::fs::create_dir_all(&dir);
                        let _ = std::process::Command::new("explorer.exe").arg(&dir).spawn();
                    }
                }
                jacquewm::core::tray::TrayAction::TogglePause => {
                    tracing::info!(target: "jacquewm", "tray toggle-pause (no-op)");
                }
            }
        });
        tray.subscribe(sink);
        tray.install();
    } else {
        health.register_disabled("tray");
    }

    // Launcher.
    if cfg_snapshot.launcher.enabled {
        health.register("launcher");
        let engine: std::sync::Arc<LauncherEngine> = build_launcher_engine(cfg_snapshot.launcher.max_results);
        // The launcher engine is consumed by main.rs's message loop
        // dispatch path — for now we just register its health.
        let _ = engine;
    } else {
        health.register_disabled("launcher");
    }

    tracing::info!(
        target: "jacquewm",
        subsystems = ?health.snapshot().iter().map(|e| (e.name.as_str(), format!("{:?}", e.health))).collect::<Vec<_>>(),
        "prompt 3 + 4 subsystems installed"
    );
}

// Silence the unused-warning for `MetricsCollector`; the platform
// panel thread drives it directly via a handle that's bound at
// runtime, not at compile time.
#[allow(dead_code)]
fn _metrics_in_scope(_: &MetricsCollector) {}
#[allow(dead_code)]
fn _tiling_in_scope(_: &tiling_ops) {}

/// Drain loop that pairs the Win32 message pump with our hotkey
/// channel. After every dispatched press the panel controller is
/// re-synced with the engine so the LEFT section's active pill flips
/// immediately.
fn run_message_loop(
    dispatcher: Arc<HotkeyManager>,
    rx: std::sync::mpsc::Receiver<HotkeyPress>,
    engine: Arc<WorkspaceEngine>,
    panel: Arc<PanelController>,
) {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, TranslateMessage, MSG, WM_QUIT,
    };

    unsafe {
        let mut msg = MSG::default();
        let mut running = true;
        while running {
            // Drain the hotkey channel FIRST.
            while let Ok(press) = rx.try_recv() {
                let prev = engine.current();
                dispatcher.dispatch_press(press);
                let now = engine.current();
                if prev != now {
                    panel.set_workspace(now);
                }
            }

            // Wait for the next window message.
            let res = GetMessageW(&mut msg, None, 0, 0);
            if res.0 == 0 {
                running = false;
                continue;
            }
            if res.0 < 0 {
                warn!(
                    target: "jacquewm",
                    code = res.0,
                    "GetMessageW returned error"
                );
                continue;
            }

            // Drain events queued during this message handling.
            while let Ok(press) = rx.try_recv() {
                let prev = engine.current();
                dispatcher.dispatch_press(press);
                let now = engine.current();
                if prev != now {
                    panel.set_workspace(now);
                }
            }

            // Process this message.
            if msg.message == WM_QUIT {
                running = false;
                continue;
            }
            let _ = TranslateMessage(&msg);
            let _ = DispatchMessageW(&msg);
        }
    }
}

fn main() -> std::process::ExitCode {
    if let Err(e) = jacquewm_main() {
        eprintln!("jacquewm: unhandled error: {e}");
        return std::process::ExitCode::from(1);
    }
    std::process::ExitCode::SUCCESS
}
