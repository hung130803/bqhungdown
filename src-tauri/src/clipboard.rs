//! Clipboard_Watcher: poll system clipboard every 1000ms while
//! `Settings.clipboard_watcher == true`. Emits `clipboard://detected` for new
//! URLs that match a known extractor. Per-URL dedupe window of 60s.
//!
//! Reads clipboard via `arboard` crate (pure-Rust, no Tauri plugin needed —
//! avoids forcing the user to install yet another plugin). If `arboard` fails
//! (headless/CI), the watcher silently no-ops.
//!
//! Note: this module spawns a tokio task via `start()`. The caller is expected
//! to spawn it once at app startup and forget the JoinHandle.

use crate::events::{ClipboardEventPayload, EV_CLIPBOARD_DETECTED};
use crate::settings_store::SettingsStore;
use crate::url_validator;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

const POLL_INTERVAL: Duration = Duration::from_millis(1000);
const DEDUPE_WINDOW: Duration = Duration::from_secs(60);

pub struct ClipboardWatcher {
    app: AppHandle,
    settings: Arc<SettingsStore>,
    last_seen: Arc<Mutex<HashMap<String, Instant>>>,
}

impl ClipboardWatcher {
    pub fn new(app: AppHandle, settings: Arc<SettingsStore>) -> Self {
        Self {
            app,
            settings,
            last_seen: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Spawn a Tauri-managed async task that polls clipboard until the AppHandle drops.
    pub fn start(self) {
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(POLL_INTERVAL);
            loop {
                interval.tick().await;
                if !self.settings.get().clipboard_watcher {
                    continue;
                }
                self.tick();
            }
        });
    }

    fn tick(&self) {
        let text = match read_clipboard_text() {
            Some(t) => t,
            None => return,
        };
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }

        // Take only the first line as URL candidate.
        let candidate = trimmed.lines().next().unwrap_or(trimmed).trim();

        let validation = url_validator::validate_url(candidate);
        if !validation.valid {
            return;
        }
        let extractor = match validation.extractor {
            Some(e) => e,
            None => return, // unknown site → don't notify
        };

        let now = Instant::now();
        {
            let mut map = self.last_seen.lock();
            if let Some(t) = map.get(candidate) {
                if now.duration_since(*t) < DEDUPE_WINDOW {
                    return;
                }
            }
            map.insert(candidate.to_string(), now);
            // Garbage-collect entries older than DEDUPE_WINDOW * 4.
            map.retain(|_, t| now.duration_since(*t) < DEDUPE_WINDOW * 4);
        }

        let payload = ClipboardEventPayload {
            url: candidate.to_string(),
            extractor,
        };
        let _ = self.app.emit(EV_CLIPBOARD_DETECTED, payload);
    }
}

#[cfg(target_os = "windows")]
fn read_clipboard_text() -> Option<String> {
    read_via_arboard()
}
#[cfg(target_os = "macos")]
fn read_clipboard_text() -> Option<String> {
    read_via_arboard()
}
#[cfg(target_os = "linux")]
fn read_clipboard_text() -> Option<String> {
    read_via_arboard()
}

fn read_via_arboard() -> Option<String> {
    // Lazy import — arboard is optional. If not added to Cargo.toml, fallback
    // to None and the watcher becomes a no-op. We can avoid pulling the dep by
    // using the Tauri shell plugin's clipboard, but arboard is simpler.
    #[cfg(feature = "with_arboard")]
    {
        if let Ok(mut cb) = arboard::Clipboard::new() {
            if let Ok(text) = cb.get_text() {
                return Some(text);
            }
        }
        None
    }
    #[cfg(not(feature = "with_arboard"))]
    {
        // Fallback: stub — clipboard auto-detect requires the `with_arboard`
        // feature flag to be enabled (which adds the `arboard` crate). Until
        // then this function returns None and the watcher is a quiet no-op.
        None
    }
}
