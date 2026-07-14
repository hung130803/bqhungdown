//! Douyin Channel Fetcher — lấy danh sách video của cả kênh.
//!
//! Chiến lược (theo thứ tự ưu tiên):
//!  1. API TRỰC TIẾP douyin.com — tự ký `a_bogus` (xem `douyin_sign.rs`),
//!     lấy ttwid, phân trang bằng max_cursor. Không cần dịch vụ trung gian,
//!     không cần cookie/proxy. Đây là đường chính.
//!  2. TikWM (dự phòng) — thường đã bị Cloudflare chặn, chỉ thử nốt.
//!
//! Nhận URL dạng `douyin.com/user/<sec_uid>` hoặc link rút gọn `v.douyin.com/…`.

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

async fn resolve_short_url(short_url: &str, proxy: &Option<String>) -> Option<String> {
    let mut b = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(UA);
    if let Some(px) = proxy {
        if let Ok(p) = reqwest::Proxy::all(px) {
            b = b.proxy(p);
        }
    }
    let client = b.build().ok()?;
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

// ─────────────────────────────────────────────────────────────────────────
//  API TRỰC TIẾP douyin.com (tự ký a_bogus) — đường CHÍNH, không cần dịch vụ
//  trung gian. tikwm ở dưới chỉ còn là phao dự phòng.
// ─────────────────────────────────────────────────────────────────────────

use crate::douyin_sign::{ABogus, DOUYIN_UA};

#[derive(Debug, Deserialize)]
struct PostResp {
    #[serde(default)]
    status_code: i32,
    #[serde(default)]
    max_cursor: i64,
    #[serde(default)]
    has_more: i64,
    #[serde(default)]
    aweme_list: Vec<TikwmAweme>,
}

/// Percent-encode a_bogus (giữ ký tự unreserved, mã hoá phần còn lại).
fn pct_encode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => {
                let mut b = [0u8; 4];
                c.encode_utf8(&mut b)
                    .bytes()
                    .map(|x| format!("%{x:02X}"))
                    .collect()
            }
        })
        .collect()
}

/// Gắn proxy vào builder nếu có (định dạng đã normalize).
fn with_proxy(mut b: reqwest::ClientBuilder, proxy: &Option<String>) -> reqwest::ClientBuilder {
    if let Some(px) = proxy {
        if let Ok(p) = reqwest::Proxy::all(px) {
            b = b.proxy(p);
        }
    }
    b
}

/// Lấy cookie ttwid từ douyin.com (best-effort; thiếu vẫn thử gọi API).
/// QUAN TRỌNG: phải dùng client KHÔNG đặt User-Agent trình duyệt — với UA
/// Chrome, Douyin trả `__ac_nonce` (đòi giải JS challenge) thay vì ttwid;
/// với UA mặc định của reqwest thì nó cấp ttwid luôn. ttwid dùng chéo UA OK.
async fn fetch_ttwid(proxy: &Option<String>) -> Option<String> {
    let client = with_proxy(
        reqwest::Client::builder().timeout(Duration::from_secs(15)),
        proxy,
    )
    .build()
    .ok()?;
    let resp = client.get("https://www.douyin.com/").send().await.ok()?;
    for val in resp.headers().get_all(reqwest::header::SET_COOKIE).iter() {
        if let Ok(s) = val.to_str() {
            if let Some(rest) = s.strip_prefix("ttwid=") {
                if let Some(v) = rest.split(';').next() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

/// Chuỗi query cho endpoint aweme/post (chưa gồm a_bogus). Thứ tự + giá trị
/// khớp web thật; browser_version phải là 90.x cho khớp DOUYIN_UA/ua_code.
fn build_post_params(sec_uid: &str, max_cursor: i64) -> String {
    format!(
        "device_platform=webapp&aid=6383&channel=channel_pc_web&sec_user_id={sec_uid}\
&max_cursor={max_cursor}&locate_query=false&show_live_replay_strategy=1&need_time_list=1\
&time_list_query=0&whale_cut_token=&cut_version=1&count=18&publish_video_strategy_type=2\
&version_code=290100&version_name=29.1.0&cookie_enabled=true&screen_width=1536\
&screen_height=864&browser_language=zh-CN&browser_platform=Win32&browser_name=Chrome\
&browser_version=90.0.4430.212&browser_online=true&engine_name=Blink&engine_version=90.0\
&os_name=Windows&os_version=10&cpu_core_num=8&device_memory=8&platform=PC&downlink=10\
&effective_type=4g&round_trip_time=50"
    )
}

fn awemes_to_posts(list: Vec<TikwmAweme>) -> Vec<DouyinPost> {
    list.into_iter()
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
        .collect()
}

/// Lấy TOÀN BỘ video của kênh qua API douyin.com, phân trang bằng max_cursor.
/// Tự ký a_bogus mỗi lần gọi. Emit tiến độ để UI cập nhật số video.
async fn fetch_channel_api(
    app: &tauri::AppHandle,
    sec_uid: &str,
    proxy: &Option<String>,
) -> AppResult<Vec<DouyinPost>> {
    let client = with_proxy(
        reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .user_agent(DOUYIN_UA),
        proxy,
    )
    .build()
    .map_err(|e| AppError::Other(format!("HTTP client lỗi: {e}")))?;

    let ttwid = fetch_ttwid(proxy).await;
    let ab = ABogus::new();
    let referer = format!("https://www.douyin.com/user/{sec_uid}");

    let mut all: Vec<DouyinPost> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut cursor: i64 = 0;
    const MAX_PAGES: usize = 120; // 120 × 18 ≈ 2160 video — dư cho hầu hết kênh

    for page in 0..MAX_PAGES {
        let params = build_post_params(sec_uid, cursor);
        let a_bogus = ab.get_value(&params, "GET");
        let url = format!(
            "https://www.douyin.com/aweme/v1/web/aweme/post/?{params}&a_bogus={}",
            pct_encode(&a_bogus)
        );

        let mut req = client.get(&url).header("Referer", &referer);
        if let Some(tt) = &ttwid {
            req = req.header("Cookie", format!("ttwid={tt}"));
        }
        let resp = req
            .send()
            .await
            .map_err(|e| AppError::Other(format!("Gọi API Douyin thất bại: {e}")))?;
        let body = resp.text().await.unwrap_or_default();

        // Rỗng = chữ ký bị từ chối / rate-limit. Trang đầu rỗng → lỗi thật;
        // trang sau rỗng → dừng, giữ những gì đã lấy.
        if body.trim().is_empty() {
            if page == 0 {
                return Err(AppError::Other(
                    "Douyin không trả dữ liệu (có thể đã đổi thuật toán chặn, hoặc kênh riêng tư). \
                     Thử lại sau, hoặc thêm cookie Douyin trong Cài đặt."
                        .into(),
                ));
            }
            break;
        }

        let json: PostResp = match serde_json::from_str(&body) {
            Ok(j) => j,
            Err(_) if page > 0 => break,
            Err(e) => {
                return Err(AppError::Other(format!("Dữ liệu Douyin không hợp lệ: {e}")))
            }
        };
        if json.status_code != 0 {
            if page == 0 {
                return Err(AppError::Other(format!(
                    "Douyin trả mã lỗi {} — kênh có thể riêng tư hoặc cần cookie.",
                    json.status_code
                )));
            }
            break;
        }

        let fresh = awemes_to_posts(json.aweme_list);
        for p in fresh {
            if seen.insert(p.id.clone()) {
                all.push(p);
            }
        }

        let _ = app.emit(
            "bqd-douyin-scraper-progress",
            serde_json::json!({ "count": all.len() }),
        );

        if json.has_more != 1 || json.max_cursor == 0 || json.max_cursor == cursor {
            break;
        }
        cursor = json.max_cursor;
        // Nghỉ nhẹ giữa các trang cho đỡ bị rate-limit.
        tokio::time::sleep(Duration::from_millis(400)).await;
    }

    Ok(all)
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
    settings: tauri::State<'_, std::sync::Arc<crate::settings_store::SettingsStore>>,
) -> AppResult<Vec<DouyinPost>> {
    // Proxy do user cấu hình — Douyin hay chặn IP Việt Nam; đi qua proxy
    // (vd proxy Nhật) thì API mới trả dữ liệu. Không có proxy → đi thẳng.
    let proxy = crate::args_builder::next_proxy(&settings.get());

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
        resolve_short_url(&url, &proxy).await.ok_or_else(|| {
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
        serde_json::json!({ "label": "douyin-api", "secUid": &sec_uid }),
    );

    // Đường CHÍNH: API douyin.com tự ký a_bogus. QUAN TRỌNG — thử TRỰC TIẾP
    // trước (IP thật): Douyin thường chặn proxy DATACENTER (đã kiểm chứng:
    // proxy datacenter → ttwid fail), nên KHÔNG ép proxy cho Douyin dù user
    // có cấu hình proxy (để tải bilibili). Chỉ khi trực tiếp thất bại mới thử
    // lại qua proxy (may ra proxy dân cư giúp được).
    let mut attempts: Vec<Option<String>> = vec![None];
    if proxy.is_some() {
        attempts.push(proxy.clone());
    }
    let mut api_err: Option<String> = None;
    for (i, px) in attempts.iter().enumerate() {
        match fetch_channel_api(&app, &sec_uid, px).await {
            Ok(posts) if !posts.is_empty() => {
                let _ = app.emit(
                    "bqd-douyin-scraper-progress",
                    serde_json::json!({ "count": posts.len() }),
                );
                return Ok(posts);
            }
            Ok(_) => {
                eprintln!("[douyin] lần {i} (proxy={}) rỗng", px.is_some());
                api_err.get_or_insert_with(|| {
                    "Douyin trả về rỗng (kênh riêng tư/không có video, hoặc bị chặn tạm)".into()
                });
            }
            Err(e) => {
                eprintln!("[douyin] lần {i} (proxy={}) lỗi: {e:?}", px.is_some());
                api_err = Some(format!("{e}"));
            }
        }
    }

    // Phao dự phòng: tikwm (thường đã bị Cloudflare chặn, nhưng thử nốt).
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

    match fetch_all_pages(&client, &endpoints, &user_url, &sec_uid).await {
        Ok(posts) if !posts.is_empty() => {
            let _ = app.emit(
                "bqd-douyin-scraper-progress",
                serde_json::json!({ "count": posts.len() }),
            );
            Ok(posts)
        }
        // Cả API chính lẫn tikwm đều thất bại → báo LỖI THẬT của đường chính
        // (đường chính mới là cái quan trọng), kèm sec_uid để chẩn đoán.
        _ => Err(AppError::Other(format!(
            "Không lấy được video của kênh Douyin này.\n\
             Nguyên nhân (từ API chính): {}\n\
             sec_uid: {}\n\
             Thử: kiểm tra link kênh (dạng douyin.com/user/…), thử lại sau vài phút, \
             hoặc kênh có thể riêng tư/trống.",
            api_err.unwrap_or_else(|| "không rõ".into()),
            &sec_uid[..sec_uid.len().min(24)],
        ))),
    }
}
