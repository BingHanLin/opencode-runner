//! Tauri 2 entry crate. `main.rs` calls `run()`; everything else lives here
//! so cargo-tauri's mobile/desktop wrappers can both invoke the same setup.

pub mod commands;
pub mod config;
pub mod db;
pub mod mcp_memory;
pub mod opencode;
pub mod proc;
pub mod runner;
pub mod scheduler;
pub mod state;

use crate::commands::make_notifier;
use crate::db::Db;
use crate::opencode::Cli;
use crate::runner::new_cancel_registry;
use crate::scheduler::Scheduler;
use crate::state::AppState;
use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WindowEvent,
};
use tauri_plugin_single_instance;
use tokio::sync::Mutex;

const TRAY_SHOW_ID: &str = "show";
const TRAY_QUIT_ID: &str = "quit";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // Re-focus the main window on second launch instead of opening
            // another instance.
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .invoke_handler(tauri::generate_handler![
            commands::get_tasks_file,
            commands::save_tasks_file,
            commands::list_runs,
            commands::list_runs_for_task,
            commands::list_events,
            commands::list_logs,
            commands::clear_runs_for_task,
            commands::get_task_memory,
            commands::set_task_memory,
            commands::list_comments_for_run,
            commands::add_comment,
            commands::delete_comment,
            commands::load_conversation,
            commands::opencode_binary_status,
            commands::list_models,
            commands::run_now,
            commands::abort_run,
            commands::restart_scheduler,
            commands::is_git_repo_path,
            commands::show_main_window,
            commands::storage_paths,
        ])
        .setup(|app| {
            // Per-user app data dir resolved from the bundle identifier in
            // tauri.conf.json. Windows: %APPDATA%\dev.opencode.orchestrator\.
            // macOS:   ~/Library/Application Support/dev.opencode.orchestrator/.
            // Linux:   ~/.local/share/dev.opencode.orchestrator/.
            // Same path in dev and packaged runs, so config/history survive
            // a `cargo clean` and don't depend on where the binary was
            // launched from.
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("resolving app data dir");
            std::fs::create_dir_all(&data_dir).expect("creating app data dir");
            tracing::info!(data_dir = %data_dir.display(), "app data dir resolved");
            let config_path = data_dir.join("tasks.toml");
            let db_path = data_dir.join("runs.db");
            let db = Db::open(&db_path).expect("opening run history db");
            // A just-launched process owns no in-flight runs, so any row still
            // marked `running` is a leftover from a prior process that was
            // force-killed or crashed without going through the tray-Quit
            // graceful shutdown. Reconcile them to `aborted` now so the UI
            // doesn't show ghost runs stuck "running" forever.
            match db.reconcile_orphaned_runs() {
                Ok(0) => {}
                Ok(n) => tracing::info!("reconciled {n} orphaned run(s) left 'running' by a prior process"),
                Err(e) => tracing::warn!("reconciling orphaned runs failed: {e:#}"),
            }
            let registry = new_cancel_registry();

            // Boot scheduler with tasks already on disk.
            let (cli, max_history) = {
                let file = config::load(&config_path).unwrap_or_default();
                (
                    Cli::resolve(file.settings.opencode_binary.as_deref()).0,
                    file.settings.max_run_history,
                )
            };
            let handle = app.handle().clone();
            let notifier = Some(make_notifier(handle.clone()));
            let scheduler = tauri::async_runtime::block_on(async {
                let s = Scheduler::new(cli, db.clone(), registry.clone(), notifier, max_history)
                    .await
                    .expect("Scheduler::new");
                if let Ok(file) = config::load(&config_path) {
                    for t in file.tasks {
                        if let Err(e) = s.register(t).await {
                            tracing::warn!("registering task at boot failed: {e}");
                        }
                    }
                }
                s
            });

            app.manage(AppState {
                db,
                registry,
                scheduler: Mutex::new(Some(scheduler)),
                config_path,
                db_path,
            });

            // System tray — minimal Show / Quit menu, left-click also shows.
            let menu = Menu::with_items(
                app,
                &[
                    &MenuItem::with_id(app, TRAY_SHOW_ID, "Show window", true, None::<&str>)?,
                    &MenuItem::with_id(app, TRAY_QUIT_ID, "Quit", true, None::<&str>)?,
                ],
            )?;
            let icon = tray_icon_image();
            let _tray = TrayIconBuilder::with_id("main-tray")
                .icon(icon)
                .menu(&menu)
                .show_menu_on_left_click(false)
                .tooltip("opencode orchestrator")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    TRAY_SHOW_ID => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.unminimize();
                            let _ = w.set_focus();
                        }
                    }
                    TRAY_QUIT_ID => {
                        // Don't call `app.exit(0)` directly — fire off the
                        // graceful path so in-flight runs get a chance to
                        // cancel and clean up their worktrees before we go.
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            graceful_shutdown(app).await;
                        });
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.unminimize();
                            let _ = w.set_focus();
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .on_window_event(|window, event| {
            // Hide-to-tray on close, matching the Iced behaviour.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        });

    builder
        .run(tauri::generate_context!())
        .expect("error running tauri application");
}

/// Tear down in-flight work cleanly before exiting. Triggered from the tray
/// Quit menu — for force-kills (Task Manager, OS signal we can't trap) the
/// process dies immediately and runs end up stuck in "running" in db.
///
/// 1. Cancel every token in the registry with a distinct reason.
/// 2. Poll the registry until it drains, capped so a hung opencode child
///    can't block the app from ever quitting.
/// 3. Shut the scheduler down so it stops accepting new cron firings.
/// 4. `app.exit(0)`.
async fn graceful_shutdown(app: tauri::AppHandle) {
    use crate::runner::CancelToken;
    let state: tauri::State<AppState> = app.state();

    let tokens: Vec<CancelToken> = {
        let reg = state.registry.lock().unwrap();
        reg.tokens()
    };
    if !tokens.is_empty() {
        tracing::info!("graceful shutdown: cancelling {} in-flight run(s)", tokens.len());
        for t in &tokens {
            t.cancel_with_reason("app shutting down");
        }
    }

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let empty = state.registry.lock().unwrap().is_empty();
        if empty {
            if !tokens.is_empty() {
                tracing::info!("graceful shutdown: all runs cleaned up");
            }
            break;
        }
        if std::time::Instant::now() > deadline {
            tracing::warn!(
                "graceful shutdown: 30s deadline exceeded, exiting with {} run(s) still active",
                state.registry.lock().unwrap().len()
            );
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    // Stop the scheduler so any pending cron firings don't kick off a new
    // run while we're closing down.
    let scheduler = state.scheduler.lock().await.take();
    if let Some(s) = scheduler {
        s.shutdown().await;
    }

    app.exit(0);
}

/// Generate a 32x32 solid-accent disc as the tray icon so we don't have to
/// ship a PNG asset for the dev build. Replace with a branded `.ico`/`.png`
/// in `tauri.conf.json#app.trayIcon.iconPath` for production.
fn tray_icon_image() -> Image<'static> {
    let size: u32 = 32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let center = size as f32 / 2.0;
    let radius = (size as f32 / 2.0) - 0.5;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - center;
            let dy = y as f32 + 0.5 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            let i = ((y * size + x) * 4) as usize;
            if dist <= radius {
                rgba[i] = 0x8B; // accent R
                rgba[i + 1] = 0x7C; // accent G
                rgba[i + 2] = 0xFF; // accent B
                rgba[i + 3] = 0xFF;
            }
        }
    }
    Image::new_owned(rgba, size, size)
}
