//! Self-update the bundled yt-dlp at runtime.
//!
//! Why: YouTube changes its anti-bot / extractor behaviour every few weeks, and
//! a stale yt-dlp stops being able to list channels or download ("Sign in to
//! confirm you're not a bot"). Previously yt-dlp only refreshed when a brand new
//! app release was built and every user updated the whole app — so a single
//! YouTube change broke every machine at once until the next release.
//!
//! yt-dlp ships a built-in updater (`-U` / `--update-to`) that replaces its own
//! executable in place. Tauri's default per-user install dir (`%LOCALAPPDATA%`)
//! is writable without admin, so this works for the common install. If the
//! binary happens to live somewhere read-only (per-machine install) the update
//! just fails silently and the bundled binary keeps working.
//!
//! Throttled to at most once per `MIN_INTERVAL_SECS` via a timestamp file in the
//! app data dir, and always run in the background so it never blocks the UI.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::AppHandle;
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;

/// Don't check more than once every 12 hours.
const MIN_INTERVAL_SECS: u64 = 12 * 60 * 60;
/// Forced check (triggered by a 403/bot failure) — still throttled, but much
/// tighter, so one broken batch triggers at most one update per hour.
const FORCED_MIN_INTERVAL_SECS: u64 = 60 * 60;
/// Give the self-update its own ceiling so a hung network call can't linger.
const UPDATE_TIMEOUT: Duration = Duration::from_secs(120);

fn stamp_path(data_dir: &Path) -> PathBuf {
    data_dir.join("ytdlp_update_check")
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// True when we haven't checked within `min_interval` seconds.
fn should_check(stamp: &Path, min_interval: u64) -> bool {
    let last = std::fs::read_to_string(stamp)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0);
    now_secs().saturating_sub(last) >= min_interval
}

/// Kick off a throttled background yt-dlp self-update. Never blocks the caller;
/// any failure is logged and ignored (the existing binary keeps working).
pub fn spawn_update_check(app: AppHandle, data_dir: PathBuf) {
    spawn_update_check_inner(app, data_dir, MIN_INTERVAL_SECS)
}

/// Update ngay khi phát hiện YouTube chặn (403 / bot wall): đa số các đợt vỡ
/// hàng loạt được yt-dlp vá trong vài giờ-vài ngày, nên item đang chờ cooldown
/// sẽ retry bằng binary MỚI thay vì fail lại y hệt. Throttle 1h/lần.
pub fn spawn_forced_update(app: AppHandle, data_dir: PathBuf) {
    spawn_update_check_inner(app, data_dir, FORCED_MIN_INTERVAL_SECS)
}

fn spawn_update_check_inner(app: AppHandle, data_dir: PathBuf, min_interval: u64) {
    tauri::async_runtime::spawn(async move {
        let stamp = stamp_path(&data_dir);
        if !should_check(&stamp, min_interval) {
            return;
        }
        // Stamp up-front so a crash mid-update doesn't make us retry every launch.
        let _ = std::fs::create_dir_all(&data_dir);
        let _ = std::fs::write(&stamp, now_secs().to_string());

        match tokio::time::timeout(UPDATE_TIMEOUT, run_update(&app)).await {
            Ok(Ok(msg)) => eprintln!("[ytdlp-update] {msg}"),
            Ok(Err(e)) => eprintln!("[ytdlp-update] failed: {e}"),
            Err(_) => eprintln!("[ytdlp-update] timed out after {}s", UPDATE_TIMEOUT.as_secs()),
        }
    });
}

/// Run `yt-dlp --update-to nightly` and return the last meaningful output line.
async fn run_update(app: &AppHandle) -> Result<String, String> {
    let cmd = app
        .shell()
        .sidecar("yt-dlp")
        .map_err(|e| e.to_string())?
        // Kênh `nightly` (khuyến nghị chính thức của yt-dlp cho người dùng
        // thường): các fix YouTube 403/anti-bot lên nightly sớm hơn stable
        // nhiều ngày → thời gian "tool chết" sau mỗi đợt YouTube đổi player
        // ngắn đi đáng kể. Nếu đã mới nhất, yt-dlp báo "up to date", exit 0.
        .args(["--update-to", "nightly", "--no-warnings"]);

    let (mut rx, _child) = cmd.spawn().map_err(|e| e.to_string())?;
    let mut out = String::new();
    while let Some(ev) = rx.recv().await {
        match ev {
            CommandEvent::Stdout(b) | CommandEvent::Stderr(b) => {
                out.push_str(&String::from_utf8_lossy(&b))
            }
            CommandEvent::Terminated(_) => break,
            _ => {}
        }
    }
    Ok(out
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("(no output)")
        .trim()
        .to_string())
}
