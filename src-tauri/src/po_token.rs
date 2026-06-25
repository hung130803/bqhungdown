//! Optional PO Token provider integration (bgutil) to reduce YouTube's
//! "Sign in to confirm you're not a bot" wall WITHOUT cookies.
//!
//! When enabled (`Settings.po_token_enabled`):
//!   1. The bundled yt-dlp gets the bgutil plugin written next to it
//!      (`<yt-dlp dir>/yt-dlp-plugins/bgutil/yt_dlp_plugins/extractor/*.py`),
//!      which yt-dlp auto-loads. The two plugin files are embedded at compile
//!      time (verified working with yt-dlp 2026.06 + provider v0.8.1).
//!   2. The bgutil provider binary (a standalone Rust server, ~46 MB) is
//!      downloaded once into the app data dir and run as `server --host
//!      127.0.0.1 --port 4416` in the background.
//! yt-dlp then fetches PO tokens from the local server automatically.
//!
//! Everything is best-effort: if the download/plugin/server fails, yt-dlp
//! simply logs that no PO token provider is available and downloads continue
//! as before. NOTE: a PO token does NOT change your IP — at very high volume
//! you still need proxies. This is a complement, not a replacement.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tauri::AppHandle;

/// Provider binary (Windows x86_64) pinned to the version matching the embedded
/// plugin. Bump both together.
const PROVIDER_URL: &str =
    "https://github.com/jim60105/bgutil-ytdlp-pot-provider-rs/releases/download/v0.8.1/bgutil-pot-windows-x86_64.exe";

const PLUGIN_BASE_PY: &str = include_str!("../resources/potoken/getpot_bgutil.py");
const PLUGIN_HTTP_PY: &str = include_str!("../resources/potoken/getpot_bgutil_http.py");

/// Handle to the running provider process so we can kill it on app shutdown.
#[derive(Default)]
pub struct ProviderProcess(pub Mutex<Option<std::process::Child>>);

/// `<yt_dlp_dir>/yt-dlp-plugins/bgutil/yt_dlp_plugins/extractor`
fn plugin_dir(yt_dlp_dir: &Path) -> PathBuf {
    yt_dlp_dir
        .join("yt-dlp-plugins")
        .join("bgutil")
        .join("yt_dlp_plugins")
        .join("extractor")
}

/// Write the embedded plugin files next to the yt-dlp binary so it auto-loads
/// the bgutil PO-token provider. Best-effort.
pub fn install_plugin(yt_dlp_dir: &Path) -> std::io::Result<()> {
    let dir = plugin_dir(yt_dlp_dir);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("getpot_bgutil.py"), PLUGIN_BASE_PY)?;
    std::fs::write(dir.join("getpot_bgutil_http.py"), PLUGIN_HTTP_PY)?;
    Ok(())
}

/// Remove the plugin (called when the user turns PO token off).
pub fn uninstall_plugin(yt_dlp_dir: &Path) {
    let _ = std::fs::remove_dir_all(yt_dlp_dir.join("yt-dlp-plugins").join("bgutil"));
}

fn provider_path(data_dir: &Path) -> PathBuf {
    data_dir.join("po").join("bgutil-pot.exe")
}

/// Download the provider binary if missing. Returns the path on success.
async fn ensure_provider(data_dir: &Path) -> Result<PathBuf, String> {
    let exe = provider_path(data_dir);
    if exe.exists() {
        return Ok(exe);
    }
    if let Some(parent) = exe.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let bytes = reqwest::Client::builder()
        .build()
        .map_err(|e| e.to_string())?
        .get(PROVIDER_URL)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;
    // Write to a temp file then rename so a half-download never looks complete.
    let tmp = exe.with_extension("part");
    std::fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &exe).map_err(|e| e.to_string())?;
    Ok(exe)
}

/// Spawn the provider server bound to IPv4 loopback (so the plugin's default
/// `http://127.0.0.1:4416` connects — the server otherwise binds IPv6-only on
/// Windows). Stores the child so it can be killed on shutdown.
fn spawn_provider(exe: &Path, slot: &Mutex<Option<std::process::Child>>) {
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["server", "--host", "127.0.0.1", "--port", "4416"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    match cmd.spawn() {
        Ok(child) => {
            if let Ok(mut g) = slot.lock() {
                // Replace any previous handle (kill the old one first).
                if let Some(mut old) = g.take() {
                    let _ = old.kill();
                }
                *g = Some(child);
            }
            eprintln!("[po-token] provider started on 127.0.0.1:4416");
        }
        Err(e) => eprintln!("[po-token] failed to start provider: {e}"),
    }
}

/// Install the plugin and start the provider. Background, best-effort.
pub fn enable(
    _app: AppHandle,
    data_dir: PathBuf,
    yt_dlp_dir: Option<PathBuf>,
    slot: std::sync::Arc<ProviderProcess>,
) {
    tauri::async_runtime::spawn(async move {
        if let Some(dir) = yt_dlp_dir.as_deref() {
            if let Err(e) = install_plugin(dir) {
                eprintln!("[po-token] plugin install failed: {e}");
            }
        }
        match ensure_provider(&data_dir).await {
            Ok(exe) => spawn_provider(&exe, &slot.0),
            Err(e) => eprintln!("[po-token] provider download failed: {e}"),
        }
    });
}

/// Kill the provider process if running (called on shutdown).
pub fn shutdown(slot: &ProviderProcess) {
    if let Ok(mut g) = slot.0.lock() {
        if let Some(mut child) = g.take() {
            let _ = child.kill();
        }
    }
}
