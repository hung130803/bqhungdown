//! Lấy danh sách video + metadata chính xác của cả kênh YouTube qua
//! **YouTube Data API v3** (API chính thức của Google).
//!
//! Vì sao có module này: nút "lấy chi tiết" cũ chạy yt-dlp dò TỪNG video để
//! lấy view/ngày → kênh 1000 video mất 1-2 tiếng. API này lấy **50 video mỗi
//! lượt gọi** nên cả kênh xong trong vài giây, lại cho view/thời lượng/ngày/
//! hashtag CHÍNH XÁC (không phải số làm tròn), và **không dính bot** vì là API
//! key chứ không cào trang.
//!
//! Quota: mỗi key miễn phí 10.000 lượt/ngày. Một kênh 1000 video tốn ~40 lượt
//! (1 resolve + 20 playlistItems + 20 videos.list) → thừa sức tra hàng trăm
//! kênh/ngày.

use crate::error::{AppError, AppResult};
use crate::channel_cache::ChannelCache;
use crate::models::{ChannelInfo, ChannelVideo};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

const API_BASE: &str = "https://www.googleapis.com/youtube/v3";
/// Số lô videos.list chạy song song khi lấy chi tiết. 8 lô × 50 = 400 video
/// "bay" cùng lúc — nhanh mà vẫn dưới ngưỡng rate-limit/phút của API.
const DETAIL_CONCURRENCY: usize = 8;
/// Trần an toàn số video lấy về khi người dùng chọn "tất cả" (limit = 0) —
/// khớp với hành vi cũ của app, tránh kênh khổng lồ kéo vô tận.
const HARD_CAP: u32 = 5000;

fn build_client() -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| AppError::YtDlpFailed(format!("Không tạo được HTTP client: {e}")))
}

/// Dịch lỗi JSON của Google API sang thông báo tiếng Việt dễ hiểu.
fn friendly_api_error(err: &Value) -> String {
    let reason = err
        .get("errors")
        .and_then(|e| e.get(0))
        .and_then(|e| e.get("reason"))
        .and_then(|r| r.as_str())
        .unwrap_or("");
    let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("");
    match reason {
        "keyInvalid" | "badRequest" => {
            "API key không hợp lệ. Kiểm tra lại chuỗi key đã dán.".into()
        }
        "quotaExceeded" | "dailyLimitExceeded" | "rateLimitExceeded" => {
            "Đã hết lượt API hôm nay (10.000 lượt/ngày). Đợi qua ngày mai (reset \
             nửa đêm giờ Mỹ) hoặc tạo key mới."
                .into()
        }
        "accessNotConfigured" | "SERVICE_DISABLED" => {
            "Key này CHƯA bật \"YouTube Data API v3\". Vào console.cloud.google.com \
             → APIs & Services → Library → tìm \"YouTube Data API v3\" → Enable."
                .into()
        }
        "ipRefererBlocked" | "forbidden" => {
            "Key bị giới hạn (HTTP referrer/IP). Vào Credentials → sửa key → \
             Application restrictions = None."
                .into()
        }
        _ if !msg.is_empty() => format!("API báo lỗi: {msg}"),
        _ => "Gọi YouTube Data API thất bại (không rõ lý do).".into(),
    }
}

/// True nếu lỗi API là do hết quota / vượt rate-limit → nên nhảy sang key khác.
fn is_quota_reason(err: &Value) -> bool {
    let reason = err
        .get("errors")
        .and_then(|e| e.get(0))
        .and_then(|e| e.get("reason"))
        .and_then(|r| r.as_str())
        .unwrap_or("");
    matches!(
        reason,
        "quotaExceeded" | "dailyLimitExceeded" | "rateLimitExceeded" | "userRateLimitExceeded"
    )
}

/// Bể key xoay vòng: giữ danh sách key + key đang dùng. Khi key hiện tại hết
/// quota, `advance()` ghi nhận nó "hết" rồi nhảy sang key kế tiếp.
struct KeyPool {
    keys: Vec<String>,
    idx: usize,
    /// Index các key đã hết quota trong lượt fetch này (để báo lên UI).
    exhausted: Vec<usize>,
}

impl KeyPool {
    fn new(keys: Vec<String>) -> Self {
        KeyPool { keys, idx: 0, exhausted: Vec::new() }
    }

    /// Key đang dùng (bản sao, để nhả lock trước khi gọi mạng). Err khi đã
    /// nhảy hết tất cả key (đều hết quota).
    fn current(&self) -> AppResult<String> {
        self.keys.get(self.idx).cloned().ok_or_else(|| {
            AppError::YtDlpFailed(
                "Tất cả YouTube API key đều đã hết lượt hôm nay. Thêm key mới trong \
                 Cài đặt, hoặc đợi reset (khoảng 14-15h chiều VN)."
                    .into(),
            )
        })
    }

    /// Đánh dấu key hiện tại đã hết quota rồi chuyển sang key tiếp theo.
    fn advance(&mut self) {
        if !self.exhausted.contains(&self.idx) {
            self.exhausted.push(self.idx);
        }
        self.idx += 1;
    }
}

/// Gọi 1 endpoint API (URL CHƯA gắn `key=`), tự gắn key từ `pool`. Nếu key hết
/// quota → nhảy key kế và thử lại CÙNG request. Bóc lỗi {error:{...}} khác
/// thành AppError tiếng Việt.
async fn api_get(
    client: &reqwest::Client,
    pool: &Arc<Mutex<KeyPool>>,
    url_no_key: &str,
) -> AppResult<Value> {
    loop {
        // Đọc key hiện tại + index của nó, rồi NHẢ lock trước khi gọi mạng để
        // các lô song song khác không bị chặn.
        let (key, my_idx) = {
            let p = pool.lock().await;
            (p.current()?, p.idx)
        };
        let sep = if url_no_key.contains('?') { '&' } else { '?' };
        let url = format!("{url_no_key}{sep}key={key}");
        let body = single_get(client, &url).await?;
        if let Some(err) = body.get("error") {
            if is_quota_reason(err) {
                // Key này hết quota. Chỉ advance nếu chưa có lô song song nào
                // advance trước (idx vẫn bằng idx lúc ta đọc) → tránh nhảy lố.
                let mut p = pool.lock().await;
                if p.idx == my_idx {
                    p.advance();
                }
                continue;
            }
            return Err(AppError::YtDlpFailed(friendly_api_error(err)));
        }
        return Ok(body);
    }
}

/// Gửi 1 GET, trả body JSON (kể cả khi body chứa `error`). Chỉ lỗi mạng/parse
/// mới thành Err ở đây — lỗi API do caller xử lý.
async fn single_get(client: &reqwest::Client, url: &str) -> AppResult<Value> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::YtDlpFailed(format!("Lỗi mạng khi gọi API: {e}")))?;
    let status = resp.status();
    resp.json().await.map_err(|e| {
        AppError::YtDlpFailed(format!("Không đọc được phản hồi API (HTTP {status}): {e}"))
    })
}

/// Kiểm tra key có dùng được không. Ok(()) = xanh, Err(msg) = đỏ + lý do.
/// Tốn 1 quota unit (videos.list trên 1 video mẫu).
pub async fn validate_key(key: &str) -> AppResult<()> {
    let key = key.trim();
    if key.is_empty() {
        return Err(AppError::YtDlpFailed("Chưa nhập API key.".into()));
    }
    let client = build_client()?;
    // dQw4w9WgXcQ là 1 video công khai luôn tồn tại → phép thử rẻ + ổn định.
    let url = format!("{API_BASE}/videos?part=id&id=dQw4w9WgXcQ&key={key}");
    let body = single_get(&client, &url).await?;
    if let Some(err) = body.get("error") {
        return Err(AppError::YtDlpFailed(friendly_api_error(err)));
    }
    if body.get("items").and_then(|i| i.as_array()).is_some() {
        Ok(())
    } else {
        Err(AppError::YtDlpFailed(
            "Key phản hồi nhưng không đúng định dạng mong đợi.".into(),
        ))
    }
}

/// Parse ISO-8601 duration ("PT1H2M3S", "PT15M", "PT45S", "P1DT2H") → giây.
fn parse_iso8601_duration(s: &str) -> Option<u64> {
    let s = s.strip_prefix('P')?;
    let mut secs: u64 = 0;
    let mut num = String::new();
    let mut in_time = false;
    for c in s.chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' => num.push(c),
            'D' => {
                secs += num.parse::<u64>().ok()? * 86_400;
                num.clear();
            }
            'H' => {
                secs += num.parse::<u64>().ok()? * 3_600;
                num.clear();
            }
            'M' => {
                // Trong phần thời gian (sau 'T') M = phút; hiếm video nào dùng
                // M = tháng nên bỏ qua nhánh đó.
                if in_time {
                    secs += num.parse::<u64>().ok()? * 60;
                }
                num.clear();
            }
            'S' => {
                secs += num.parse::<u64>().ok()?;
                num.clear();
            }
            _ => {}
        }
    }
    Some(secs)
}

/// Rút hashtag (#abc) từ tiêu đề + mô tả. Hỗ trợ chữ có dấu tiếng Việt
/// (is_alphanumeric của Rust tính cả ký tự Unicode). Gộp trùng, tối đa 30.
fn extract_hashtags(title: &str, desc: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for text in [title, desc] {
        // skip(1): đoạn đầu tiên là phần TRƯỚC dấu # đầu tiên (không phải tag).
        // Mỗi đoạn sau đó nằm ngay sau một dấu # → lấy phần chữ/số liền sau.
        for chunk in text.split('#').skip(1) {
            let tag: String = chunk
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if tag.is_empty() {
                continue;
            }
            let key = tag.to_lowercase();
            if seen.insert(key) {
                out.push(format!("#{tag}"));
                if out.len() >= 30 {
                    return out;
                }
            }
        }
    }
    out
}

/// publishedAt ("2024-01-15T10:30:00Z") → "20240115".
fn published_to_date(published: &str) -> Option<String> {
    let date_part = published.get(0..10)?; // YYYY-MM-DD
    let digits: String = date_part.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() == 8 {
        Some(digits)
    } else {
        None
    }
}

struct Resolved {
    uploads_playlist: String,
    info: ChannelInfo,
}

/// Từ URL kênh → channel_id + uploads playlist + thông tin kênh.
/// Hỗ trợ /channel/UC..., /@handle, /user/name, /c/custom (custom dùng search).
async fn resolve_channel(
    client: &reqwest::Client,
    pool: &Arc<Mutex<KeyPool>>,
    url: &str,
) -> AppResult<Resolved> {
    let lower = url.to_lowercase();

    // Xác định tham số lookup cho channels.list.
    let lookup: String = if let Some(rest) = lower.split("/channel/").nth(1) {
        let id = rest
            .split(|c| c == '/' || c == '?' || c == '&' || c == '#')
            .next()
            .unwrap_or("");
        format!("id={id}")
    } else if let Some(rest) = url.split("/@").nth(1) {
        // handle giữ nguyên hoa/thường; cắt phần đuôi.
        let handle = rest
            .split(|c| c == '/' || c == '?' || c == '&' || c == '#')
            .next()
            .unwrap_or("");
        format!("forHandle=@{handle}")
    } else if let Some(rest) = lower.split("/user/").nth(1) {
        let name = rest
            .split(|c| c == '/' || c == '?' || c == '&' || c == '#')
            .next()
            .unwrap_or("");
        format!("forUsername={name}")
    } else if let Some(rest) = url.split("/c/").nth(1) {
        // Custom URL cũ — không có tham số trực tiếp, phải search.
        let name = rest
            .split(|c| c == '/' || c == '?' || c == '&' || c == '#')
            .next()
            .unwrap_or("");
        let cid = search_channel_id(client, pool, name).await?;
        format!("id={cid}")
    } else {
        return Err(AppError::YtDlpFailed(
            "Link kênh không nhận diện được (cần dạng /channel/, /@tên, /user/ hoặc /c/).".into(),
        ));
    };

    let url = format!("{API_BASE}/channels?part=snippet,contentDetails,statistics&{lookup}");
    let body = api_get(client, pool, &url).await?;
    let item = body
        .get("items")
        .and_then(|i| i.as_array())
        .and_then(|a| a.first())
        .ok_or_else(|| AppError::YtDlpFailed("Không tìm thấy kênh với link này.".into()))?;

    let channel_id = item
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let uploads_playlist = item
        .pointer("/contentDetails/relatedPlaylists/uploads")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AppError::YtDlpFailed("Kênh không có playlist uploads (lỗi bất thường).".into())
        })?
        .to_string();
    let title = item
        .pointer("/snippet/title")
        .and_then(|v| v.as_str())
        .unwrap_or("Kênh YouTube")
        .to_string();
    let thumbnail = item
        .pointer("/snippet/thumbnails/high/url")
        .or_else(|| item.pointer("/snippet/thumbnails/default/url"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let video_count = item
        .pointer("/statistics/videoCount")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u32>().ok());

    let info = ChannelInfo {
        url: format!("https://www.youtube.com/channel/{channel_id}"),
        title,
        thumbnail,
        video_count,
        extractor: "youtube:api".into(),
        hidden_downloaded: None,
        channel_id: Some(channel_id.clone()),
        api_note: None,
    };

    Ok(Resolved {
        uploads_playlist,
        info,
    })
}

/// Resolve custom (/c/) URL → channelId qua search.list (tốn 100 quota).
async fn search_channel_id(
    client: &reqwest::Client,
    pool: &Arc<Mutex<KeyPool>>,
    query: &str,
) -> AppResult<String> {
    let q = urlencoding(query);
    let url = format!("{API_BASE}/search?part=snippet&type=channel&maxResults=1&q={q}");
    let body = api_get(client, pool, &url).await?;
    body.pointer("/items/0/id/channelId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AppError::YtDlpFailed("Không tìm thấy kênh khớp với tên này.".into()))
}

/// Mã hoá tối thiểu cho query string (đủ cho tên kênh có dấu cách/Unicode).
fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Lấy toàn bộ (đến `cap`) video id trong playlist uploads, mới nhất trước.
async fn list_upload_ids(
    client: &reqwest::Client,
    pool: &Arc<Mutex<KeyPool>>,
    uploads_playlist: &str,
    cap: u32,
    // Khi Some: dừng sớm ngay khi gặp 1 id đã có trong tập này (incremental).
    // Trả về (ids mới, có_gặp_id_đã_cache).
    stop_at_cached: Option<&HashSet<String>>,
) -> AppResult<(Vec<String>, bool)> {
    let mut ids: Vec<String> = Vec::new();
    let mut hit_cached = false;
    let mut page_token: Option<String> = None;
    loop {
        let token_param = page_token
            .as_deref()
            .map(|t| format!("&pageToken={t}"))
            .unwrap_or_default();
        let url = format!(
            "{API_BASE}/playlistItems?part=contentDetails&maxResults=50&playlistId={uploads_playlist}{token_param}"
        );
        let body = api_get(client, pool, &url).await?;
        if let Some(items) = body.get("items").and_then(|i| i.as_array()) {
            for it in items {
                if let Some(id) = it
                    .pointer("/contentDetails/videoId")
                    .and_then(|v| v.as_str())
                {
                    // Incremental: uploads xếp mới→cũ, nên gặp id đã cache nghĩa
                    // là từ đây trở đi đều đã có → dừng, chỉ giữ phần mới.
                    if let Some(cached) = stop_at_cached {
                        if cached.contains(id) {
                            hit_cached = true;
                            break;
                        }
                    }
                    ids.push(id.to_string());
                }
            }
        }
        if hit_cached || ids.len() as u32 >= cap {
            ids.truncate(cap as usize);
            break;
        }
        match body.get("nextPageToken").and_then(|v| v.as_str()) {
            Some(t) => page_token = Some(t.to_string()),
            None => break,
        }
    }
    Ok((ids, hit_cached))
}

/// Parse 1 item trong videos.list → (id, ChannelVideo). None nếu thiếu id.
fn parse_video_item(it: &Value) -> Option<(String, ChannelVideo)> {
    let id = it.get("id").and_then(|v| v.as_str())?.to_string();
    let title = it
        .pointer("/snippet/title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let description = it
        .pointer("/snippet/description")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let upload_date = it
        .pointer("/snippet/publishedAt")
        .and_then(|v| v.as_str())
        .and_then(published_to_date);
    let view_count = it
        .pointer("/statistics/viewCount")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u64>().ok());
    let duration_sec = it
        .pointer("/contentDetails/duration")
        .and_then(|v| v.as_str())
        .and_then(parse_iso8601_duration);
    let thumbnail = it
        .pointer("/snippet/thumbnails/medium/url")
        .or_else(|| it.pointer("/snippet/thumbnails/default/url"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let hashtags = extract_hashtags(&title, description);
    // YouTube Data API không gắn cờ Shorts; dùng heuristic thời lượng ngắn
    // (≤ 180s) khớp looks_like_short — đúng giới hạn YouTube Shorts (3 phút).
    let is_short = duration_sec.map(|d| d > 0 && d <= 180).unwrap_or(false);
    Some((
        id.clone(),
        ChannelVideo {
            url: format!("https://www.youtube.com/watch?v={id}"),
            title,
            duration_sec,
            view_count,
            upload_date,
            thumbnail,
            is_photo: false,
            is_short,
            hashtags,
        },
    ))
}

/// Lấy chi tiết cho 1 lô (≤ 50 id) → danh sách (id, video).
async fn fetch_detail_chunk(
    client: &reqwest::Client,
    pool: &Arc<Mutex<KeyPool>>,
    chunk: &[String],
) -> AppResult<Vec<(String, ChannelVideo)>> {
    let id_param = chunk.join(",");
    let url =
        format!("{API_BASE}/videos?part=snippet,statistics,contentDetails&id={id_param}");
    let body = api_get(client, pool, &url).await?;
    let items = match body.get("items").and_then(|i| i.as_array()) {
        Some(a) => a,
        None => return Ok(Vec::new()),
    };
    Ok(items.iter().filter_map(parse_video_item).collect())
}

/// Lấy metadata chi tiết cho danh sách id, CHẠY SONG SONG nhiều lô (mỗi lô 50)
/// để nhanh hơn ~vài lần. Trả map id → video.
async fn fetch_video_details(
    client: &reqwest::Client,
    pool: &Arc<Mutex<KeyPool>>,
    ids: &[String],
) -> AppResult<std::collections::HashMap<String, ChannelVideo>> {
    use tokio::task::JoinSet;

    let chunks: Vec<Vec<String>> = ids.chunks(50).map(|c| c.to_vec()).collect();
    let mut map = std::collections::HashMap::new();
    let mut iter = chunks.into_iter();
    let mut set: JoinSet<AppResult<Vec<(String, ChannelVideo)>>> = JoinSet::new();

    // Mồi tối đa DETAIL_CONCURRENCY lô chạy cùng lúc.
    for _ in 0..DETAIL_CONCURRENCY {
        if let Some(chunk) = iter.next() {
            let client = client.clone();
            let pool = pool.clone();
            set.spawn(async move { fetch_detail_chunk(&client, &pool, &chunk).await });
        }
    }
    while let Some(joined) = set.join_next().await {
        // Có slot trống → mồi tiếp 1 lô để giữ ống đầy.
        if let Some(chunk) = iter.next() {
            let client = client.clone();
            let pool = pool.clone();
            set.spawn(async move { fetch_detail_chunk(&client, &pool, &chunk).await });
        }
        match joined {
            Ok(Ok(items)) => {
                for (id, v) in items {
                    map.insert(id, v);
                }
            }
            // Lỗi API thật (vd hết sạch key) → huỷ phần còn lại + báo lên trên
            // để channel_fetcher quay về yt-dlp.
            Ok(Err(e)) => {
                set.abort_all();
                return Err(e);
            }
            // Task panic → bỏ qua lô đó (hiếm), không làm sập cả lượt.
            Err(_) => {}
        }
    }
    Ok(map)
}

/// Gộp video mới (newest-first) lên trước video đã cache (cũng newest-first),
/// bỏ trùng theo url, cắt còn `cap`. Kết quả vẫn theo thứ tự mới → cũ.
fn merge_incremental(
    new_videos: Vec<ChannelVideo>,
    cached: Vec<ChannelVideo>,
    cap: usize,
) -> Vec<ChannelVideo> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<ChannelVideo> = Vec::with_capacity(new_videos.len() + cached.len());
    for v in new_videos.into_iter().chain(cached.into_iter()) {
        if seen.insert(v.url.clone()) {
            out.push(v);
            if out.len() >= cap {
                break;
            }
        }
    }
    out
}

/// Lấy danh sách + metadata chính xác của cả kênh. `limit = 0` → tất cả (đến
/// HARD_CAP). Giữ thứ tự mới-nhất-trước từ playlist uploads.
///
/// `keys`: nhiều API key — key nào hết quota giữa chừng tự nhảy sang key kế.
/// `cache`: nếu Some và KHÔNG `force_refresh` → chỉ lấy video MỚI so với lần
/// trước (tiết kiệm quota + nhanh). `force_refresh = true` → lấy lại toàn bộ
/// (view mới nhất). `info.api_note` báo nếu có nhảy key / có dùng cache.
pub async fn fetch_channel(
    url: &str,
    keys: &[String],
    limit: u32,
    cache: Option<&ChannelCache>,
    force_refresh: bool,
) -> AppResult<(ChannelInfo, Vec<ChannelVideo>)> {
    let keys: Vec<String> = keys
        .iter()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .collect();
    if keys.is_empty() {
        return Err(AppError::YtDlpFailed("Chưa nhập YouTube API key.".into()));
    }
    let client = build_client()?;
    let pool = Arc::new(Mutex::new(KeyPool::new(keys)));

    let resolved = resolve_channel(&client, &pool, url).await?;
    let channel_id = resolved.info.channel_id.clone().unwrap_or_default();
    let cap_u32 = if limit == 0 { HARD_CAP } else { limit.min(HARD_CAP) };
    let cap = cap_u32 as usize;

    // Bộ nhớ đệm: chỉ dùng khi không ép làm mới + có channel_id hợp lệ.
    let cached: Option<Vec<ChannelVideo>> = if force_refresh || channel_id.is_empty() {
        None
    } else {
        cache.and_then(|c| c.load(&channel_id))
    };

    let mut info = resolved.info;
    let mut used_cache_note: Option<String> = None;

    let videos: Vec<ChannelVideo> = if let Some(cached_videos) = cached {
        // ----- Incremental: chỉ lấy video mới hơn cái mới nhất đã cache -----
        let cached_ids: HashSet<String> = cached_videos
            .iter()
            .filter_map(|v| crate::channel_fetcher::extract_video_id(&v.url))
            .collect();
        let (new_ids, _hit) =
            list_upload_ids(&client, &pool, &resolved.uploads_playlist, cap_u32, Some(&cached_ids))
                .await?;
        let new_count = new_ids.len();
        let new_videos = if new_ids.is_empty() {
            Vec::new()
        } else {
            let mut details = fetch_video_details(&client, &pool, &new_ids).await?;
            order_by_ids(&new_ids, &mut details)
        };
        used_cache_note = Some(if new_count == 0 {
            "♻️ Dùng bộ nhớ đệm — kênh không có video mới (gần như 0 quota).".to_string()
        } else {
            format!("♻️ Dùng bộ nhớ đệm — chỉ lấy thêm {new_count} video mới (tiết kiệm quota).")
        });
        merge_incremental(new_videos, cached_videos, cap)
    } else {
        // ----- Lấy toàn bộ (lần đầu hoặc force_refresh) -----
        let (ids, _) =
            list_upload_ids(&client, &pool, &resolved.uploads_playlist, cap_u32, None).await?;
        if ids.is_empty() {
            Vec::new()
        } else {
            let mut details = fetch_video_details(&client, &pool, &ids).await?;
            order_by_ids(&ids, &mut details)
        }
    };

    // `statistics.videoCount` của Google hay sai (bỏ Shorts, bị trễ) → dùng số
    // video THỰC SỰ có để "bài trên kênh" khớp danh sách. (Sửa "số lượng lệch".)
    info.video_count = Some(videos.len() as u32);

    // Ghi cache (toàn bộ danh sách sau khi gộp) cho lần sau.
    if let Some(c) = cache {
        if !channel_id.is_empty() {
            c.save(&channel_id, &videos);
        }
    }

    // Note: gộp thông báo nhảy key + dùng cache (nếu có).
    let failover_note = {
        let p = pool.lock().await;
        build_api_note(&p)
    };
    info.api_note = match (failover_note, used_cache_note) {
        (Some(a), Some(b)) => Some(format!("{b}\n{a}")),
        (Some(a), None) => Some(a),
        (None, b) => b,
    };

    Ok((info, videos))
}

/// Sắp xếp video theo đúng thứ tự `ids` (mới→cũ), bỏ id không có chi tiết.
fn order_by_ids(
    ids: &[String],
    details: &mut std::collections::HashMap<String, ChannelVideo>,
) -> Vec<ChannelVideo> {
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(v) = details.remove(id) {
            out.push(v);
        }
    }
    out
}

/// Soạn câu thông báo nhảy key (nếu có) cho UI. None khi không có key nào hết.
fn build_api_note(pool: &KeyPool) -> Option<String> {
    if pool.exhausted.is_empty() {
        return None;
    }
    let dead: Vec<String> = pool.exhausted.iter().map(|i| format!("#{}", i + 1)).collect();
    Some(format!(
        "⚠️ API key {} đã hết lượt hôm nay → đã tự chuyển sang key #{}.",
        dead.join(", "),
        pool.idx + 1
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_iso8601_durations() {
        assert_eq!(parse_iso8601_duration("PT1H2M3S"), Some(3_723));
        assert_eq!(parse_iso8601_duration("PT15M"), Some(900));
        assert_eq!(parse_iso8601_duration("PT45S"), Some(45));
        assert_eq!(parse_iso8601_duration("PT1H"), Some(3_600));
        assert_eq!(parse_iso8601_duration("P1DT2H"), Some(93_600));
        assert_eq!(parse_iso8601_duration("PT0S"), Some(0));
    }

    #[test]
    fn published_at_to_yyyymmdd() {
        assert_eq!(
            published_to_date("2024-01-15T10:30:00Z").as_deref(),
            Some("20240115")
        );
        assert_eq!(published_to_date("bad"), None);
    }

    #[test]
    fn extracts_hashtags_dedup_unicode() {
        let tags = extract_hashtags("Xin chào #Reup #shorts", "lặp lại #reup #tiếngviệt");
        assert_eq!(tags, vec!["#Reup", "#shorts", "#tiếngviệt"]);
    }

    #[test]
    fn hashtags_empty_when_none() {
        assert!(extract_hashtags("không có tag", "mô tả thường").is_empty());
    }

    #[test]
    fn urlencodes_spaces_and_unicode() {
        assert_eq!(urlencoding("a b"), "a%20b");
        assert_eq!(urlencoding("Reup-2024_x"), "Reup-2024_x");
    }

    #[test]
    fn keypool_advances_and_records_exhausted() {
        let mut pool = KeyPool::new(vec!["k1".into(), "k2".into(), "k3".into()]);
        assert_eq!(pool.current().unwrap(), "k1");
        pool.advance(); // k1 hết
        assert_eq!(pool.current().unwrap(), "k2");
        pool.advance(); // k2 hết
        assert_eq!(pool.current().unwrap(), "k3");
        assert_eq!(pool.exhausted, vec![0, 1]);
    }

    #[test]
    fn keypool_errors_when_all_exhausted() {
        let mut pool = KeyPool::new(vec!["only".into()]);
        pool.advance(); // key duy nhất hết → không còn key
        assert!(pool.current().is_err());
    }

    #[test]
    fn keypool_advance_idempotent_on_same_index() {
        // Gọi advance khi đã hết key không nhân đôi index trong `exhausted`.
        let mut pool = KeyPool::new(vec!["k1".into(), "k2".into()]);
        pool.advance();
        pool.advance(); // idx=2, ghi nhận index 1
        assert_eq!(pool.exhausted, vec![0, 1]);
    }

    #[test]
    fn api_note_none_when_no_failover() {
        let pool = KeyPool::new(vec!["k1".into()]);
        assert!(build_api_note(&pool).is_none());
    }

    #[test]
    fn api_note_reports_dead_key_and_current() {
        let mut pool = KeyPool::new(vec!["k1".into(), "k2".into()]);
        pool.advance(); // k1 hết → đang dùng k2 (idx 1)
        let note = build_api_note(&pool).unwrap();
        assert!(note.contains("#1"));
        assert!(note.contains("#2"));
    }

    #[test]
    fn quota_reason_detection() {
        let quota = serde_json::json!({"errors":[{"reason":"quotaExceeded"}]});
        let invalid = serde_json::json!({"errors":[{"reason":"keyInvalid"}]});
        assert!(is_quota_reason(&quota));
        assert!(!is_quota_reason(&invalid));
    }

    fn tvid(id: &str) -> ChannelVideo {
        ChannelVideo {
            url: format!("https://www.youtube.com/watch?v={id}"),
            title: id.into(),
            duration_sec: Some(10),
            view_count: Some(1),
            upload_date: Some("20240101".into()),
            thumbnail: None,
            is_photo: false,
            is_short: false,
            hashtags: vec![],
        }
    }

    #[test]
    fn merge_incremental_prepends_new_dedups_caps() {
        // new = [n1, n2] (mới nhất), cached = [n2(trùng), c1, c2]
        let new = vec![tvid("n1"), tvid("n2")];
        let cached = vec![tvid("n2"), tvid("c1"), tvid("c2")];
        let out = merge_incremental(new, cached, 100);
        let ids: Vec<&str> = out.iter().map(|v| v.title.as_str()).collect();
        assert_eq!(ids, vec!["n1", "n2", "c1", "c2"]); // bỏ trùng n2, mới lên đầu
    }

    #[test]
    fn merge_incremental_respects_cap() {
        let new = vec![tvid("n1")];
        let cached = vec![tvid("c1"), tvid("c2"), tvid("c3")];
        let out = merge_incremental(new, cached, 2);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].title, "n1");
        assert_eq!(out[1].title, "c1");
    }
}
