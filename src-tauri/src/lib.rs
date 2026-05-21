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
pub mod clipboard;
pub mod channel_fetcher;
pub mod ytdlp_runner;
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

            let queue = QueueManager::new(handle.clone(), settings.clone(), history.clone(), runner.clone());

            // Spawn clipboard watcher.
            let watcher = crate::clipboard::ClipboardWatcher::new(handle.clone(), settings.clone());
            watcher.start();

            // NOTE: yt-dlp auto-update on startup intentionally disabled.
            // Updates ship with each app release (GitHub Actions re-fetches the
            // latest yt-dlp at build time and bundles it into the installer).
            // Users get yt-dlp updates by accepting the in-app "Cập nhật" banner.

            // Manage state.
            app.manage(settings);
            app.manage(history);
            app.manage(runner);
            app.manage(queue);
            app.manage(PendingConflicts::default());

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                if let Some(queue) = window.try_state::<Arc<QueueManager>>() {
                    queue.shutdown();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::validate_url,
            commands::fetch_metadata,
            commands::fetch_channel_videos,
            commands::cancel_channel_fetch,
            commands::enqueue_download,
            commands::enqueue_batch,
            commands::enqueue_playlist,
            commands::pause_download,
            commands::resume_download,
            commands::cancel_download,
            commands::retry_download,
            commands::list_queue,
            commands::remove_queue_item,
            commands::path_exists,
            commands::resolve_conflict,
            commands::get_settings,
            commands::update_settings,
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
            commands::app_bootstrap,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
