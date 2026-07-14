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
    // "Sign in to confirm your AGE" = video giới hạn tuổi (cần cookie), KHÔNG
    // phải bot wall — retry/cooldown không bao giờ giúp, đừng nhầm.
    if l.contains("confirm your age") {
        return false;
    }
    l.contains(SOFT_BLOCK_MARKER)
        || l.contains("sign in to confirm")
        || l.contains("not a bot")
        || l.contains("confirm you")
        || l.contains("http error 429")
        || l.contains("too many requests")
        // Explicit YouTube rate-limit ("rate-limited ... for up to an hour").
        || l.contains("rate-limit")
        || l.contains("rate limit")
}

/// True when the media CDN rejected the download URL itself (HTTP 403).
/// On YouTube this means the extracted googlevideo URL was refused: missing/
/// invalid PO token for the chosen client, IP mismatch between extraction and
/// download, or a player change the current yt-dlp doesn't handle yet. The fix
/// is re-extracting with other player clients + conservative networking (and a
/// fresh proxy if configured) — NOT waiting, and NOT giving up immediately.
pub fn is_forbidden_error(msg: &str) -> bool {
    let l = msg.to_lowercase();
    l.contains("http error 403") || l.contains("403 forbidden") || l.contains("error 403:")
}

/// Marker gắn vào đầu `reason` khi yt-dlp báo "Video unavailable" nhưng app
/// kiểm chứng được video THẬT RA vẫn sống (qua oembed) — tức YouTube đang
/// soft-block IP (hay gặp khi tải cả kênh dồn dập). `is_bot_error` nhận marker
/// này → queue cooldown rồi TỰ tải lại thay vì bỏ cuộc với thông báo sai
/// "video đã bị xoá".
pub const SOFT_BLOCK_MARKER: &str = "[bi-chan-tam-thoi]";

/// True khi yt-dlp nói video không tồn tại. CẢNH GIÁC: khi bị soft-block,
/// YouTube trả đúng câu này cho video vẫn sống — caller phải kiểm chứng
/// (oembed) trước khi tin.
pub fn is_unavailable_error(msg: &str) -> bool {
    msg.to_lowercase().contains("video unavailable")
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
        // Format URL served 0 bytes (often a SABR/broken format) — retrying
        // with other clients usually yields a working URL.
        || l.contains("downloaded file is empty")
        || l.contains("did not get any data blocks")
}

pub fn is_cookie_decrypt_error(msg: &str) -> bool {
    let l = msg.to_lowercase();
    l.contains("dpapi")
        || l.contains("failed to decrypt")
        || l.contains("unable to decrypt")
        || l.contains("could not copy")
        || (l.contains("cookie") && l.contains("decrypt"))
}

/// Trích "lý do" tốt nhất từ stderr của yt-dlp để đưa cho `friendly_reason`.
/// yt-dlp thường in dòng `ERROR: ...` rồi một dòng gợi ý riêng (vd
/// "You might want to use a VPN or a proxy..."). Nếu chỉ lấy DÒNG CUỐI ta hay
/// vớ phải dòng gợi ý → nhận diện sai. Nên: gộp từ dòng chứa "ERROR" tới hết
/// (kèm mọi gợi ý), hoặc 3 dòng cuối nếu không thấy "ERROR".
pub fn best_error_line(stderr: &str) -> String {
    let lines: Vec<&str> = stderr.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    if lines.is_empty() {
        return "yt-dlp failed".to_string();
    }
    if let Some(idx) = lines.iter().rposition(|l| l.to_lowercase().contains("error")) {
        // Dòng ERROR + các dòng sau nó (gợi ý VPN/proxy/cookie) — gộp lại.
        return lines[idx..].join(" ");
    }
    let start = lines.len().saturating_sub(3);
    lines[start..].join(" ")
}

/// Dịch lỗi thô của yt-dlp (tiếng Anh, khó hiểu) thành thông báo tiếng Việt
/// RÕ RÀNG + kèm hướng dẫn user phải làm gì tiếp. Dòng chi tiết kỹ thuật gốc
/// được giữ ở cuối (cỡ chữ nhỏ trong UI) để còn chẩn đoán từ xa được.
///
/// Nguyên tắc viết thông báo: câu đầu nói CHUYỆN GÌ xảy ra, câu sau nói
/// LÀM GÌ để sửa — ưu tiên trỏ về nút "Sửa lỗi tải ngay" trong Cài đặt vì nút
/// đó tự vá được ~90% trường hợp YouTube đổi luật.
pub fn friendly_reason(raw: &str) -> String {
    let l = raw.to_lowercase();

    let hint = if is_forbidden_error(raw) || is_bot_error(raw) {
        "🚫 YouTube đang chặn tải (đã tự thử lại nhiều lần không thành). \
         Cách sửa theo thứ tự: 1) mở Cài đặt → bấm \"Sửa lỗi tải ngay\" → Thử lại; \
         2) thêm cookie (Cài đặt → Cookie — xuất file cookies.txt từ cửa sổ ẩn danh); \
         3) đợi vài giờ cho IP hết bị đánh dấu, hoặc thêm proxy nếu tải số lượng lớn."
    } else if l.contains("private video") {
        "🔒 Video ở chế độ riêng tư — chỉ tải được nếu thêm cookie của tài khoản có quyền xem \
         (Cài đặt → Cookie)."
    } else if l.contains("members-only") || l.contains("join this channel") {
        "🔒 Video chỉ dành cho hội viên của kênh — cần cookie của tài khoản đã đăng ký hội viên \
         (Cài đặt → Cookie)."
    } else if l.contains("confirm your age") || l.contains("age-restricted") || l.contains("age restricted") {
        "🔞 Video giới hạn tuổi — thêm cookie của tài khoản đã đăng nhập (Cài đặt → Cookie) rồi thử lại."
    } else if l.contains("live event will begin") || l.contains("premieres in") || l.contains("premiere will begin") {
        "⏰ Video này là buổi phát trực tiếp / công chiếu CHƯA diễn ra — chưa có gì để tải. \
         Đợi phát xong rồi bấm Thử lại."
    } else if l.contains("registered users") || (l.contains("biliintl") && l.contains("account credentials")) {
        // Nội dung bilibili.tv yêu cầu ĐĂNG NHẬP (tài khoản miễn phí cũng được).
        // buvid ẩn danh không đủ — cần cookie phiên đăng nhập.
        "🔑 Video bilibili.tv này yêu cầu ĐĂNG NHẬP tài khoản (tài khoản MIỄN PHÍ cũng được). \
         Cách làm: mở bilibili.tv trên trình duyệt (qua VPN/proxy) → đăng nhập → xuất file \
         cookies.txt → chọn trong Cài đặt → Cookie. Sau đó tải lại. \
         (Tập có 🔒 Premium thì cần tài khoản trả phí; tập thường chỉ cần đăng nhập.)"
    } else if l.contains("biliintl") && l.contains("412") {
        // bilibili.tv (Bstation): playurl nằm sau tường lửa chống bot (412.js
        // challenge) — không vượt được bằng header/proxy đơn thuần. Cần cookie
        // của tài khoản ĐÃ ĐĂNG NHẬP bilibili.tv + proxy nước ngoài.
        "🅱️ Bilibili.tv chặn tải bằng tường lửa chống bot (lỗi 412). Đây là rào cản mạnh, \
         cần cả 2 thứ: (1) proxy/VPN nước ngoài (Nhật/Singapore) trong Cài đặt; \
         (2) cookie của tài khoản ĐÃ ĐĂNG NHẬP bilibili.tv — mở bilibili.tv trên trình duyệt, \
         đăng nhập, xuất file cookies.txt rồi chọn trong Cài đặt → Cookie. \
         Mẹo: nội dung này thường cũng có trên bilibili.com (không bị tường lửa này)."
    } else if l.contains("412") && (l.contains("precondition") || l.contains("bilibili")) {
        "🅱️ Bilibili tạm chặn request (lỗi 412). App đã tự thêm header chống chặn — \
         bấm Thử lại 1-2 lần. Vẫn lỗi thì đợi vài phút, hoặc thêm proxy (Cài đặt)."
    } else if l.contains("drm protected") || l.contains("drm-protected") {
        "🔐 Video có khoá bản quyền DRM (phim/nội dung trả phí) — YouTube không cho tải loại này, \
         không phải lỗi của app."
    } else if l.contains("in your country")
        || l.contains("in your location")
        || l.contains("in your region")
        || l.contains("geo restricted")
        || l.contains("geo-restricted")
        || l.contains("not available in your")
        || l.contains("use a vpn or a proxy")
        || l.contains("vpn or a proxy server")
    {
        // Vd: "The uploader has not made this video available in your country".
        // Rất hay gặp với kênh đài Nhật/Hàn (日テレ…) khoá video mới chỉ cho
        // xem trong nước họ. KHÔNG phải lỗi app — cần đổi IP sang đúng nước đó.
        "🌍 Video này người đăng CHỈ cho xem ở nước khác (khoá theo vùng) — không tải \
         trực tiếp từ Việt Nam được, mọi tool đều vậy. Cách sửa: mở Cài đặt → thêm 1 PROXY \
         Ở ĐÚNG NƯỚC cho phép (kênh Nhật → proxy Nhật 🇯🇵, kênh Hàn → proxy Hàn 🇰🇷) → \
         bấm \"Kiểm tra proxy\" để xác nhận đúng nước → rồi Thử lại. Hoặc bật VPN đặt ở nước đó."
    } else if l.contains("video unavailable") || l.contains("this video is not available")
        || l.contains("has been removed") || l.contains("account associated with this video has been terminated")
    {
        "❌ Video không tồn tại — đã bị xoá, bị ẩn hoặc link sai. Kiểm tra lại link trên trình duyệt."
    } else if l.contains("is not a valid url") || l.contains("unsupported url") || l.contains("no suitable extractor") {
        "❌ Link này app chưa hỗ trợ hoặc link sai. Thử mở link trên trình duyệt xem có video thật không."
    } else if is_format_error(raw) {
        "⚠️ Không lấy được định dạng video (YouTube vừa đổi cách phát). \
         Cách sửa: mở Cài đặt → bấm \"Sửa lỗi tải ngay\" → rồi bấm Thử lại."
    } else if is_cookie_decrypt_error(raw) {
        "🍪 Không đọc được cookie từ trình duyệt (Chrome/Edge mã hoá kiểu mới). \
         Cách sửa: xuất file cookies.txt (từ cửa sổ ẩn danh) rồi chọn file đó trong Cài đặt → Cookie."
    } else if l.contains("no space left") || l.contains("not enough space") || l.contains("disk full") {
        "💾 Ổ đĩa đầy — dọn bớt dung lượng hoặc đổi thư mục lưu trong Cài đặt."
    } else if l.contains("permission denied") || l.contains("access is denied") {
        "🔐 Không có quyền ghi vào thư mục lưu — đổi thư mục lưu trong Cài đặt (tránh ổ C:\\ gốc / Program Files)."
    } else if l.contains("[watchdog]") {
        "🐌 Tải bị treo quá lâu nên app đã dừng để thử lại. Bấm Thử lại; nếu lặp lại nhiều lần, \
         mở Cài đặt → bấm \"Sửa lỗi tải ngay\" hoặc kiểm tra mạng."
    } else if l.contains("actively refused") || l.contains("failed to establish a new connection") {
        // Kết nối bị TỪ CHỐI ngay — nhà mạng chặn tên miền bằng cách đầu độc
        // DNS (trả 127.0.0.1). Điển hình: bilibili.tv/Bstation ở Việt Nam.
        // Đã kiểm chứng: đây CHỈ là chặn DNS — đổi DNS sang 8.8.8.8 / 1.1.1.1
        // là vào lại được (không cần VPN). Video khoá theo vùng thì mới cần VPN.
        "🚫 Không kết nối được — nhà mạng đang chặn tên miền này (hay gặp với \
         bilibili.tv/Bstation ở Việt Nam). Cách sửa, thử theo thứ tự: \
         1) ĐỔI DNS máy sang 8.8.8.8 và 8.8.4.4 (Cài đặt mạng Windows) rồi bấm Thử lại — \
         nhẹ nhất, không cần cài gì; \
         2) nếu vẫn báo lỗi vùng, bật VPN 1.1.1.1 WARP (miễn phí); \
         3) hoặc tìm video đó trên bilibili.com (không bị chặn)."
    } else if l.contains("getaddrinfo") || l.contains("timed out") || l.contains("timeout")
        || l.contains("unable to connect") || l.contains("connection reset") || l.contains("connection refused")
        || l.contains("network is unreachable") || l.contains("ssl")
    {
        "📡 Lỗi mạng — kiểm tra Internet (hoặc proxy nếu có bật) rồi bấm Thử lại."
    } else {
        "⚠️ Tải thất bại. Cách sửa nhanh: mở Cài đặt → bấm \"Sửa lỗi tải ngay\" → bấm Thử lại. \
         Vẫn lỗi thì gửi dòng chi tiết bên dưới cho người hỗ trợ."
    };

    // Giữ nguyên dòng lỗi gốc (rút gọn) để chẩn đoán — user gửi ảnh chụp là đủ thông tin.
    let detail: String = raw.chars().take(220).collect();
    format!("{hint}\n(Chi tiết kỹ thuật: {detail})")
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self { AppError::Io(err.to_string()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mẫu lỗi thô lấy từ yt-dlp thật — mỗi cái phải ra đúng thông báo hướng dẫn.

    #[test]
    fn friendly_403_points_to_fix_button() {
        let m = friendly_reason("(exit 1) ERROR: unable to download video data: HTTP Error 403: Forbidden");
        assert!(m.contains("Sửa lỗi tải ngay"), "403 phải trỏ về nút sửa lỗi: {m}");
        assert!(m.contains("Chi tiết kỹ thuật"));
        assert!(m.contains("403"), "phải giữ chi tiết gốc");
    }

    #[test]
    fn friendly_bot_wall_points_to_fix_button() {
        let m = friendly_reason("ERROR: [youtube] abc: Sign in to confirm you're not a bot.");
        assert!(m.contains("Sửa lỗi tải ngay"));
    }

    #[test]
    fn friendly_age_restricted_says_cookie_not_bot() {
        let raw = "ERROR: [youtube] abc: Sign in to confirm your age. This video may be inappropriate for some users.";
        assert!(!is_bot_error(raw), "giới hạn tuổi không phải bot wall");
        let m = friendly_reason(raw);
        assert!(m.contains("Cookie"), "giới hạn tuổi phải chỉ về cookie: {m}");
    }

    #[test]
    fn friendly_private_and_members_only() {
        assert!(friendly_reason("ERROR: [youtube] x: Private video. Sign in if you've been granted access to this video").contains("riêng tư"));
        assert!(friendly_reason("ERROR: [youtube] x: Join this channel to get access to members-only content").contains("hội viên"));
    }

    #[test]
    fn friendly_unavailable_and_unsupported() {
        assert!(friendly_reason("ERROR: [youtube] x: Video unavailable").contains("không tồn tại"));
        assert!(friendly_reason("ERROR: Unsupported URL: https://example.com/abc").contains("chưa hỗ trợ"));
    }

    #[test]
    fn friendly_format_error_points_to_fix_button() {
        let m = friendly_reason("ERROR: [youtube] x: Requested format is not available.");
        assert!(m.contains("Sửa lỗi tải ngay"));
    }

    #[test]
    fn friendly_network_and_disk() {
        assert!(friendly_reason("ERROR: Unable to download webpage: <urlopen error [Errno 11001] getaddrinfo failed>").contains("Lỗi mạng"));
        assert!(friendly_reason("OSError: [Errno 28] No space left on device").contains("đầy"));
        assert!(friendly_reason("PermissionError: [WinError 5] Access is denied").contains("quyền ghi"));
    }

    #[test]
    fn friendly_cookie_decrypt() {
        let m = friendly_reason("ERROR: Failed to decrypt with DPAPI. See https://github.com/yt-dlp/yt-dlp/issues/10927");
        assert!(m.contains("cookies.txt"));
    }

    #[test]
    fn friendly_watchdog_stall() {
        let m = friendly_reason("[watchdog] no activity for 90s, killed yt-dlp");
        assert!(m.contains("treo"));
    }

    #[test]
    fn friendly_unknown_error_has_default_guidance_and_detail() {
        let m = friendly_reason("ERROR: something totally new and weird 12345");
        assert!(m.contains("Sửa lỗi tải ngay"));
        assert!(m.contains("something totally new"), "chi tiết gốc phải được giữ: {m}");
    }

    #[test]
    fn friendly_detail_is_truncated() {
        let long = format!("ERROR: xyz {}", "a".repeat(500));
        let m = friendly_reason(&long);
        assert!(m.len() < 700, "chi tiết phải được cắt ngắn, len = {}", m.len());
    }

    #[test]
    fn friendly_domain_blocked_by_isp() {
        // Lỗi thật từ bilibili.tv trên mạng VN (DNS nhà mạng trỏ về 127.0.0.1).
        let m = friendly_reason(
            "ERROR: [BiliIntl] 479439: Unable to download webpage: HTTPSConnection(host='www.bilibili.tv', port=443): \
             Failed to establish a new connection: [WinError 10061] No connection could be made because the target machine actively refused it",
        );
        assert!(m.contains("nhà mạng"), "phải chỉ ra bị nhà mạng chặn: {m}");
        assert!(m.contains("DNS"), "phải gợi ý đổi DNS: {m}");
        assert!(m.contains("VPN"));
    }

    #[test]
    fn friendly_geo_blocked_matches_real_youtube_message() {
        // Đúng câu yt-dlp trả cho kênh Nhật 日テレ (khoá video mới chỉ ở Nhật).
        let raw = "(exit 1) ERROR: [youtube] 3HStu-xnR6E: The uploader has not made this video available in your country";
        let m = friendly_reason(raw);
        assert!(m.contains("khoá theo vùng"), "phải nhận diện geo-block: {m}");
        assert!(m.contains("PROXY") || m.contains("proxy"), "phải hướng dẫn proxy: {m}");
        assert!(!m.contains("Sửa lỗi tải ngay"), "KHÔNG được báo nhầm là lỗi bot: {m}");
        assert!(!m.contains("không tồn tại"), "KHÔNG được báo nhầm là video bị xoá: {m}");
    }

    #[test]
    fn best_error_line_grabs_error_not_hint() {
        // yt-dlp in dòng ERROR rồi dòng gợi ý riêng — phải gộp cả hai.
        let stderr = "[BiliIntl] Extracting URL\n\
                      ERROR: [BiliIntl] 123: This video is not available in your region\n\
                      You might want to use a VPN or a proxy server (with --proxy) to workaround.";
        let r = best_error_line(stderr);
        assert!(r.contains("not available in your region"), "phải lấy dòng ERROR: {r}");
        assert!(r.contains("VPN or a proxy"), "phải kèm gợi ý: {r}");
        // Và friendly_reason phải ra thông báo khoá vùng (không phải generic).
        let m = friendly_reason(&r);
        assert!(m.contains("khoá theo vùng"), "phải nhận diện geo: {m}");
    }

    #[test]
    fn friendly_vpn_hint_alone_is_geo_not_generic() {
        // Trường hợp xấu: chỉ còn dòng gợi ý (không có dòng ERROR rõ).
        let m = friendly_reason("You might want to use a VPN or a proxy server (with --proxy) to workaround.");
        assert!(m.contains("khoá theo vùng"), "gợi ý VPN → geo: {m}");
        assert!(!m.contains("Sửa lỗi tải ngay"), "không được ra generic: {m}");
    }

    #[test]
    fn friendly_biliintl_registered_users() {
        let m = friendly_reason("(exit 1) ERROR: [BiliIntl] This video is only available for registered users. Use --cookies, --cookies-from-browser, --username and --password to provide account credentials");
        assert!(m.contains("ĐĂNG NHẬP"), "phải hướng dẫn đăng nhập: {m}");
        assert!(m.contains("cookies.txt") || m.contains("Cookie"));
        assert!(!m.contains("Sửa lỗi tải ngay"));
    }

    #[test]
    fn friendly_bilibili_412() {
        let m = friendly_reason("(exit 1) ERROR: [BiliIntl] 23336253: Unable to download video formats: HTTP Error 412: Precondition Failed");
        assert!(m.contains("412"), "phải nhận diện 412: {m}");
        assert!(m.contains("cookie") || m.contains("Cookie"), "bilibili.tv 412 phải hướng dẫn cookie: {m}");
        assert!(!m.contains("Sửa lỗi tải ngay"), "không nhầm sang lỗi bot: {m}");

        // bilibili.com 412 (không phải BiliIntl) → thông báo nhẹ hơn.
        let m2 = friendly_reason("ERROR: [BiliBili] BV1xx: HTTP Error 412: Precondition Failed");
        assert!(m2.contains("Thử lại"), "bilibili.com 412 bảo thử lại: {m2}");
    }

    #[test]
    fn friendly_premiere_and_drm() {
        assert!(friendly_reason("ERROR: [youtube] x: This live event will begin in 3 hours").contains("CHƯA diễn ra"));
        assert!(friendly_reason("ERROR: [youtube] x: Premieres in 2 hours").contains("CHƯA diễn ra"));
        assert!(friendly_reason("ERROR: [youtube] x: This video is DRM protected").contains("DRM"));
    }

    #[test]
    fn unavailable_detection() {
        assert!(is_unavailable_error("(exit 1) ERROR: [youtube] 2kWaMLjMzXA: Video unavailable"));
        assert!(!is_unavailable_error("HTTP Error 403: Forbidden"));
    }

    #[test]
    fn soft_block_marker_treated_as_bot_not_deleted_video() {
        // "Video unavailable" nhưng oembed xác nhận video còn sống → runner gắn
        // marker → phải được xử như bot wall (cooldown + tự tải lại), và thông
        // báo KHÔNG được nói "video đã bị xoá".
        let reason = format!("{SOFT_BLOCK_MARKER} (exit 1) ERROR: [youtube] abc: Video unavailable");
        assert!(is_bot_error(&reason), "soft-block phải được coi là bot error");
        let m = friendly_reason(&reason);
        assert!(m.contains("Sửa lỗi tải ngay"), "phải ra thông báo chặn tạm: {m}");
        assert!(!m.contains("không tồn tại"), "không được báo nhầm video bị xoá: {m}");
    }

    #[test]
    fn forbidden_error_detection() {
        assert!(is_forbidden_error("HTTP Error 403: Forbidden"));
        assert!(is_forbidden_error("(exit 1) ERROR: ... HTTP Error 403: Forbidden"));
        assert!(!is_forbidden_error("HTTP Error 404: Not Found"));
        assert!(!is_forbidden_error("Sign in to confirm you're not a bot"));
    }
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
