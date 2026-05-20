//! Site-specific URL pre-resolution.
//!
//! Một số nền tảng cần xử lý URL trước khi đẩy vào yt-dlp:
//!
//! ## ViralHog
//! Trang `viralhog.com/watch/file/<id>` chỉ chứa iframe nhúng video; trang
//! embed `viralhog.com/e/<id>` mới có `<video><source>` thật. Module này
//! tải HTML trang gốc, regex trích URL iframe → trả URL embed.
//!
//! ## Douyin
//! yt-dlp Douyin extractor liên tục báo `Fresh cookies needed` cho user ngoài
//! Trung Quốc, kể cả khi đã cung cấp cookies hợp lệ — đó là quirk của Douyin
//! anti-bot, không phải bug ta sửa được.
//!
//! Cách lách: gọi **TikWM** (`tikwm.com/api/`) — public scraping API miễn phí
//! mà gần như mọi web downloader Douyin/TikTok đang dùng. TikWM resolve dùm
//! anti-bot và trả về URL MP4 trực tiếp; ta đẩy URL đó vào yt-dlp generic
//! extractor để tải file. Không cần cookies, không cần VPN.
//!
//! **Privacy implication**: URL Douyin được gửi qua tikwm.com khi resolve.
//! TikWM thấy URL ta đang tải (không biết IP của user nếu user dùng VPN).
//! Đối với Douyin có alternative nào khác không cần proxy bên thứ 3 (kiểu
//! như VPN TQ + cookies fresh) thì user vẫn có thể bypass module này bằng
//! cách paste link `iesdouyin.com/share/video/...` thẳng — module chỉ
//! resolve khi nhận diện hostname douyin.

use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;
use std::time::Duration;

const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                  (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";

/// Resolve `url` to a yt-dlp-friendly URL when needed. Returns input unchanged
/// when no rule applies. Network failures gracefully fall back to input so we
/// don't break URLs of unrelated sites.
pub async fn resolve(url: &str) -> String {
    resolve_with_meta(url).await.0
}

/// Returns `(resolved_url, optional_meta)`. The `meta` carries title /
/// thumbnail / channel scraped from the share page so the queue item shows
/// human-friendly text instead of the CDN file ID.
pub async fn resolve_with_meta(url: &str) -> (String, Option<DouyinMeta>) {
    let lower = url.to_lowercase();

    // ── Douyin: tries 3 strategies in order:
    //   1. tikwm.com proxy → direct MP4 URL (works for trending content)
    //   2. iesdouyin share page → scrape play_addr.url_list + title (no cookies)
    //   3. iesdouyin.com share URL → yt-dlp IesDouyin extractor (last resort)
    if lower.contains("douyin.com") || lower.contains("iesdouyin.com") {
        if let Some(id) = extract_douyin_id(url) {
            let canonical = format!("https://www.douyin.com/video/{id}");
            if let Some((direct, meta)) = resolve_via_tikwm(&canonical).await {
                return (direct, Some(meta));
            }
            if let Some((direct, meta)) = resolve_via_share_page(&id).await {
                return (direct, Some(meta));
            }
            return (format!("https://www.iesdouyin.com/share/video/{id}/"), None);
        }
    }

    // ── ViralHog: /watch/file/<id> → /e/<embed_id>
    if lower.contains("viralhog.com/watch/") {
        if let Some(embed) = resolve_viralhog(url).await {
            return (embed, None);
        }
    }

    (url.to_string(), None)
}

/// Metadata scraped along with the resolved URL.
#[derive(Debug, Clone, Default)]
pub struct DouyinMeta {
    pub title: Option<String>,
    pub thumbnail: Option<String>,
    pub channel: Option<String>,
}

/// Extract the numeric video ID from any Douyin URL form.
/// Matches `?modal_id=<id>`, `/video/<id>`, `/share/video/<id>`.
fn extract_douyin_id(url: &str) -> Option<String> {
    static RE_MODAL: OnceLock<Regex> = OnceLock::new();
    static RE_VIDEO: OnceLock<Regex> = OnceLock::new();

    let re_modal = RE_MODAL.get_or_init(|| {
        Regex::new(r#"(?i)[?&]modal_id=(\d+)"#).expect("douyin modal_id regex")
    });
    let re_video = RE_VIDEO.get_or_init(|| {
        Regex::new(r#"(?i)/video/(\d+)|/share/video/(\d+)"#).expect("douyin /video/ regex")
    });

    if let Some(c) = re_modal.captures(url) {
        return c.get(1).map(|m| m.as_str().to_string());
    }
    if let Some(c) = re_video.captures(url) {
        return c.get(1).or_else(|| c.get(2)).map(|m| m.as_str().to_string());
    }
    None
}

/// Query TikWM scraping API for the direct MP4 URL. Returns `None` on any
/// failure (timeout, network, non-zero `code`, missing data) so the caller
/// can fall back to a different strategy.
///
/// API contract:
/// ```
/// GET https://www.tikwm.com/api/?url=<douyin_url>&hd=1
/// → { "code": 0, "data": { "hdplay": "...mp4", "play": "...", "wmplay": "..." } }
/// ```
async fn resolve_via_tikwm(douyin_url: &str) -> Option<(String, DouyinMeta)> {
    let client = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent(UA)
        .build()
        .ok()?;

    let resp = client
        .get("https://www.tikwm.com/api/")
        .query(&[("url", douyin_url), ("hd", "1")])
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
    // Prefer no-watermark variants first; fall through to watermarked as last resort.
    let direct = data
        .get("hdplay")
        .and_then(|v| v.as_str())
        .or_else(|| data.get("play").and_then(|v| v.as_str()))
        .or_else(|| data.get("wmplay").and_then(|v| v.as_str()))
        .map(String::from)?;

    let meta = DouyinMeta {
        title: data.get("title").and_then(|v| v.as_str()).map(String::from),
        thumbnail: data.get("cover").and_then(|v| v.as_str()).map(String::from)
            .or_else(|| data.get("origin_cover").and_then(|v| v.as_str()).map(String::from)),
        channel: data
            .get("author")
            .and_then(|a| a.get("nickname"))
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| data.get("author").and_then(|a| a.get("unique_id")).and_then(|v| v.as_str()).map(String::from)),
    };
    Some((direct, meta))
}

async fn resolve_viralhog(url: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r#"(?i)<iframe[^>]+src=["'](https?://(?:www\.)?viralhog\.com/e/[A-Za-z0-9_-]+)["']"#,
        )
        .expect("viralhog iframe regex")
    });

    let html = http_get(url).await?;
    let caps = re.captures(&html)?;
    Some(caps.get(1)?.as_str().to_string())
}

/// Scrape `iesdouyin.com/share/video/<id>/` HTML for the embedded video URL.
/// The page renders `_SSR_DATA` JSON inline that contains `video.play_addr.url_list`
/// with one or more `aweme.snssdk.com/aweme/v1/playwm/?...` direct video URLs.
/// We unescape the `\u002F` slashes in the JSON, regex out the first URL, and
/// strip `playwm` (watermarked) → `play` for the no-watermark variant.
async fn resolve_via_share_page(id: &str) -> Option<(String, DouyinMeta)> {
    static RE_URL: OnceLock<Regex> = OnceLock::new();
    static RE_TITLE: OnceLock<Regex> = OnceLock::new();
    static RE_COVER: OnceLock<Regex> = OnceLock::new();
    static RE_AUTHOR: OnceLock<Regex> = OnceLock::new();

    let re_url = RE_URL.get_or_init(|| {
        Regex::new(r#""play_addr"[^}]*?"url_list"\s*:\s*\[\s*"([^"]+)""#)
            .expect("share page url regex")
    });
    let re_title = RE_TITLE.get_or_init(|| {
        Regex::new(r#""desc"\s*:\s*"((?:[^"\\]|\\.)*)""#).expect("share page title regex")
    });
    let re_cover = RE_COVER.get_or_init(|| {
        Regex::new(r#""cover"[^}]*?"url_list"\s*:\s*\[\s*"([^"]+)""#)
            .expect("share page cover regex")
    });
    let re_author = RE_AUTHOR.get_or_init(|| {
        Regex::new(r#""nickname"\s*:\s*"((?:[^"\\]|\\.)*)""#)
            .expect("share page author regex")
    });

    let url = format!("https://www.iesdouyin.com/share/video/{id}/");
    let html = http_get_with_ua(
        &url,
        "Mozilla/5.0 (iPhone; CPU iPhone OS 16_0 like Mac OS X) AppleWebKit/605.1.15 \
         (KHTML, like Gecko) Mobile/15E148",
    )
    .await?;

    let raw_url = re_url.captures(&html)?.get(1)?.as_str();
    let direct = decode_json_string(raw_url).replace("/playwm/", "/play/");

    let title = re_title
        .captures(&html)
        .and_then(|c| c.get(1))
        .map(|m| decode_json_string(m.as_str()))
        .filter(|s| !s.trim().is_empty());
    let thumbnail = re_cover
        .captures(&html)
        .and_then(|c| c.get(1))
        .map(|m| decode_json_string(m.as_str()));
    let channel = re_author
        .captures(&html)
        .and_then(|c| c.get(1))
        .map(|m| decode_json_string(m.as_str()))
        .filter(|s| !s.trim().is_empty());

    Some((direct, DouyinMeta { title, thumbnail, channel }))
}

/// Decode JSON-escaped slashes/unicode that show up in the HTML (`\u002F`,
/// `\/`, basic `\"`, `\\`). Good enough for short text fields like title /
/// nickname / URL strings.
fn decode_json_string(raw: &str) -> String {
    let mut s = raw.replace("\\u002F", "/").replace("\\/", "/");
    s = s.replace("\\\"", "\"").replace("\\\\", "\\");
    // Decode common \uXXXX (covers Vietnamese diacritics + emoji BMP).
    static RE_U: OnceLock<Regex> = OnceLock::new();
    let re = RE_U.get_or_init(|| Regex::new(r#"\\u([0-9a-fA-F]{4})"#).unwrap());
    re.replace_all(&s, |c: &regex::Captures| {
        let n = u32::from_str_radix(&c[1], 16).unwrap_or(0);
        char::from_u32(n).map(|c| c.to_string()).unwrap_or_default()
    })
    .into_owned()
}

async fn http_get_with_ua(url: &str, ua: &str) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent(ua)
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .ok()?;
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.text().await.ok()
}

async fn http_get(url: &str) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent(UA)
        .build()
        .ok()?;
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.text().await.ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iframe_regex_matches_real_html() {
        let html = r#"<div class="rounded-2xl w-full flex flex-col">
                            <iframe width="100%" height="600" src="https://viralhog.com/e/rv8ue3qu23" frameborder="0" allowfullscreen></iframe>"#;
        let re = Regex::new(r#"(?i)<iframe[^>]+src=["'](https?://(?:www\.)?viralhog\.com/e/[A-Za-z0-9_-]+)["']"#).unwrap();
        let caps = re.captures(html).unwrap();
        assert_eq!(caps.get(1).unwrap().as_str(), "https://viralhog.com/e/rv8ue3qu23");
    }

    #[test]
    fn extract_id_from_modal() {
        assert_eq!(
            extract_douyin_id("https://www.douyin.com/jingxuan?modal_id=7641727778538196250").as_deref(),
            Some("7641727778538196250")
        );
    }

    #[test]
    fn extract_id_from_video_path() {
        assert_eq!(
            extract_douyin_id("https://www.douyin.com/video/12345").as_deref(),
            Some("12345")
        );
    }

    #[test]
    fn extract_id_from_share_path() {
        assert_eq!(
            extract_douyin_id("https://www.iesdouyin.com/share/video/99/").as_deref(),
            Some("99")
        );
    }

    #[test]
    fn extract_id_returns_none_for_homepage() {
        assert!(extract_douyin_id("https://www.douyin.com/").is_none());
        assert!(extract_douyin_id("https://www.douyin.com/search").is_none());
    }
}
