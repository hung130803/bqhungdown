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
use crate::models::{ChannelInfo, ChannelVideo};
use serde_json::Value;
use std::collections::HashSet;
use std::time::Duration;

const API_BASE: &str = "https://www.googleapis.com/youtube/v3";
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

/// Gọi 1 endpoint API, trả JSON đã parse. Bóc lỗi {error:{...}} thành AppError.
async fn api_get(client: &reqwest::Client, url: &str) -> AppResult<Value> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::YtDlpFailed(format!("Lỗi mạng khi gọi API: {e}")))?;
    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| {
        AppError::YtDlpFailed(format!("Không đọc được phản hồi API (HTTP {status}): {e}"))
    })?;
    if let Some(err) = body.get("error") {
        return Err(AppError::YtDlpFailed(friendly_api_error(err)));
    }
    if !status.is_success() {
        return Err(AppError::YtDlpFailed(format!(
            "YouTube API trả lỗi HTTP {}",
            status.as_u16()
        )));
    }
    Ok(body)
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
    let body = api_get(&client, &url).await?;
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
    channel_id: String,
    uploads_playlist: String,
    info: ChannelInfo,
}

/// Từ URL kênh → channel_id + uploads playlist + thông tin kênh.
/// Hỗ trợ /channel/UC..., /@handle, /user/name, /c/custom (custom dùng search).
async fn resolve_channel(
    client: &reqwest::Client,
    key: &str,
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
        let cid = search_channel_id(client, key, name).await?;
        format!("id={cid}")
    } else {
        return Err(AppError::YtDlpFailed(
            "Link kênh không nhận diện được (cần dạng /channel/, /@tên, /user/ hoặc /c/).".into(),
        ));
    };

    let url = format!(
        "{API_BASE}/channels?part=snippet,contentDetails,statistics&{lookup}&key={key}"
    );
    let body = api_get(client, &url).await?;
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
    };

    Ok(Resolved {
        channel_id,
        uploads_playlist,
        info,
    })
}

/// Resolve custom (/c/) URL → channelId qua search.list (tốn 100 quota).
async fn search_channel_id(
    client: &reqwest::Client,
    key: &str,
    query: &str,
) -> AppResult<String> {
    let q = urlencoding(query);
    let url =
        format!("{API_BASE}/search?part=snippet&type=channel&maxResults=1&q={q}&key={key}");
    let body = api_get(client, &url).await?;
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
    key: &str,
    uploads_playlist: &str,
    cap: u32,
) -> AppResult<Vec<String>> {
    let mut ids: Vec<String> = Vec::new();
    let mut page_token: Option<String> = None;
    loop {
        let token_param = page_token
            .as_deref()
            .map(|t| format!("&pageToken={t}"))
            .unwrap_or_default();
        let url = format!(
            "{API_BASE}/playlistItems?part=contentDetails&maxResults=50&playlistId={uploads_playlist}{token_param}&key={key}"
        );
        let body = api_get(client, &url).await?;
        if let Some(items) = body.get("items").and_then(|i| i.as_array()) {
            for it in items {
                if let Some(id) = it
                    .pointer("/contentDetails/videoId")
                    .and_then(|v| v.as_str())
                {
                    ids.push(id.to_string());
                }
            }
        }
        if ids.len() as u32 >= cap {
            ids.truncate(cap as usize);
            break;
        }
        match body.get("nextPageToken").and_then(|v| v.as_str()) {
            Some(t) => page_token = Some(t.to_string()),
            None => break,
        }
    }
    Ok(ids)
}

/// Lấy metadata chi tiết cho danh sách id (chia lô 50), trả map id → video.
async fn fetch_video_details(
    client: &reqwest::Client,
    key: &str,
    ids: &[String],
) -> AppResult<std::collections::HashMap<String, ChannelVideo>> {
    let mut map = std::collections::HashMap::new();
    for chunk in ids.chunks(50) {
        let id_param = chunk.join(",");
        let url = format!(
            "{API_BASE}/videos?part=snippet,statistics,contentDetails&id={id_param}&key={key}"
        );
        let body = api_get(client, &url).await?;
        let items = match body.get("items").and_then(|i| i.as_array()) {
            Some(a) => a,
            None => continue,
        };
        for it in items {
            let id = match it.get("id").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
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
            // YouTube Data API không gắn cờ Shorts; dùng heuristic thời lượng
            // ngắn (≤ 60s) như phần còn lại của app.
            let is_short = duration_sec.map(|d| d <= 60).unwrap_or(false);

            map.insert(
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
            );
        }
    }
    Ok(map)
}

/// Lấy danh sách + metadata chính xác của cả kênh. `limit = 0` → tất cả (đến
/// HARD_CAP). Giữ thứ tự mới-nhất-trước từ playlist uploads.
pub async fn fetch_channel(
    url: &str,
    key: &str,
    limit: u32,
) -> AppResult<(ChannelInfo, Vec<ChannelVideo>)> {
    let key = key.trim();
    if key.is_empty() {
        return Err(AppError::YtDlpFailed("Chưa nhập YouTube API key.".into()));
    }
    let client = build_client()?;

    let resolved = resolve_channel(&client, key, url).await?;
    let cap = if limit == 0 { HARD_CAP } else { limit.min(HARD_CAP) };
    let ids = list_upload_ids(&client, key, &resolved.uploads_playlist, cap).await?;
    if ids.is_empty() {
        return Ok((resolved.info, Vec::new()));
    }
    let mut details = fetch_video_details(&client, key, &ids).await?;

    // Giữ đúng thứ tự id (mới nhất trước) khi videos.list trả về xáo trộn.
    let mut videos: Vec<ChannelVideo> = Vec::with_capacity(ids.len());
    for id in &ids {
        if let Some(v) = details.remove(id) {
            videos.push(v);
        }
    }

    let _ = resolved.channel_id; // đã nhét vào info.channel_id
    Ok((resolved.info, videos))
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
}
