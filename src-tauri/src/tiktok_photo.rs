//! TikTok photo (slideshow) download helper.
//!
//! Background:
//! TikTok photo posts are served by the same `tiktok.com/@user/video/<id>`
//! URL scheme as videos, but the post type is "slideshow" — a sequence of
//! still images plus a music track. yt-dlp only extracts the audio MP3 for
//! these posts, so we bypass it entirely and use TikWM's `/api/` scraping
//! endpoint (the same one we already use for Douyin) to retrieve the image
//! URL list and download them ourselves.
//!
//! Layout: each photo post becomes a *folder* named after the sanitized
//! title; inside we save `01.jpg`, `02.jpg`, … plus an optional `audio.mp3`.

use crate::error::{AppError, AppResult};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                  (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";
const TIKWM_ENDPOINTS: &[&str] = &[
    "https://www.tikwm.com/api/",
    "https://tikwm.com/api/",
    "https://api.tikwm.com/api/",
];

/// Fetched metadata for a TikTok photo post.
#[derive(Debug, Clone)]
pub struct PhotoPost {
    pub title: String,
    pub images: Vec<String>,
    pub audio_url: Option<String>,
}

/// Heuristic: tikwm `data.images` array tells us this is a photo post.
/// Returns `None` for any URL that isn't a photo slideshow (let yt-dlp
/// handle videos as usual).
pub async fn fetch_photo_meta(url: &str) -> Option<PhotoPost> {
    for endpoint in TIKWM_ENDPOINTS {
        if let Some(post) = try_endpoint(endpoint, url).await {
            return Some(post);
        }
    }
    None
}

async fn try_endpoint(endpoint: &str, url: &str) -> Option<PhotoPost> {
    let client = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent(UA)
        .build()
        .ok()?;
    let resp = client
        .get(endpoint)
        .query(&[("url", url), ("hd", "1")])
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let json: Value = resp.json().await.ok()?;
    if json.get("code").and_then(|v| v.as_i64()) != Some(0) {
        return None;
    }
    let data = json.get("data")?;
    let images: Vec<String> = data
        .get("images")
        .and_then(|v| v.as_array())?
        .iter()
        .filter_map(|i| i.as_str().map(String::from))
        .collect();
    if images.is_empty() {
        return None;
    }
    let title = data
        .get("title")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| "tiktok-photo".to_string());
    let audio_url = data
        .get("music")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| data.get("music_info").and_then(|m| m.get("play")).and_then(|v| v.as_str()).map(String::from));
    Some(PhotoPost {
        title,
        images,
        audio_url,
    })
}

/// Download every image (and audio when available) into a folder named
/// after the sanitized title, under `save_folder`. Returns the path to the
/// folder so the queue can show / open it later.
pub async fn download_photo_post(
    post: &PhotoPost,
    save_folder: &Path,
) -> AppResult<PathBuf> {
    let stem = crate::filename_resolver::sanitize(&post.title);
    let mut folder = save_folder.join(&stem);
    // Auto-rename `(N)` if a folder with the same name already exists, mirroring
    // the per-file collision policy.
    let mut suffix = 1u32;
    while folder.exists() {
        folder = save_folder.join(format!("{stem} ({suffix})"));
        suffix += 1;
    }
    std::fs::create_dir_all(&folder).map_err(AppError::from)?;

    let client = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent(UA)
        .build()
        .map_err(|e| AppError::Other(e.to_string()))?;

    // Download images sequentially — TikTok CDN is fine with parallel but
    // 5-15 images won't take long either way. Sequential keeps the code simple.
    let total = post.images.len();
    let pad = ((total as f64).log10().floor() as usize) + 1;
    for (i, img_url) in post.images.iter().enumerate() {
        let idx = i + 1;
        // Detect extension from URL; default jpg for TikTok photomode.
        let ext = guess_image_ext(img_url).unwrap_or("jpg");
        let path = folder.join(format!("{:0width$}.{ext}", idx, width = pad));
        download_one(&client, img_url, &path).await?;
    }

    // Best-effort audio: nice-to-have but failure shouldn't kill the whole post.
    if let Some(audio_url) = &post.audio_url {
        let path = folder.join("audio.mp3");
        let _ = download_one(&client, audio_url, &path).await;
    }

    Ok(folder)
}

async fn download_one(
    client: &reqwest::Client,
    url: &str,
    path: &Path,
) -> AppResult<()> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(AppError::Other(format!(
            "HTTP {} fetching {}",
            resp.status(),
            url
        )));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;
    std::fs::write(path, &bytes).map_err(AppError::from)?;
    Ok(())
}

fn guess_image_ext(url: &str) -> Option<&'static str> {
    let lower = url.to_lowercase();
    if lower.contains(".jpeg") || lower.contains(".jpg") {
        Some("jpg")
    } else if lower.contains(".png") {
        Some("png")
    } else if lower.contains(".webp") {
        Some("webp")
    } else if lower.contains(".heic") {
        Some("heic")
    } else {
        None
    }
}
