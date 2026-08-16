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

/// Bóc một trường DANH SÁCH mà API có thể trả `null` thay vì `[]`.
///
/// `#[serde(default)]` CHỈ cứu trường THIẾU HẲN. Trường CÓ MẶT nhưng giá trị
/// `null` vẫn nổ `invalid type: null, expected a sequence` — đây là chỗ ai cũng
/// tưởng đã an toàn vì "đã có serde(default) rồi".
///
/// Douyin trả `"images": null` cho MỌI video thường (chỉ bài ảnh mới có mảng),
/// nên chỉ cần kênh có ĐÚNG 1 video thường là gãy CẢ lượt quét kênh — anh Hùng
/// gặp 14/08/2026: "Dữ liệu Douyin không hợp lệ: invalid type: null, expected a
/// sequence at line 1 column 89208". Chú ý cột 89208: dữ liệu ĐÃ về đủ 89 KB,
/// mạng/chữ ký/cookie đều tốt, chỉ chết đúng lúc bóc JSON.
///
/// DÙNG CHO MỌI `Vec` bóc từ JSON của Douyin/TikWM, đừng chỉ vá `images` —
/// bất kỳ trường danh sách nào cũng có thể về `null` khi bài không có loại
/// nội dung đó.
fn null_to_vec<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}

#[derive(Debug, Deserialize)]
struct TikwmData {
    #[serde(default, deserialize_with = "null_to_vec")]
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
    #[serde(default, deserialize_with = "null_to_vec")]
    images: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize, Default)]
struct TikwmVideo {
    #[serde(default)]
    cover: Option<TikwmCover>,
}

#[derive(Debug, Deserialize, Default)]
struct TikwmCover {
    #[serde(default, deserialize_with = "null_to_vec")]
    url_list: Vec<String>,
}

// ─────────────────────────────────────────────────────────────────────────
//  API TRỰC TIẾP douyin.com (tự ký a_bogus) — đường CHÍNH, không cần dịch vụ
//  trung gian. tikwm ở dưới chỉ còn là phao dự phòng.
// ─────────────────────────────────────────────────────────────────────────

use crate::douyin_sign::{ABogus, DOUYIN_UA};
use crate::models::Settings;

// ─────────────────────────────────────────────────────────────────────────
//  COOKIE ĐĂNG NHẬP — thứ quyết định lấy được 20 hay 250 video
// ─────────────────────────────────────────────────────────────────────────

/// Bóc cookie của một domain từ file Netscape (đúng định dạng `--cookies` của
/// yt-dlp) thành chuỗi header `ten1=gt1; ten2=gt2`.
///
/// VÌ SAO PHẢI CÓ: Douyin chỉ cho KHÁCH VÃNG LAI xem TRANG ĐẦU (~20 video) rồi
/// phán `has_more=0` — trông y như "kênh chỉ có 20 video". Đo thật 16/08/2026
/// trên đúng kênh anh Hùng gửi:
///   · chỉ ttwid ẩn danh (app 0.1.139 làm thế) → 21 video / 2 trang
///   · cookie đăng nhập của anh Hùng          → 250 video / 14 trang
/// Gấp 11,9 lần. Trước bản này `fetch_channel_api` KHÔNG hề đọc
/// `settings.cookies_file`, nên dù anh Hùng đã nạp cookie Douyin đầy đủ
/// (63 dòng, có sessionid/sid_guard/passport_*) app vẫn đi như khách.
///
/// CHỈ ĐỌC, KHÔNG BAO GIỜ GHI: chính file này cũng được đưa cho yt-dlp qua
/// `--cookies`, mà yt-dlp lúc thoát thì GHI ĐÈ file đó. Ở đây ta chỉ đọc nên
/// an toàn — đừng bao giờ đưa đường dẫn này cho tiến trình con.
pub fn netscape_to_cookie_header(raw: &str, domain_filter: &str) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for line in raw.lines() {
        // yt-dlp/Chrome ghi cookie HttpOnly với tiền tố này.
        let line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 7 || !f[0].contains(domain_filter) {
            continue;
        }
        let (name, value) = (f[5].trim(), f[6].trim());
        if name.is_empty() || value.is_empty() {
            continue;
        }
        // Cookie trùng tên giữa `.douyin.com` và `www.douyin.com` — giữ cái đầu.
        if seen.insert(name.to_string()) {
            parts.push(format!("{name}={value}"));
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("; "))
    }
}

/// True khi chuỗi cookie có dấu hiệu ĐÃ ĐĂNG NHẬP (không chỉ cookie khách).
fn cookie_has_login(header: &str) -> bool {
    ["sessionid=", "sessionid_ss=", "sid_tt=", "sid_guard="]
        .iter()
        .any(|k| header.contains(k))
}

/// Lấy cookie Douyin từ cài đặt của user (file Netscape). Trả None nếu user
/// chưa cấu hình file cookie hoặc file không có dòng nào cho douyin.com.
fn douyin_cookie_header(settings: &Settings) -> Option<String> {
    let path = settings.cookies_file.as_deref()?;
    if path.is_empty() {
        return None;
    }
    let raw = std::fs::read_to_string(path).ok()?;
    netscape_to_cookie_header(&raw, "douyin.com")
}

// ─────────────────────────────────────────────────────────────────────────
//  PHÂN LOẠI LỖI — mỗi loại một cách xử khác nhau, nói đúng cho user
// ─────────────────────────────────────────────────────────────────────────

/// Vì sao một lượt gọi API Douyin hỏng. Tách riêng để thông báo cho user nói
/// ĐÚNG việc phải làm, thay vì đổ hết vào "Dữ liệu Douyin không hợp lệ".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DouyinFail {
    /// Douyin CHẶN theo tần suất. Đo thật: sau ~25 request liên tiếp không
    /// nghỉ, Douyin trả HTTP 403 + thân `Blocked by ArgusSecurityPlugin
    /// Validate Error` (text/plain, 45 byte). Thân này KHÔNG phải JSON nên
    /// bản cũ nổ đúng câu anh Hùng thấy: "expected value at line 1 column 1".
    Blocked,
    /// Thân RỖNG kèm HTTP 200 — Douyin từ chối im lặng. Đo thật: khi không có
    /// ttwid thì MỌI request đều rơi vào đây.
    EmptyBody,
    /// Douyin trả JSON nhưng `status_code != 0`.
    ApiCode(i64),
    /// Trả về thứ không phải JSON mà cũng không phải trang chặn đã biết —
    /// nhiều khả năng Douyin đổi API hoặc bắt giải xác minh.
    NotJson { status: u16, ctype: String, head: String },
    /// Lỗi mạng/timeout.
    Network(String),
}

impl DouyinFail {
    /// Câu tiếng Việt nói THẲNG chuyện gì xảy ra và phải làm gì.
    pub fn message(&self, co_cookie: bool) -> String {
        let goi_y_cookie = if co_cookie {
            "Cookie Douyin của bạn có thể đã hết hạn — đăng nhập lại douyin.com \
             rồi xuất cookie mới vào Cài đặt."
        } else {
            "Bạn CHƯA nạp cookie Douyin. Vào Cài đặt → Cookie, chọn file cookie \
             đã đăng nhập douyin.com — không có cookie thì Douyin chặn rất nhanh."
        };
        match self {
            DouyinFail::Blocked => format!(
                "Douyin CHẶN TẠM THỜI vì hỏi quá nhanh (máy chủ trả 403 \
                 \"Blocked by ArgusSecurityPlugin\").\n\
                 Đây KHÔNG phải lỗi link kênh — link của bạn vẫn đúng.\n\
                 Cách xử: chờ 5-10 phút rồi lấy lại; đừng bấm \"Lấy danh sách\" \
                 liên tiếp; lấy từng kênh một thay vì nhiều kênh cùng lúc.\n{goi_y_cookie}"
            ),
            DouyinFail::EmptyBody => format!(
                "Douyin nhận request nhưng trả về RỖNG — nghĩa là nó từ chối \
                 phục vụ (thường do thiếu cookie/ttwid, hoặc IP đang bị hạn chế).\n\
                 Link kênh của bạn không sai.\n{goi_y_cookie}"
            ),
            DouyinFail::ApiCode(c) => match c {
                // 2154/2156: cần xác minh; 8: phiên hỏng.
                2154 | 2156 => format!(
                    "Douyin ĐÒI XÁC MINH (mã {c}). Mở douyin.com trên trình duyệt, \
                     làm bước xác minh/đăng nhập lại, rồi xuất cookie mới vào Cài đặt."
                ),
                8 => format!(
                    "Phiên đăng nhập Douyin không còn hiệu lực (mã {c}). \
                     Đăng nhập lại douyin.com rồi xuất cookie mới."
                ),
                _ => format!(
                    "Douyin trả mã lỗi {c}. Kênh có thể riêng tư/đã xoá, hoặc cần \
                     đăng nhập.\n{goi_y_cookie}"
                ),
            },
            DouyinFail::NotJson { status, ctype, head } => format!(
                "Douyin trả về thứ KHÔNG phải dữ liệu video (HTTP {status}, kiểu {ctype}).\n\
                 Nhiều khả năng Douyin đã đổi API hoặc đang bắt giải xác minh.\n\
                 Nội dung nhận được: {head}\n{goi_y_cookie}"
            ),
            DouyinFail::Network(e) => format!(
                "Không kết nối được tới Douyin: {e}\n\
                 Kiểm tra mạng, hoặc Douyin đang chặn IP của bạn."
            ),
        }
    }
}

/// Đọc phản hồi THÔ (đã biết status + content-type + thân) và phán đúng loại.
/// Hàm THUẦN để test được không cần mạng.
pub fn classify_body(status: u16, ctype: &str, body: &str) -> Result<serde_json::Value, DouyinFail> {
    if body.trim().is_empty() {
        return Err(DouyinFail::EmptyBody);
    }
    // Trang chặn của Douyin: 403 + text/plain "Blocked by ArgusSecurityPlugin".
    if status == 403 || body.contains("Blocked by ArgusSecurityPlugin") {
        return Err(DouyinFail::Blocked);
    }
    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(v) => Ok(v),
        Err(_) => {
            let head: String = body.chars().take(200).collect();
            Err(DouyinFail::NotJson {
                status,
                ctype: ctype.to_string(),
                head: head.replace(['\n', '\r'], " "),
            })
        }
    }
}

#[derive(Debug, Deserialize)]
struct PostResp {
    #[serde(default)]
    status_code: i32,
    #[serde(default)]
    max_cursor: i64,
    #[serde(default)]
    has_more: i64,
    #[serde(default, deserialize_with = "null_to_vec")]
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

/// Kết quả một lượt bò kênh: video lấy được + vì sao dừng.
pub struct CrawlOutcome {
    pub posts: Vec<DouyinPost>,
    /// Lỗi làm lượt bò dừng giữa chừng (None = dừng tự nhiên vì hết video).
    pub stopped_by: Option<DouyinFail>,
    /// True khi Douyin CẮT NGAY sau trang đầu (dấu hiệu kinh điển của "đi như
    /// khách vãng lai"): đủ một trang rồi has_more=0.
    pub cut_after_first_page: bool,
    pub pages: usize,
}

/// Gọi 1 trang, tự thử lại khi bị chặn (403). Backoff 3s → 6s → 12s.
///
/// VÌ SAO PHẢI THỬ LẠI: 403 "Blocked by ArgusSecurityPlugin" là chặn TẠM THỜI
/// theo tần suất, không phải hỏng vĩnh viễn. Bản cũ gặp 403 là hỏng cả lượt và
/// đổ lỗi cho dữ liệu, làm anh Hùng đi kiểm tra link (vốn đúng).
async fn fetch_page_retry(
    client: &reqwest::Client,
    url: &str,
    referer: &str,
    cookie: Option<&str>,
) -> Result<serde_json::Value, DouyinFail> {
    let mut last = DouyinFail::Network("chưa gọi".into());
    for attempt in 0..3u32 {
        if attempt > 0 {
            // 3s, 6s — cho Douyin nguôi. Cố tình chậm: nhanh nữa là bị chặn tiếp.
            tokio::time::sleep(Duration::from_secs(3 * (1 << (attempt - 1)))).await;
        }
        let mut req = client.get(url).header("Referer", referer);
        if let Some(c) = cookie {
            req = req.header("Cookie", c);
        }
        match req.send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let ctype = resp
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("?")
                    .to_string();
                let body = resp.text().await.unwrap_or_default();
                match classify_body(status, &ctype, &body) {
                    Ok(v) => return Ok(v),
                    // Chỉ 2 loại này đáng thử lại; còn lại thử lại cũng vô ích.
                    Err(e @ (DouyinFail::Blocked | DouyinFail::EmptyBody)) => last = e,
                    Err(e) => return Err(e),
                }
            }
            Err(e) => last = DouyinFail::Network(e.to_string()),
        }
    }
    Err(last)
}

/// Lấy TOÀN BỘ video của kênh qua API douyin.com, phân trang bằng max_cursor.
/// Tự ký a_bogus mỗi lần gọi. Emit tiến độ để UI cập nhật số video.
///
/// KHÁC BẢN CŨ: gửi cookie đăng nhập của user (nếu có) — đây là thứ quyết định
/// lấy được ~20 hay HÀNG TRĂM video; và ĐỌC HTTP status thay vì nhét thẳng
/// trang chặn 403 vào serde_json.
async fn fetch_channel_api(
    app: &tauri::AppHandle,
    sec_uid: &str,
    proxy: &Option<String>,
    cookie: Option<&str>,
) -> CrawlOutcome {
    let client = match with_proxy(
        reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .user_agent(DOUYIN_UA),
        proxy,
    )
    .build()
    {
        Ok(c) => c,
        Err(e) => {
            return CrawlOutcome {
                posts: Vec::new(),
                stopped_by: Some(DouyinFail::Network(e.to_string())),
                cut_after_first_page: false,
                pages: 0,
            }
        }
    };

    // Cookie user đã có ttwid riêng rồi; chỉ xin ttwid ẩn danh khi user chưa
    // nạp cookie (thiếu ttwid thì MỌI request trả thân rỗng — đã đo).
    let cookie_owned: Option<String> = match cookie {
        Some(c) if cookie_has_login(c) => Some(c.to_string()),
        Some(c) => match fetch_ttwid(proxy).await {
            Some(tt) if !c.contains("ttwid=") => Some(format!("{c}; ttwid={tt}")),
            _ => Some(c.to_string()),
        },
        None => fetch_ttwid(proxy).await.map(|tt| format!("ttwid={tt}")),
    };

    let ab = ABogus::new();
    let referer = format!("https://www.douyin.com/user/{sec_uid}");

    let mut all: Vec<DouyinPost> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut cursor: i64 = 0;
    let mut stopped_by: Option<DouyinFail> = None;
    let mut pages = 0usize;
    // 200 × 18 ≈ 3600 video. Kênh to hơn thì lấy được 3600 mới nhất.
    const MAX_PAGES: usize = 200;

    for page in 0..MAX_PAGES {
        let params = build_post_params(sec_uid, cursor);
        let a_bogus = ab.get_value(&params, "GET");
        let url = format!(
            "https://www.douyin.com/aweme/v1/web/aweme/post/?{params}&a_bogus={}",
            pct_encode(&a_bogus)
        );

        let v = match fetch_page_retry(&client, &url, &referer, cookie_owned.as_deref()).await {
            Ok(v) => v,
            Err(e) => {
                stopped_by = Some(e);
                break;
            }
        };
        pages = page + 1;

        let json: PostResp = match serde_json::from_value(v) {
            Ok(j) => j,
            Err(e) => {
                // JSON hợp lệ nhưng hình dạng lạ -> Douyin đổi API.
                stopped_by = Some(DouyinFail::NotJson {
                    status: 200,
                    ctype: "application/json".into(),
                    head: format!("hình dạng lạ: {e}"),
                });
                break;
            }
        };
        if json.status_code != 0 {
            stopped_by = Some(DouyinFail::ApiCode(json.status_code.into()));
            break;
        }

        let got = json.aweme_list.len();
        for p in awemes_to_posts(json.aweme_list) {
            if seen.insert(p.id.clone()) {
                all.push(p);
            }
        }

        let _ = app.emit(
            "bqd-douyin-scraper-progress",
            serde_json::json!({ "count": all.len() }),
        );

        let _ = got;
        let het = json.has_more != 1 || json.max_cursor == 0 || json.max_cursor == cursor;
        if het {
            break;
        }
        cursor = json.max_cursor;
        // Nghỉ nhẹ giữa các trang cho đỡ bị rate-limit.
        tokio::time::sleep(Duration::from_millis(400)).await;
    }

    // Dấu hiệu "bị đối xử như khách vãng lai": Douyin bảo hết video sau 1-2
    // trang. ĐO THẬT trên kênh anh Hùng gửi — không cookie: dừng ở trang 2 với
    // 21 video; có cookie đăng nhập: đi 14 trang, 250 video. Nên `pages <= 2`
    // + dừng "tự nhiên" là cờ đáng ngờ (caller chỉ cảnh báo khi user CHƯA có
    // cookie đăng nhập, để kênh thật sự ít video không bị báo nhầm).
    let cut_after_first_page = stopped_by.is_none() && pages <= 2 && !all.is_empty();

    CrawlOutcome {
        posts: all,
        stopped_by,
        cut_after_first_page,
        pages,
    }
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

    // Cookie đăng nhập của user — THỨ QUYẾT ĐỊNH lấy được ~20 hay hàng trăm
    // video. Bản trước không hề đọc nó (xem `netscape_to_cookie_header`).
    let cookie = douyin_cookie_header(&settings.get());
    let co_dang_nhap = cookie.as_deref().map(cookie_has_login).unwrap_or(false);
    eprintln!(
        "[douyin] cookie: {} (đăng nhập: {co_dang_nhap})",
        match &cookie {
            Some(c) => format!("{} cookie douyin.com", c.split("; ").count()),
            None => "KHÔNG có".into(),
        }
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
        let out = fetch_channel_api(&app, &sec_uid, px, cookie.as_deref()).await;
        if !out.posts.is_empty() {
            let _ = app.emit(
                "bqd-douyin-scraper-progress",
                serde_json::json!({ "count": out.posts.len() }),
            );
            // Nói THẲNG khi kết quả có mùi thiếu, thay vì im lặng trả 20 video
            // rồi để anh Hùng tự đoán.
            let mut note = String::new();
            if let Some(f) = &out.stopped_by {
                note = format!(
                    "Mới lấy được {} video thì Douyin dừng lượt quét.\n{}",
                    out.posts.len(),
                    f.message(co_dang_nhap)
                );
            } else if out.cut_after_first_page && !co_dang_nhap {
                note = format!(
                    "CHỈ lấy được {} video — Douyin báo hết ngay sau trang đầu. \
                     Đây là cách Douyin đối xử với KHÁCH VÃNG LAI: chưa đăng nhập \
                     thì nó chỉ cho xem khoảng 20 video mới nhất.\n\
                     Muốn lấy ĐỦ cả kênh: vào Cài đặt → Cookie, nạp file cookie đã \
                     đăng nhập douyin.com (đo thật trên kênh này: không cookie 21 \
                     video, có cookie 250 video).",
                    out.posts.len()
                );
            }
            if !note.is_empty() {
                let _ = app.emit(
                    "bqd-douyin-scraper-note",
                    serde_json::json!({ "message": note }),
                );
            }
            return Ok(out.posts);
        }
        match out.stopped_by {
            Some(f) => {
                eprintln!("[douyin] lần {i} (proxy={}) lỗi: {f:?}", px.is_some());
                api_err = Some(f.message(co_dang_nhap));
            }
            None => {
                eprintln!("[douyin] lần {i} (proxy={}) rỗng", px.is_some());
                api_err.get_or_insert_with(|| {
                    "Douyin trả danh sách RỖNG — kênh không có video công khai, \
                     đã bị xoá, hoặc để riêng tư."
                        .into()
                });
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
        // Cả API chính lẫn tikwm đều thất bại → báo LỖI THẬT của đường chính,
        // đã phân loại sẵn (chặn / cookie / riêng tư / đổi API).
        //
        // KHÔNG in sec_uid nữa: nó là định danh của user, và câu cũ "kiểm tra
        // link kênh" khiến anh Hùng đi soi lại cái link vốn ĐÚNG, trong khi
        // nguyên nhân thật là Douyin chặn theo tần suất.
        _ => Err(AppError::Other(format!(
            "Không lấy được video của kênh Douyin này.\n\n{}",
            api_err.unwrap_or_else(|| "Không rõ nguyên nhân.".into()),
        ))),
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  CỔNG: JSON Douyin có trường danh sách = null
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests_null_list {
    use super::*;

    /// CHỨNG MINH CÁI BẪY LÀ THẬT: chỉ `#[serde(default)]` thôi thì `null` NỔ.
    ///
    /// Cổng này tồn tại để ai đó gỡ `deserialize_with = "null_to_vec"` (tưởng
    /// thừa vì "đã có default rồi") sẽ thấy ngay vì sao không được gỡ.
    #[test]
    fn chi_serde_default_van_no_voi_null() {
        #[derive(Debug, Deserialize)]
        struct ChiDefault {
            #[serde(default)]
            #[allow(dead_code)]
            images: Vec<serde_json::Value>,
        }
        // Trường THIẾU HẲN -> default cứu được.
        assert!(serde_json::from_str::<ChiDefault>(r#"{}"#).is_ok());
        // Trường CÓ MẶT nhưng null -> VẪN NỔ. Đây chính là lỗi anh Hùng gặp.
        let e = serde_json::from_str::<ChiDefault>(r#"{"images":null}"#).unwrap_err();
        assert!(
            e.to_string().contains("invalid type: null"),
            "phải nổ đúng lỗi 'invalid type: null', thực tế: {e}"
        );
    }

    /// Bản đã vá: đúng hình dạng Douyin trả về cho VIDEO THƯỜNG (`images: null`)
    /// thì phải bóc trót lọt và ra đủ số bài.
    #[test]
    fn video_thuong_images_null_van_boc_duoc() {
        let raw = r#"{
            "status_code": 0, "max_cursor": 1723600000000, "has_more": 1,
            "aweme_list": [
                {"aweme_id":"7655989781075217704","desc":"video thường",
                 "video":{"cover":{"url_list":["https://x/1.jpg"]}}, "images": null},
                {"aweme_id":"7655989781075217705","desc":"bài ảnh",
                 "video":{"cover":{"url_list": null}}, "images": [{"a":1}]}
            ]
        }"#;
        let r: PostResp = serde_json::from_str(raw).expect("images:null phải bóc được");
        assert_eq!(r.aweme_list.len(), 2, "phải giữ đủ 2 bài");
        assert_eq!(r.aweme_list[0].images.len(), 0, "images:null -> mảng rỗng");
        assert_eq!(r.aweme_list[1].images.len(), 1, "bài ảnh giữ nguyên mảng");
        assert_eq!(r.has_more, 1);
    }

    /// `aweme_list` chính nó bằng null (kênh trống/riêng tư) -> rỗng, KHÔNG nổ.
    #[test]
    fn aweme_list_null_ra_rong_khong_no() {
        let r: PostResp =
            serde_json::from_str(r#"{"status_code":0,"aweme_list":null}"#).unwrap();
        assert_eq!(r.aweme_list.len(), 0);
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  CỔNG: trang CHẶN của Douyin không được coi là "dữ liệu không hợp lệ"
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests_phan_loai_loi {
    use super::*;

    /// THÂN THẬT Douyin trả khi chặn — bắt được 16/08/2026 bằng
    /// `tests/douyin_block.rs`: sau 25 request liên tiếp, HTTP 403 +
    /// text/plain 45 byte. Bản cũ nhét thẳng cái này vào serde_json và in ra
    /// "Dữ liệu Douyin không hợp lệ: expected value at line 1 column 1" —
    /// đúng câu anh Hùng chụp màn hình gửi.
    const THAN_CHAN_THAT: &str = "Blocked by ArgusSecurityPlugin Validate Error";

    #[test]
    fn trang_chan_403_phai_ra_bi_chan_khong_phai_du_lieu_hong() {
        let e = classify_body(403, "text/plain", THAN_CHAN_THAT).unwrap_err();
        assert_eq!(e, DouyinFail::Blocked, "403 + Argus phải là BỊ CHẶN");

        let msg = e.message(true);
        assert!(msg.contains("CHẶN TẠM THỜI"), "phải nói bị chặn: {msg}");
        assert!(
            msg.contains("KHÔNG phải lỗi link kênh"),
            "phải nói rõ link không sai, đừng bắt user đi soi link: {msg}"
        );
        assert!(
            !msg.contains("không hợp lệ"),
            "TUYỆT ĐỐI không được đổ lỗi 'dữ liệu không hợp lệ' khi bị chặn: {msg}"
        );
    }

    /// Chặn nhưng máy chủ trả 200 (đã gặp biến thể) — vẫn phải nhận ra.
    #[test]
    fn nhan_ra_chan_ca_khi_status_200() {
        assert_eq!(
            classify_body(200, "text/plain", THAN_CHAN_THAT).unwrap_err(),
            DouyinFail::Blocked
        );
    }

    /// Thân rỗng (đo thật: xảy ra với MỌI request khi không có ttwid) phải là
    /// một loại RIÊNG, có lời khuyên riêng.
    #[test]
    fn than_rong_la_loai_rieng() {
        assert_eq!(
            classify_body(200, "text/plain", "").unwrap_err(),
            DouyinFail::EmptyBody
        );
        assert_eq!(
            classify_body(200, "text/plain", "   \n").unwrap_err(),
            DouyinFail::EmptyBody
        );
    }

    /// JSON thật thì phải đi lọt.
    #[test]
    fn json_that_van_boc_duoc() {
        let v = classify_body(200, "application/json", r#"{"status_code":0}"#).unwrap();
        assert_eq!(v.get("status_code").and_then(|x| x.as_i64()), Some(0));
    }

    /// HTML lạ = Douyin đổi API / bắt xác minh — phải nói thế, và phải KÈM
    /// nội dung nhận được để còn chẩn đoán.
    #[test]
    fn html_la_bao_doi_api_kem_noi_dung() {
        let e = classify_body(200, "text/html", "<!DOCTYPE html><html>xac minh").unwrap_err();
        match &e {
            DouyinFail::NotJson { head, .. } => assert!(head.contains("DOCTYPE")),
            other => panic!("phải là NotJson, thực tế {other:?}"),
        }
        let msg = e.message(false);
        assert!(msg.contains("đổi API") || msg.contains("xác minh"), "{msg}");
    }

    /// Mỗi loại lỗi phải cho ra lời khuyên KHÁC nhau — nếu ai đó gộp chung
    /// message thì cổng này đỏ.
    #[test]
    fn moi_loai_mot_loi_khuyen_khac_nhau() {
        let msgs = [
            DouyinFail::Blocked.message(true),
            DouyinFail::EmptyBody.message(true),
            DouyinFail::ApiCode(2154).message(true),
            DouyinFail::ApiCode(8).message(true),
            DouyinFail::Network("timeout".into()).message(true),
        ];
        for (i, a) in msgs.iter().enumerate() {
            for (j, b) in msgs.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "loại {i} và {j} cho ra CÙNG một câu");
                }
            }
        }
    }

    /// Chưa có cookie thì phải bảo user nạp cookie; có rồi thì bảo cookie hết hạn.
    #[test]
    fn loi_khuyen_cookie_doi_theo_tinh_trang() {
        assert!(DouyinFail::Blocked.message(false).contains("CHƯA nạp cookie"));
        assert!(DouyinFail::Blocked.message(true).contains("hết hạn"));
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  CỔNG: đọc cookie đăng nhập Douyin từ file Netscape
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests_cookie {
    use super::*;

    /// Hình dạng THẬT của file cookie yt-dlp/trình duyệt xuất ra: có dòng
    /// bình luận, có tiền tố `#HttpOnly_`, có cả domain khác phải loại bỏ.
    const FILE_MAU: &str = "\
# Netscape HTTP Cookie File
# This file is generated by yt-dlp.

.douyin.com\tTRUE\t/\tTRUE\t1799999999\tttwid\tGIA_TRI_TTWID
#HttpOnly_.douyin.com\tTRUE\t/\tTRUE\t1799999999\tsessionid\tGIA_TRI_PHIEN
.douyin.com\tTRUE\t/\tTRUE\t1799999999\tsid_guard\tGIA_TRI_GUARD
www.douyin.com\tFALSE\t/\tFALSE\t1799999999\ts_v_web_id\tGIA_TRI_WEBID
.youtube.com\tTRUE\t/\tTRUE\t1799999999\tSID\tKHONG_LIEN_QUAN
.douyin.com\tTRUE\t/\tTRUE\t1799999999\ttrong_rong\t
";

    #[test]
    fn boc_dung_cookie_douyin_bo_domain_khac() {
        let h = netscape_to_cookie_header(FILE_MAU, "douyin.com").expect("phải có cookie");
        assert!(h.contains("ttwid=GIA_TRI_TTWID"));
        assert!(h.contains("sessionid=GIA_TRI_PHIEN"), "phải bóc được dòng #HttpOnly_");
        assert!(h.contains("s_v_web_id=GIA_TRI_WEBID"), "www.douyin.com cũng phải lấy");
        assert!(!h.contains("SID=KHONG_LIEN_QUAN"), "KHÔNG được lẫn cookie YouTube");
        assert!(!h.contains("trong_rong"), "cookie rỗng giá trị phải bỏ");
    }

    #[test]
    fn nhan_ra_cookie_da_dang_nhap() {
        let h = netscape_to_cookie_header(FILE_MAU, "douyin.com").unwrap();
        assert!(cookie_has_login(&h), "có sessionid/sid_guard = đã đăng nhập");
        assert!(
            !cookie_has_login("ttwid=abc; s_v_web_id=xyz"),
            "chỉ ttwid = KHÁCH VÃNG LAI, không phải đăng nhập"
        );
    }

    #[test]
    fn file_khong_co_douyin_tra_none() {
        let chi_youtube = ".youtube.com\tTRUE\t/\tTRUE\t1\tSID\tabc\n";
        assert!(netscape_to_cookie_header(chi_youtube, "douyin.com").is_none());
        assert!(netscape_to_cookie_header("", "douyin.com").is_none());
    }

    /// Dòng hỏng/thiếu cột không được làm sập cả file cookie.
    #[test]
    fn dong_hong_khong_lam_sap() {
        let ban = format!("{FILE_MAU}.douyin.com\tTRUE\tthieu_cot\n\n\t\t\n");
        let h = netscape_to_cookie_header(&ban, "douyin.com").expect("vẫn phải bóc được");
        assert!(h.contains("sessionid=GIA_TRI_PHIEN"));
    }
}
