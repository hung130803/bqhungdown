//! Embedded yt-dlp postprocessor; explicitly enabled only for download jobs.
use std::path::{Path, PathBuf};

pub fn install(data_dir: &Path) -> std::io::Result<PathBuf> {
    static INSTALL_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = INSTALL_LOCK.lock().map_err(|_| std::io::Error::new(
        std::io::ErrorKind::Other, "Audio plugin install lock poisoned"))?;
    // Versioned directory avoids replacing code while a previous job imports it.
    let root = data_dir.join("audio-plugin-v1");
    let dir = root.join("bqd-audio").join("yt_dlp_plugins").join("postprocessor");
    let file = dir.join("bqd_audio.py");
    const SOURCE: &str = include_str!("../resources/audio/yt_dlp_plugins/postprocessor/bqd_audio.py");
    if std::fs::read_to_string(&file).ok().as_deref() != Some(SOURCE) {
        std::fs::create_dir_all(&dir)?;
        std::fs::write(&file, SOURCE)?;
    }
    Ok(root)
}
