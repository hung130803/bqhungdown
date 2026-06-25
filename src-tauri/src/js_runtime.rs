//! Ensure a JavaScript runtime (Deno) sits next to the bundled yt-dlp.
//!
//! Since 2026, YouTube requires solving a JavaScript challenge (signature +
//! "n" parameter) to obtain real video URLs. yt-dlp needs a JS runtime for
//! this; without one it only sees storyboard images and fails with "Requested
//! format is not available". yt-dlp auto-detects a `deno` binary in its OWN
//! directory and runs the EJS solver scripts it already bundles (no remote
//! code fetch). So we just drop `deno.exe` next to the yt-dlp sidecar.
//!
//! Verified working: with deno.exe beside yt-dlp, a video that previously gave
//! "Only images are available" resolved a real format.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};

/// Latest stable Deno for Windows x64. yt-dlp needs Deno >= 2.3.0.
const DENO_URL: &str =
    "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip";

/// Deno setup state for the UI: 0 unknown, 1 downloading, 2 ready, 3 failed.
static STATUS: AtomicU8 = AtomicU8::new(0);

/// Current Deno status as a string for the frontend.
pub fn status() -> &'static str {
    match STATUS.load(Ordering::Relaxed) {
        1 => "downloading",
        2 => "ready",
        3 => "failed",
        _ => "unknown",
    }
}

/// Download + unpack Deno next to yt-dlp if missing. Background, best-effort:
/// on failure some YouTube videos won't resolve (logged), the rest still work.
pub fn ensure(yt_dlp_dir: Option<PathBuf>) {
    let Some(dir) = yt_dlp_dir else {
        STATUS.store(3, Ordering::Relaxed);
        return;
    };
    tauri::async_runtime::spawn(async move {
        if dir.join("deno.exe").exists() {
            STATUS.store(2, Ordering::Relaxed);
            return;
        }
        STATUS.store(1, Ordering::Relaxed); // downloading
        match download_deno(&dir).await {
            Ok(()) => {
                STATUS.store(2, Ordering::Relaxed);
                eprintln!("[js-runtime] Deno ready next to yt-dlp");
            }
            Err(e) => {
                STATUS.store(3, Ordering::Relaxed);
                eprintln!("[js-runtime] Deno setup failed: {e}");
            }
        }
    });
}

async fn download_deno(dir: &Path) -> Result<(), String> {
    let bytes = reqwest::Client::builder()
        .build()
        .map_err(|e| e.to_string())?
        .get(DENO_URL)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;

    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if entry.name().ends_with("deno.exe") {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            // Write to a temp file then rename so a half-extract never looks done.
            let tmp = dir.join("deno.exe.part");
            {
                let mut out = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
                std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
            }
            std::fs::rename(&tmp, dir.join("deno.exe")).map_err(|e| e.to_string())?;
            return Ok(());
        }
    }
    Err("deno.exe not found inside the downloaded zip".into())
}
