//! Lấy danh sách video của một kênh (UP) bilibili.com kèm ĐỦ metadata
//! (tên + ảnh + view + thời lượng + ngày) trong 1 lần gọi API — như BBDown/
//! yutto. Dùng chữ ký WBI (mixin key + MD5) mà bilibili.com web bắt buộc.
//!
//! Vì sao không dùng yt-dlp flat + dò từng video: flat trả ID trần (NA hết),
//! còn dò từng video thì bilibili risk-control chặn giữa chừng → mất tên loạn
//! xạ. API space/wbi/arc/search trả 30-50 video/lần kèm mọi field, ít request
//! hơn hẳn → ổn định hơn. Endpoint hay trả 412/-352 tạm thời → tự thử lại.
//!
//! Không cần proxy (bilibili.com không bị chặn ở VN), không cần đăng nhập.

use md5::{Digest, Md5};
use serde_json::Value;
use std::time::Duration;

use crate::models::{ChannelInfo, ChannelVideo};

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                  (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36";

/// Bảng hoán vị 64 phần tử để trộn img_key + sub_key thành mixin key (WBI).
const MIXIN_TAB: [usize; 64] = [
    46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42, 19, 29,
    28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40, 61, 26, 17, 0, 1, 60, 51, 30, 4, 22, 25,
    54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52,
];

/// mid (user id) từ URL space.bilibili.com/<mid>.
pub fn extract_mid(url: &str) -> Option<String> {
    let re = regex::Regex::new(r"space\.bilibili\.com/(\d+)").ok()?;
    Some(re.captures(url)?.get(1)?.as_str().to_string())
}

fn md5_hex(s: &str) -> String {
    let mut h = Md5::new();
    h.update(s.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Percent-encode giá trị query (RFC3986 unreserved giữ nguyên).
fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Ký WBI: sắp param theo key, nối query, w_rid = md5(query + mixin_key).
/// Trả về query string đã kèm wts + w_rid.
fn wbi_sign(mut params: Vec<(String, String)>, mixin_key: &str, wts: i64) -> String {
    params.push(("wts".into(), wts.to_string()));
    params.sort_by(|a, b| a.0.cmp(&b.0));
    let q: String = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, enc(v)))
        .collect::<Vec<_>>()
        .join("&");
    let w_rid = md5_hex(&format!("{q}{mixin_key}"));
    format!("{q}&w_rid={w_rid}")
}

struct WbiCtx {
    client: reqwest::Client,
    mixin_key: String,
    cookie: Option<String>,
}

/// Lấy bộ cookie ĐẦY ĐỦ (buvid3 + buvid4 qua spi API) + wbi keys → mixin key.
/// buvid4 giúp qua risk-control tốt hơn buvid3 đơn lẻ. Cache lại để tái dùng
/// cho mọi trang (không xin lại mỗi lần).
async fn init_ctx() -> Option<WbiCtx> {
    let bare = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(UA)
        .build()
        .ok()?;
    // spi API cấp cả buvid3 + buvid4 (đủ hơn scrape trang chủ chỉ có buvid3).
    let cookie = match bare
        .get("https://api.bilibili.com/x/frontend/finger/spi")
        .send()
        .await
    {
        Ok(resp) => match resp.json::<Value>().await {
            Ok(j) => {
                let b3 = j.pointer("/data/b_3").and_then(|v| v.as_str());
                let b4 = j.pointer("/data/b_4").and_then(|v| v.as_str());
                match (b3, b4) {
                    (Some(b3), Some(b4)) => Some(format!("buvid3={b3}; buvid4={b4}")),
                    (Some(b3), None) => Some(format!("buvid3={b3}")),
                    _ => None,
                }
            }
            Err(_) => None,
        },
        Err(_) => None,
    };

    // nav → wbi_img.img_url / sub_url → basename không đuôi.
    let mut req = bare.get("https://api.bilibili.com/x/web-interface/nav");
    if let Some(c) = &cookie {
        req = req.header("Cookie", c);
    }
    let nav: Value = req.send().await.ok()?.json().await.ok()?;
    let img = nav.pointer("/data/wbi_img/img_url")?.as_str()?;
    let sub = nav.pointer("/data/wbi_img/sub_url")?.as_str()?;
    let key_of = |u: &str| -> String {
        u.rsplit('/').next().unwrap_or("").split('.').next().unwrap_or("").to_string()
    };
    let orig: Vec<char> = format!("{}{}", key_of(img), key_of(sub)).chars().collect();
    let mixin_key: String = MIXIN_TAB.iter().filter_map(|&i| orig.get(i)).take(32).collect();
    if mixin_key.len() < 32 {
        return None;
    }

    Some(WbiCtx { client: bare, mixin_key, cookie })
}

/// "mm:ss" hoặc "hh:mm:ss" → giây.
fn parse_length(s: &str) -> Option<u64> {
    let parts: Vec<&str> = s.split(':').collect();
    let nums: Option<Vec<u64>> = parts.iter().map(|p| p.trim().parse::<u64>().ok()).collect();
    let nums = nums?;
    match nums.len() {
        2 => Some(nums[0] * 60 + nums[1]),
        3 => Some(nums[0] * 3600 + nums[1] * 60 + nums[2]),
        1 => Some(nums[0]),
        _ => None,
    }
}

/// Lấy 1 trang video của UP qua API WBI (đã ký). Tự thử lại tối đa
/// `MAX_RETRY` lần khi gặp 412/-352 (risk-control tạm thời). `wts` truyền vào
/// để test tất định; None = dùng thời gian thật.
async fn fetch_page(
    ctx: &WbiCtx,
    mid: &str,
    pn: u32,
    ps: u32,
    now_secs: i64,
) -> Option<(Vec<ChannelVideo>, bool)> {
    // Risk-control (-352/412) của Bilibili rất hay xảy ra ngẫu nhiên; server
    // tự nhả sau vài giây. Thử nhiều lần với nghỉ ngắn → tỉ lệ đậu cao.
    const MAX_RETRY: usize = 10;
    for attempt in 0..MAX_RETRY {
        // dm_img_* = tham số chống-crawler trình duyệt thật gửi kèm; giúp
        // giảm -352. Giá trị tĩnh hợp lệ là đủ (server không kiểm nội dung).
        let params = vec![
            ("mid".into(), mid.to_string()),
            ("pn".into(), pn.to_string()),
            ("ps".into(), ps.to_string()),
            ("order".into(), "pubdate".into()),
            ("platform".into(), "web".into()),
            ("web_location".into(), "1550101".into()),
            ("dm_img_list".into(), "[]".into()),
            ("dm_img_str".into(), "V2ViR0wgMS4wIChPcGVuR0wgRVMgMi4wIENocm9taXVtKQ".into()),
            ("dm_cover_img_str".into(), "QU5HTEUgKEludGVsLCBJbnRlbChSKSBVSEQgR3JhcGhpY3M".into()),
            ("dm_img_inter".into(), r#"{"ds":[],"wh":[0,0,0],"of":[0,0,0]}"#.into()),
        ];
        let qs = wbi_sign(params, &ctx.mixin_key, now_secs + attempt as i64);
        let url = format!("https://api.bilibili.com/x/space/wbi/arc/search?{qs}");
        let mut req = ctx
            .client
            .get(&url)
            .header("Referer", format!("https://space.bilibili.com/{mid}"))
            .header("Origin", "https://space.bilibili.com");
        if let Some(c) = &ctx.cookie {
            req = req.header("Cookie", c);
        }
        let body = match req.send().await {
            Ok(r) => r.text().await.unwrap_or_default(),
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(700)).await;
                continue;
            }
        };
        let json: Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(_) => {
                // 412 trả HTML challenge → không parse được → thử lại.
                tokio::time::sleep(Duration::from_millis(700)).await;
                continue;
            }
        };
        let code = json.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            // -352/-412/-799… risk-control tạm thời → nghỉ rồi thử lại.
            tokio::time::sleep(Duration::from_millis(700)).await;
            continue;
        }
        let vlist = json
            .pointer("/data/list/vlist")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let count = json.pointer("/data/page/count").and_then(|v| v.as_i64()).unwrap_or(0);

        let mut videos = Vec::new();
        for v in &vlist {
            let bvid = match v.get("bvid").and_then(|x| x.as_str()) {
                Some(s) if !s.is_empty() => s,
                _ => continue,
            };
            let title = v.get("title").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let mut pic = v.get("pic").and_then(|x| x.as_str()).unwrap_or("").to_string();
            if pic.starts_with("//") {
                pic = format!("https:{pic}");
            } else if pic.starts_with("http://") {
                pic = pic.replacen("http://", "https://", 1);
            }
            let view = v.get("play").and_then(|x| x.as_u64());
            let duration = v.get("length").and_then(|x| x.as_str()).and_then(parse_length);
            let upload_date = v.get("created").and_then(|x| x.as_i64()).map(unix_to_yyyymmdd);
            videos.push(ChannelVideo {
                url: format!("https://www.bilibili.com/video/{bvid}"),
                title,
                duration_sec: duration,
                view_count: view,
                upload_date,
                thumbnail: if pic.is_empty() { None } else { Some(pic) },
                is_photo: false,
                is_short: false,
                hashtags: Vec::new(),
            });
        }
        let got = (pn * ps) as i64;
        let has_more = got < count && !videos.is_empty();
        let _ = attempt;
        return Some((videos, has_more));
    }
    None
}

/// Lấy tên + avatar của UP qua card API (mở, không cần WBI). Trả (name, face_https).
async fn fetch_up_card(ctx: &WbiCtx, mid: &str) -> Option<(String, Option<String>)> {
    let url = format!("https://api.bilibili.com/x/web-interface/card?mid={mid}&photo=false");
    let mut req = ctx.client.get(&url).header("Referer", "https://www.bilibili.com/");
    if let Some(c) = &ctx.cookie {
        req = req.header("Cookie", c);
    }
    let json: Value = req.send().await.ok()?.json().await.ok()?;
    if json.get("code").and_then(|v| v.as_i64()) != Some(0) {
        return None;
    }
    let card = json.pointer("/data/card")?;
    let name = card.get("name").and_then(|v| v.as_str())?.to_string();
    let face = card.get("face").and_then(|v| v.as_str()).map(|f| {
        if let Some(rest) = f.strip_prefix("http://") {
            format!("https://{rest}")
        } else {
            f.to_string()
        }
    });
    Some((name, face))
}

/// UTC unix seconds → "YYYYMMDD" (giờ VN +7 để ngày khớp cảm nhận người dùng).
fn unix_to_yyyymmdd(ts: i64) -> String {
    // Thuật toán civil-from-days (Howard Hinnant), +7h cho giờ VN.
    let secs = ts + 7 * 3600;
    let days = secs.div_euclid(86400);
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}{:02}{:02}", y, m, d)
}

/// Lấy toàn bộ (tới `limit`, 0 = hết) video của UP bilibili.com.
/// Trả None nếu không phải URL space hợp lệ hoặc API hỏng hoàn toàn.
pub async fn fetch_space(
    url: &str,
    limit: u32,
    now_secs: i64,
) -> Option<(ChannelInfo, Vec<ChannelVideo>)> {
    let mid = extract_mid(url)?;
    let ctx = init_ctx().await?;
    const PS: u32 = 30;

    let mut all: Vec<ChannelVideo> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut pn = 1u32;
    loop {
        let (videos, has_more) = match fetch_page(&ctx, &mid, pn, PS, now_secs).await {
            Some(r) => r,
            None => {
                // Trang 1 fail hẳn (sau 10 lần) → trả None để fetch_channel
                // rơi xuống yt-dlp flat. Trang sau fail → giữ những gì đã lấy.
                if pn == 1 {
                    return None;
                }
                break;
            }
        };
        if videos.is_empty() {
            break;
        }
        for v in videos {
            if seen.insert(v.url.clone()) {
                all.push(v);
            }
        }
        if limit > 0 && all.len() as u32 >= limit {
            all.truncate(limit as usize);
            break;
        }
        if !has_more || pn >= 100 {
            break;
        }
        pn += 1;
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    if all.is_empty() {
        return None;
    }

    // Tên + avatar thật của UP (fallback về tên chung nếu card API lỗi).
    let (title, avatar) = match fetch_up_card(&ctx, &mid).await {
        Some((name, face)) => (name, face),
        None => (format!("Bilibili — {} video", all.len()), None),
    };

    let info = ChannelInfo {
        url: url.to_string(),
        title,
        thumbnail: avatar.or_else(|| all.first().and_then(|v| v.thumbnail.clone())),
        video_count: Some(all.len() as u32),
        extractor: "bilibili".into(),
        hidden_downloaded: None,
        channel_id: None, // field này dành cho RSS YouTube (UC…), không dùng cho bili
        api_note: None,
    };
    Some((info, all))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixin_key_len_and_sign_stable() {
        // Kiểm tra hoán vị + md5 cho ra chữ ký tất định.
        let params = vec![("mid".to_string(), "123".to_string()), ("pn".into(), "1".into())];
        let s1 = wbi_sign(params.clone(), "abcdefabcdefabcdefabcdefabcdef12", 1_700_000_000);
        let s2 = wbi_sign(params, "abcdefabcdefabcdefabcdefabcdef12", 1_700_000_000);
        assert_eq!(s1, s2);
        assert!(s1.contains("w_rid="));
        assert!(s1.contains("wts=1700000000"));
        // params phải được sắp xếp: mid trước pn trước wts.
        assert!(s1.find("mid=").unwrap() < s1.find("pn=").unwrap());
    }

    #[test]
    fn extract_mid_works() {
        assert_eq!(
            extract_mid("https://space.bilibili.com/546195/video").as_deref(),
            Some("546195")
        );
        assert_eq!(extract_mid("https://www.youtube.com/x"), None);
    }

    #[test]
    fn parse_length_formats() {
        assert_eq!(parse_length("7:43"), Some(463));
        assert_eq!(parse_length("1:02:03"), Some(3723));
        assert_eq!(parse_length("bad"), None);
    }

    #[test]
    fn unix_date_conversion() {
        // 2021-05-01 00:00:00 UTC = 1619827200 → giờ VN vẫn 20210501.
        assert_eq!(unix_to_yyyymmdd(1619827200), "20210501");
    }
}
