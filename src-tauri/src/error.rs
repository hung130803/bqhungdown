use serde::{Deserialize, Serialize};
use thiserror::Error;
use std::path::PathBuf;

#[derive(Debug, Clone, Error, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "PascalCase")]
pub enum AppError {
    #[error("URL không hợp lệ")]
    InvalidUrl,

    #[error("Không hỗ trợ site này")]
    UnsupportedSite,

    #[error("yt-dlp thất bại: {0}")]
    YtDlpFailed(String),

    #[error("Không tìm thấy ffmpeg")]
    FfmpegMissing,

    #[error("Thư mục lưu không khả dụng: {0}")]
    SaveFolderUnavailable(PathBuf),

    #[error("Hết thời gian chờ")]
    Timeout,

    #[error("Trạng thái không hợp lệ: {from:?} với event {event}")]
    IllegalTransition { from: String, event: String },

    #[error("Lỗi I/O: {0}")]
    Io(String),

    #[error("Cấu hình hỏng")]
    ConfigCorrupt,

    #[error("Giá trị cấu hình không hợp lệ: {field}")]
    InvalidSetting { field: String },

    #[error("Không tìm thấy mục: {0}")]
    NotFound(String),

    #[error("Đã bị huỷ")]
    Cancelled,

    #[error("Lỗi: {0}")]
    Other(String),
}

pub type AppResult<T> = Result<T, AppError>;

/// True when a yt-dlp error means it couldn't read/decrypt browser cookies
/// (modern Chrome/Edge on Windows use AppBound/DPAPI encryption that yt-dlp
/// can't decrypt). When this happens we retry the call WITHOUT cookies, since
/// public videos don't need them. See https://github.com/yt-dlp/yt-dlp/issues/10927
/// True when yt-dlp hit YouTube's anti-bot / rate-limit wall ("Sign in to
/// confirm you're not a bot" or HTTP 429). We respond by rotating to the next
/// proxy (if configured) and backing off longer before retrying.
pub fn is_bot_error(msg: &str) -> bool {
    let l = msg.to_lowercase();
    l.contains("sign in to confirm")
        || l.contains("not a bot")
        || l.contains("confirm you")
        || l.contains("http error 429")
        || l.contains("too many requests")
}

/// True when yt-dlp extracted the video but couldn't produce a downloadable
/// format ("Requested format is not available" / no formats). On YouTube this
/// is usually the SABR rollout hiding direct URLs on the default client — we
/// retry pulling formats from additional clients (tv/mweb) that still serve them.
pub fn is_format_error(msg: &str) -> bool {
    let l = msg.to_lowercase();
    l.contains("requested format is not available")
        || l.contains("no video formats")
        || l.contains("no formats found")
        || l.contains("only images are available")
}

pub fn is_cookie_decrypt_error(msg: &str) -> bool {
    let l = msg.to_lowercase();
    l.contains("dpapi")
        || l.contains("failed to decrypt")
        || l.contains("unable to decrypt")
        || l.contains("could not copy")
        || (l.contains("cookie") && l.contains("decrypt"))
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self { AppError::Io(err.to_string()) }
}
impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self { AppError::Other(format!("JSON: {err}")) }
}
impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self { AppError::Other(format!("SQLite: {err}")) }
}
impl From<url::ParseError> for AppError {
    fn from(_: url::ParseError) -> Self { AppError::InvalidUrl }
}
impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self { AppError::Other(err.to_string()) }
}
impl From<tauri::Error> for AppError {
    fn from(err: tauri::Error) -> Self { AppError::Other(format!("Tauri: {err}")) }
}
