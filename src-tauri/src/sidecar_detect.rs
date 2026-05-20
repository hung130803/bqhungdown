//! Detect availability of optional binaries (aria2c, ffmpeg).
//!
//! - `aria2c_available()` — true if either bundled sidecar (`src-tauri/binaries/aria2c-<triple>[.exe]`
//!   resolved via Tauri resource resolver) OR system PATH has `aria2c`.
//! - `aria2c_path()` — returns the full path to use; prefers bundled, falls back to plain `aria2c`.
//! - `ffmpeg_available_in_path()` — system PATH probe (Tauri sidecar ffmpeg is invoked separately).
//!
//! Results are cached for the lifetime of the process.

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

static ARIA2C: OnceLock<Option<PathBuf>> = OnceLock::new();
static FFMPEG: OnceLock<bool> = OnceLock::new();

/// Initialise aria2c lookup with the bundled sidecar path if available.
/// Should be called once at app startup with the Tauri-resolved binary directory.
pub fn init_aria2c(bundled_dir: Option<&std::path::Path>) {
    let resolved = resolve_aria2c(bundled_dir);
    let _ = ARIA2C.set(resolved);
}

fn resolve_aria2c(bundled_dir: Option<&std::path::Path>) -> Option<PathBuf> {
    // 1) Look in the Tauri bundle directory next to yt-dlp/ffmpeg.
    if let Some(dir) = bundled_dir {
        let triple = current_triple();
        let exe_suffix = if cfg!(windows) { ".exe" } else { "" };
        let candidate = dir.join(format!("aria2c-{triple}{exe_suffix}"));
        if candidate.is_file() {
            return Some(candidate);
        }
        // Also accept the bare name that Tauri uses post-bundling.
        let bare = dir.join(format!("aria2c{exe_suffix}"));
        if bare.is_file() {
            return Some(bare);
        }
    }
    // 2) Fall back to system PATH lookup.
    if which_exists("aria2c") {
        return Some(PathBuf::from("aria2c"));
    }
    None
}

pub fn aria2c_available() -> bool {
    ARIA2C.get_or_init(|| resolve_aria2c(None)).is_some()
}

pub fn aria2c_path() -> Option<PathBuf> {
    ARIA2C.get_or_init(|| resolve_aria2c(None)).clone()
}

pub fn ffmpeg_available_in_path() -> bool {
    *FFMPEG.get_or_init(|| which_exists("ffmpeg"))
}

fn which_exists(bin: &str) -> bool {
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = Command::new("where");
        c.arg(bin);
        c
    };
    #[cfg(not(target_os = "windows"))]
    let mut cmd = {
        let mut c = Command::new("which");
        c.arg(bin);
        c
    };
    match cmd.output() {
        Ok(out) => out.status.success() && !out.stdout.is_empty(),
        Err(_) => false,
    }
}

fn current_triple() -> &'static str {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    { "x86_64-pc-windows-msvc" }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    { "x86_64-apple-darwin" }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    { "aarch64-apple-darwin" }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    { "x86_64-unknown-linux-gnu" }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    { "aarch64-unknown-linux-gnu" }
}
