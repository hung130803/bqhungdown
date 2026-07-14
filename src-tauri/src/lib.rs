//! Application entry: register plugins, init stores, register commands.

pub mod models;
pub mod error;
pub mod settings_store;
pub mod extractors;
pub mod history_store;
pub mod short_id;
pub mod filename_resolver;
pub mod progress_parser;
pub mod args_builder;
pub mod sidecar_detect;
pub mod events;
pub mod notification;
pub mod url_validator;
pub mod url_resolver;
pub mod tiktok_photo;
pub mod douyin_scraper;
pub mod douyin_sign;
pub mod clipboard;
pub mod channel_fetcher;
pub mod youtube_api;
pub mod channel_cache;
pub mod ytdlp_runner;
pub mod ytdlp_update;
pub mod watchlist_store;
pub mod watcher;
pub mod po_token;
pub mod js_runtime;
pub mod bookmarks_store;
pub mod queue;
pub mod commands;

use std::sync::Arc;

use tauri::Manager;

use crate::commands::PendingConflicts;
use crate::history_store::HistoryStore;
use crate::queue::QueueManager;
use crate::settings_store::SettingsStore;
use crate::ytdlp_runner::YtDlpRunner;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_drag::init())
        .setup(|app| {
            let handle = app.handle().clone();

            // Resolve config + data paths via Tauri path API.
            let config_dir = handle
                .path()
                .app_config_dir()
                .expect("app_config_dir resolves on supported platforms");
            std::fs::create_dir_all(&config_dir).ok();
            let settings_path = config_dir.join("settings.json");

            let data_dir = handle
                .path()
                .app_data_dir()
                .expect("app_data_dir resolves on supported platforms");
            std::fs::create_dir_all(&data_dir).ok();
            let db_path = data_dir.join("state.db");

            // Initialize stores.
            let (settings_store, settings_warn) = SettingsStore::load(settings_path);
            if let Some(err) = settings_warn {
                eprintln!("[settings] {err}");
            }
            let settings = Arc::new(settings_store);

            let history = Arc::new(
                HistoryStore::open(db_path).expect("open history db"),
            );
            let runner = Arc::new(YtDlpRunner::new(handle.clone()));

            // Resolve sidecar binaries directory and probe for aria2c.
            // In dev: src-tauri/binaries; in bundled production: alongside the executable.
            let bundled_dir = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|x| x.to_path_buf()));
            let dev_bin_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries");
            crate::sidecar_detect::init_aria2c(Some(&dev_bin_dir));
            // Also probe production location if dev didn't find it.
            if !crate::sidecar_detect::aria2c_available() {
                if let Some(dir) = bundled_dir.as_deref() {
                    crate::sidecar_detect::init_aria2c(Some(dir));
                }
            }
            // Note: aria2c is opt-in via Settings. It's faster but yt-dlp does
            // not pass through aria2c's progress, so the UI progress bar stays
            // empty until the file is fully downloaded.

            let queue_path = data_dir.join("queue.json");
            let queue = QueueManager::new(
                handle.clone(),
                settings.clone(),
                history.clone(),
                runner.clone(),
                queue_path.clone(),
            );
            // Restore a previously saved queue so unfinished downloads resume
            // automatically after the app is closed/reopened (or crashes).
            if let Ok(text) = std::fs::read_to_string(&queue_path) {
                if let Ok(saved) = serde_json::from_str::<Vec<crate::models::DownloadItem>>(&text) {
                    queue.restore(saved);
                }
            }

            // Spawn clipboard watcher.
            let watcher = crate::clipboard::ClipboardWatcher::new(handle.clone(), settings.clone());
            watcher.start();

            // Keep yt-dlp fresh on every machine, decoupled from app releases.
            // YouTube breaks old yt-dlp builds every few weeks; this throttled
            // background self-update (max once / 12h) means a YouTube change is
            // picked up automatically without shipping a whole new app version.
            crate::ytdlp_update::spawn_update_check(handle.clone(), data_dir.clone());

            // Ensure Deno (JS runtime) is next to yt-dlp so it can solve
            // YouTube's signature / n-challenge — required since 2026 to get
            // real video URLs (otherwise "Requested format is not available").
            crate::js_runtime::ensure(bundled_dir.clone());

            // Auto-watch channels: load the watchlist and start the background
            // monitor that periodically enqueues new uploads.
            let bookmarks = Arc::new(crate::bookmarks_store::BookmarksStore::load(
                config_dir.join("bookmarks.json"),
            ));

            let watchlist = Arc::new(crate::watchlist_store::WatchlistStore::load(
                config_dir.join("watchlist.json"),
            ));
            crate::watcher::spawn_monitor(
                handle.clone(),
                watchlist.clone(),
                queue.clone(),
                settings.clone(),
                history.clone(),
            );

            // PO Token provider (bgutil) — opt-in anti-bot helper. Start it in
            // the background when enabled; killed on shutdown.
            let po_proc = Arc::new(crate::po_token::ProviderProcess::default());
            if settings.get().po_token_enabled {
                crate::po_token::enable(
                    handle.clone(),
                    data_dir.clone(),
                    bundled_dir.clone(),
                    po_proc.clone(),
                );
            }

            // Manage state.
            app.manage(settings);
            app.manage(history);
            app.manage(runner);
            app.manage(queue);
            app.manage(watchlist);
            app.manage(bookmarks);
            app.manage(po_proc);
            app.manage(PendingConflicts::default());

            // System tray: lets the app keep running in the background (so it
            // resumes rate-limited downloads) after the window is closed.
            {
                use tauri::menu::{Menu, MenuItem};
                use tauri::tray::TrayIconBuilder;
                let show_i = MenuItem::with_id(app, "show", "Mở BQHungDown", true, None::<&str>)?;
                let quit_i = MenuItem::with_id(app, "quit", "Thoát hẳn", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&show_i, &quit_i])?;
                let mut builder = TrayIconBuilder::with_id("main-tray");
                if let Some(icon) = app.default_window_icon() {
                    builder = builder.icon(icon.clone());
                }
                builder
                    .tooltip("BQHungDown — đang chạy ngầm")
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "show" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                        "quit" => {
                            if let Some(q) = app.try_state::<Arc<QueueManager>>() {
                                q.shutdown();
                            }
                            app.exit(0);
                        }
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let tauri::tray::TrayIconEvent::Click {
                            button: tauri::tray::MouseButton::Left,
                            button_state: tauri::tray::MouseButtonState::Up,
                            ..
                        } = event
                        {
                            let app = tray.app_handle();
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                    })
                    .build(app)?;
            }

            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Close button: hide to tray (keep running) when enabled.
            tauri::WindowEvent::CloseRequested { api, .. } => {
                let hide = window
                    .try_state::<Arc<SettingsStore>>()
                    .map(|s| s.get().minimize_to_tray)
                    .unwrap_or(true);
                if hide {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
            tauri::WindowEvent::Destroyed => {
                if let Some(queue) = window.try_state::<Arc<QueueManager>>() {
                    queue.shutdown();
                }
                if let Some(po) = window.try_state::<Arc<crate::po_token::ProviderProcess>>() {
                    crate::po_token::shutdown(&po);
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::validate_url,
            commands::fetch_metadata,
            commands::fetch_channel_videos,
            commands::cancel_channel_fetch,
            commands::fetch_thumbnail_data_url,
            douyin_scraper::scrape_douyin_channel,
            commands::enqueue_download,
            commands::enqueue_batch,
            commands::enqueue_playlist,
            commands::pause_download,
            commands::resume_download,
            commands::cancel_download,
            commands::retry_download,
            commands::retry_all_failed,
            commands::force_download,
            commands::test_proxy,
            commands::list_queue,
            commands::remove_queue_item,
            commands::remove_queue_group,
            commands::undo_remove_group,
            commands::clean_junk_files,
            commands::path_exists,
            commands::resolve_conflict,
            commands::get_settings,
            commands::update_settings,
            commands::validate_youtube_api_key,
            commands::pick_folder,
            commands::pick_file,
            commands::check_folder_writable,
            commands::open_in_folder,
            commands::open_file,
            commands::find_output_file,
            commands::update_history_output_path,
            commands::open_url,
            commands::list_history,
            commands::delete_history_entry,
            commands::delete_history_entries,
            commands::set_history_edited,
            commands::clear_history,
            commands::redownload_from_history,
            commands::list_extractors,
            commands::get_subtitle_langs,
            commands::set_clipboard_watcher,
            commands::deno_status,
            commands::retry_deno,
            commands::fix_download_engine,
            commands::list_bookmarks,
            commands::add_bookmark,
            commands::remove_bookmark,
            commands::update_bookmark_note,
            commands::list_watched_channels,
            commands::add_watched_channel,
            commands::remove_watched_channel,
            commands::set_watched_enabled,
            commands::set_watched_auto_download,
            commands::download_pending,
            commands::dismiss_pending,
            commands::check_watched_now,
            commands::app_bootstrap,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
