//! Douyin Channel Fetcher
//!
//! Lấy danh sách video từ kênh Douyin. Các chiến lược (theo thứ tự ưu tiên):
//!  1. TikWM API (https://www.tikwm.com) — phổ biến, miễn phí
//!  2. TikWM mirror (tikwm.com, api.tikwm.com) — fallback khi primary down
//!
//! Lưu ý: Các API này có thể chặn IP Việt Nam hoặc bị rate-limit.
//! Nếu không hoạt động, user cần dùng VPN hoặc dùng link video riêng lẻ.

use crate::error::{AppError, AppResult};
use regex::Regex;
use serde::Deserialize;
use std::time::Duration;
use tauri::Emitter;

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                  (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36";

fn extract_sec_uid(url: &str) -> Option<String> {
    let re = Regex::new(r"douyin\.com/user/([A-Za-z0-9_-]{20,})").ok()?;
    Some(re.captures(url)?.get(1)?.as_str().to_string())
}

async fn resolve_short_url(short_url: &str) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(UA)
        .build()
        .ok()?;

    let resp = client.head(short_url).send().await.ok()?;
    extract_sec_uid(resp.url().as_str())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DouyinPost {
    pub id: String,
    pub url: String,
    pub title: String,
    pub thumbnail: String,
    pub is_photo: bool,
}

#[derive(Debug, Deserialize)]
struct TikwmResponse {
    code: i32,
    #[serde(default)]
    msg: String,
    #[serde(default)]
    data: Option<TikwmData>,
}

#[derive(Debug, Deserialize)]
struct TikwmData {
    #[serde(default)]
    aweme_list: Vec<TikwmAweme>,
    #[serde(default)]
    has_more: bool,
}

#[derive(Debug, Deserialize)]
struct TikwmAweme {
    aweme_id: Option<String>,
    desc: Option<String>,
    #[serde(default)]
    video: Option<TikwmVideo>,
    #[serde(default)]
    images: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize, Default)]
struct TikwmVideo {
    #[serde(default)]
    cover: Option<TikwmCover>,
}

#[derive(Debug, Deserialize, Default)]
struct TikwmCover {
    #[serde(default)]
    url_list: Vec<String>,
}

/// Gọi 1 endpoint cụ thể. Trả về (posts, has_more) hoặc lỗi.
async fn try_tikwm_endpoint(
    client: &reqwest::Client,
    endpoint: &str,
    user_url: &str,
    sec_uid: &str,
    page: usize,
) -> AppResult<(Vec<DouyinPost>, bool)> {
    let resp = client
        .get(endpoint)
        .query(&[
            ("url", user_url),
            ("sec_uid", sec_uid),
            ("count", "35"),
            ("page", &page.to_string()),
            ("hd", "1"),
        ])
        .send()
        .await
        .map_err(|e| {
            AppError::Other(format!("Kết nối thất bại. Lỗi: {}", e))
        })?;

    if !resp.status().is_success() {
        return Err(AppError::Other(format!(
            "API trả lỗi HTTP {}",
            resp.status()
        )));
    }

    let body = resp.text().await
        .map_err(|e| AppError::Other(format!("Đọc response thất bại: {}", e)))?;

    let json: TikwmResponse = serde_json::from_str(&body)
        .map_err(|e| AppError::Other(format!("Dữ liệu không hợp lệ: {}", e)))?;

    if json.code != 0 {
        let msg = json.msg.as_str();
        if msg.is_empty() {
            return Err(AppError::Other("API trả lỗi không xác định".into()));
        }
        if msg.contains("Too Many") || msg.contains("rate") || msg.contains("limit") {
            return Err(AppError::Other(
                "API bị giới hạn tạm thời. Thử lại sau 1-2 phút.".into(),
            ));
        }
        if msg.contains("user") || msg.contains("not found") || msg.contains("private") {
            return Err(AppError::Other(
                "Kênh này có thể ở chế độ riêng tư hoặc không tồn tại.".into(),
            ));
        }
        return Err(AppError::Other(format!("API lỗi: {msg}")));
    }

    let data = json.data
        .ok_or_else(|| AppError::Other("API không trả dữ liệu. Kênh có thể không công khai.".into()))?;

    let posts: Vec<DouyinPost> = data.aweme_list
        .into_iter()
        .filter_map(|aweme| {
            let id = aweme.aweme_id?;
            let thumb = aweme
                .video
                .as_ref()
                .and_then(|v| v.cover.as_ref())
                .and_then(|c| c.url_list.first())
                .cloned()
                .unwrap_or_default();

            Some(DouyinPost {
                id: id.clone(),
                url: format!("https://www.douyin.com/video/{id}"),
                title: aweme.desc.unwrap_or_default(),
                thumbnail: thumb,
                is_photo: !aweme.images.is_empty(),
            })
        })
        .collect();

    Ok((posts, data.has_more))
}

/// Thử tất cả endpoints cho đến khi 1 cái thành công.
async fn fetch_all_pages(
    client: &reqwest::Client,
    endpoints: &[&str],
    user_url: &str,
    sec_uid: &str,
) -> AppResult<Vec<DouyinPost>> {
    let mut all_posts: Vec<DouyinPost> = Vec::new();
    let mut page = 1;
    let mut has_more = true;
    let max_pages = 20;
    let mut tried_any = false;

    while has_more && page <= max_pages {
        let mut last_err: Option<String> = None;

        for endpoint in endpoints {
            match try_tikwm_endpoint(client, endpoint, user_url, sec_uid, page).await {
                Ok((posts, more)) => {
                    all_posts.extend(posts);
                    has_more = more;
                    last_err = None;
                    tried_any = true;
                    break;
                }
                Err(e) => {
                    last_err = Some(format_error(&e));
                }
            }
        }

        if last_err.is_some() {
            if !tried_any && page == 1 {
                return Err(AppError::Other(
                    "Không kết nối được API TikWM.\n\n\
                     Nguyên nhân có thể:\n\
                     1. TikWM chặn IP Việt Nam\n\
                     2. Firewall hoặc antivirus chặn kết nối\n\
                     3. Mạng không ổn định\n\n\
                     Giải pháp:\n\
                     • Dùng VPN để đổi IP\n\
                     • Tắt tạm firewall/antivirus\n\
                     • Thử lại sau".into(),
                ));
            }
            // Đã lấy được vài trang rồi — chấp nhận kết quả hiện tại
            break;
        }

        page += 1;
    }

    // Deduplicate
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    all_posts.retain(|p| seen.insert(p.id.clone()));

    Ok(all_posts)
}

fn format_error(e: &AppError) -> String {
    match e {
        AppError::Other(msg) => msg.clone(),
        _ => format!("{:?}", e),
    }
}

#[tauri::command]
pub async fn scrape_douyin_channel(
    app: tauri::AppHandle,
    url: String,
) -> AppResult<Vec<DouyinPost>> {
    let lower = url.to_lowercase();
    if !lower.contains("douyin.com") {
        return Err(AppError::Other("URL không phải liên kết Douyin".into()));
    }

    // Extract sec_uid
    let sec_uid = if lower.contains("/user/") {
        extract_sec_uid(&url).ok_or_else(|| {
            AppError::Other("Không tìm được sec_uid. Dùng URL dạng douyin.com/user/...".into())
        })?
    } else if lower.contains("v.douyin.com") {
        resolve_short_url(&url).await.ok_or_else(|| {
            AppError::Other("Không theo được redirect. Thử URL đầy đủ (douyin.com/user/...)".into())
        })?
    } else {
        return Err(AppError::Other(
            "URL Douyin không nhận diện được.\nDùng dạng:\n• douyin.com/user/...\n• v.douyin.com/...".into(),
        ));
    };

    let user_url = format!("https://www.douyin.com/user/{sec_uid}");

    let _ = app.emit(
        "bqd-douyin-scraper-started",
        serde_json::json!({ "label": "tikwm", "secUid": &sec_uid }),
    );

    let client = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent(UA)
        .build()
        .map_err(|e| AppError::Other(format!("Khởi tạo HTTP client thất bại: {e}")))?;

    let endpoints = [
        "https://www.tikwm.com/api/",
        "https://tikwm.com/api/",
        "https://api.tikwm.com/api/",
    ];

    let posts = fetch_all_pages(&client, &endpoints, &user_url, &sec_uid).await?;

    let _ = app.emit(
        "bqd-douyin-scraper-progress",
        serde_json::json!({ "count": posts.len() }),
    );

    Ok(posts)
}
