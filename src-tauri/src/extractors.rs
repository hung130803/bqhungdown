use std::sync::OnceLock;
use regex::Regex;

pub struct ExtractorPattern {
    pub name: &'static str,
    pub host_regex: &'static str, // pattern for host (or path-aware)
    pub featured: bool,
}

/// Bảng extractor — đặt tên match yt-dlp khi có thể. Featured = nền tảng nổi
/// bật, hiện ưu tiên trên UI. Non-featured vẫn nhận diện badge/icon đúng,
/// chỉ không nằm trong danh sách "platform suggestion" trên trang chủ.
///
/// LƯU Ý: yt-dlp tự nó hỗ trợ 1800+ site. Bảng này chủ yếu để hiển thị icon /
/// màu sắc / tên nền tảng cho UI; site nào không có trong bảng vẫn tải được
/// nếu yt-dlp hỗ trợ — chỉ là badge hiện "generic".
pub static EXTRACTORS: &[ExtractorPattern] = &[
    // ── Featured (Top 10) ──────────────────────────────────────────────────
    ExtractorPattern { name: "youtube",     host_regex: r"^(?:www\.|m\.|music\.)?youtube\.com$|^youtu\.be$",                    featured: true },
    ExtractorPattern { name: "tiktok",      host_regex: r"^(?:www\.|m\.|vm\.|vt\.)?tiktok\.com$",                                featured: true },
    ExtractorPattern { name: "facebook",    host_regex: r"^(?:www\.|m\.|web\.|business\.|fb\.)?facebook\.com$|^fb\.watch$",      featured: true },
    ExtractorPattern { name: "instagram",   host_regex: r"^(?:www\.)?instagram\.com$",                                            featured: true },
    ExtractorPattern { name: "twitter",     host_regex: r"^(?:www\.|mobile\.)?(?:twitter|x)\.com$|^t\.co$",                       featured: true },
    ExtractorPattern { name: "twitch",      host_regex: r"^(?:www\.|m\.|clips\.)?twitch\.tv$",                                    featured: true },
    ExtractorPattern { name: "vimeo",       host_regex: r"^(?:www\.|player\.)?vimeo\.com$",                                       featured: true },
    ExtractorPattern { name: "reddit",      host_regex: r"^(?:www\.|old\.|new\.)?reddit\.com$|^v\.redd\.it$",                     featured: true },
    ExtractorPattern { name: "dailymotion", host_regex: r"^(?:www\.)?dailymotion\.com$|^dai\.ly$",                                featured: true },
    // bilibili.tv = Bstation (bản quốc tế) — yt-dlp BiliIntl extractor xử lý.
    // LƯU Ý: bilibili.tv bị nhà mạng VN chặn DNS + kết nối (Bstation rút khỏi
    // VN) — link vẫn nhận, nhưng tải được hay không tuỳ mạng/VPN của user;
    // lỗi kết nối sẽ ra thông báo hướng dẫn VPN (error::friendly_reason).
    ExtractorPattern { name: "bilibili",    host_regex: r"^(?:www\.|m\.|space\.)?bilibili\.(?:com|tv)$|^b23\.tv$",               featured: true },

    // ── Streaming / Live ───────────────────────────────────────────────────
    ExtractorPattern { name: "kick",        host_regex: r"^(?:www\.)?kick\.com$",                                                 featured: false },
    ExtractorPattern { name: "rumble",      host_regex: r"^(?:www\.)?rumble\.com$",                                               featured: false },
    ExtractorPattern { name: "odysee",      host_regex: r"^(?:www\.)?odysee\.com$",                                               featured: false },
    ExtractorPattern { name: "bitchute",    host_regex: r"^(?:www\.)?bitchute\.com$",                                             featured: false },
    ExtractorPattern { name: "dtube",       host_regex: r"^(?:www\.)?d\.tube$",                                                   featured: false },
    ExtractorPattern { name: "streamable",  host_regex: r"^(?:www\.)?streamable\.com$",                                           featured: false },

    // ── Social / Short-form ────────────────────────────────────────────────
    ExtractorPattern { name: "threads",     host_regex: r"^(?:www\.)?threads\.net$",                                              featured: false },
    ExtractorPattern { name: "snapchat",    host_regex: r"^(?:www\.|story\.)?snapchat\.com$",                                     featured: false },
    ExtractorPattern { name: "tumblr",      host_regex: r"^(?:www\.|.+\.)?tumblr\.com$",                                          featured: false },
    ExtractorPattern { name: "linkedin",    host_regex: r"^(?:www\.)?linkedin\.com$",                                             featured: false },
    ExtractorPattern { name: "pinterest",   host_regex: r"^(?:www\.)?pinterest\.com$|^pin\.it$",                                  featured: false },
    ExtractorPattern { name: "vk",          host_regex: r"^(?:www\.|m\.)?vk\.com$",                                               featured: false },
    ExtractorPattern { name: "ok_ru",       host_regex: r"^(?:www\.)?ok\.ru$",                                                    featured: false },
    ExtractorPattern { name: "weibo",       host_regex: r"^(?:www\.|m\.)?weibo\.com$|^weibo\.cn$",                                featured: false },
    ExtractorPattern { name: "viralhog",    host_regex: r"^(?:www\.)?viralhog\.com$",                                             featured: false },
    ExtractorPattern { name: "9gag",        host_regex: r"^(?:www\.|img-9gag-fun\.)?9gag\.com$",                                  featured: false },
    ExtractorPattern { name: "imgur",       host_regex: r"^(?:www\.|i\.|m\.)?imgur\.com$",                                        featured: false },
    ExtractorPattern { name: "gfycat",      host_regex: r"^(?:www\.)?gfycat\.com$",                                               featured: false },
    ExtractorPattern { name: "redgifs",     host_regex: r"^(?:www\.|v3\.)?redgifs\.com$",                                         featured: false },
    ExtractorPattern { name: "coub",        host_regex: r"^(?:www\.)?coub\.com$",                                                 featured: false },

    // ── Asian / Chinese ────────────────────────────────────────────────────
    ExtractorPattern { name: "douyin",      host_regex: r"^(?:www\.)?douyin\.com$|^v\.douyin\.com$|^(?:www\.)?iesdouyin\.com$",                              featured: false },
    ExtractorPattern { name: "kuaishou",    host_regex: r"^(?:www\.)?kuaishou\.com$",                                             featured: false },
    ExtractorPattern { name: "iqiyi",       host_regex: r"^(?:www\.)?iqiyi\.com$|^iq\.com$",                                      featured: false },
    ExtractorPattern { name: "youku",       host_regex: r"^(?:www\.|v\.)?youku\.com$",                                            featured: false },
    ExtractorPattern { name: "niconico",    host_regex: r"^(?:www\.|sp\.)?nicovideo\.jp$",                                        featured: false },
    ExtractorPattern { name: "naver",       host_regex: r"^(?:tv\.|m\.)?naver\.com$",                                             featured: false },
    ExtractorPattern { name: "vlive",       host_regex: r"^(?:www\.)?vlive\.tv$",                                                 featured: false },

    // ── Music / Audio ──────────────────────────────────────────────────────
    ExtractorPattern { name: "soundcloud",  host_regex: r"^(?:www\.|m\.)?soundcloud\.com$",                                       featured: false },
    ExtractorPattern { name: "mixcloud",    host_regex: r"^(?:www\.)?mixcloud\.com$",                                             featured: false },
    ExtractorPattern { name: "bandcamp",    host_regex: r"^(?:www\.|.+\.)?bandcamp\.com$",                                        featured: false },
    ExtractorPattern { name: "audiomack",   host_regex: r"^(?:www\.)?audiomack\.com$",                                            featured: false },

    // ── Education ──────────────────────────────────────────────────────────
    ExtractorPattern { name: "ted",         host_regex: r"^(?:www\.)?ted\.com$",                                                  featured: false },
    ExtractorPattern { name: "coursera",    host_regex: r"^(?:www\.)?coursera\.org$",                                             featured: false },
    ExtractorPattern { name: "udemy",       host_regex: r"^(?:www\.)?udemy\.com$",                                                featured: false },
    ExtractorPattern { name: "khanacademy", host_regex: r"^(?:www\.)?khanacademy\.org$",                                          featured: false },

    // ── Broadcaster / News ────────────────────────────────────────────────
    ExtractorPattern { name: "bbc",         host_regex: r"^(?:www\.)?bbc\.(?:co\.uk|com)$",                                       featured: false },
    ExtractorPattern { name: "cnn",         host_regex: r"^(?:www\.|edition\.|money\.)?cnn\.com$",                                featured: false },
    ExtractorPattern { name: "arte",        host_regex: r"^(?:www\.)?arte\.tv$",                                                  featured: false },
    ExtractorPattern { name: "zdf",         host_regex: r"^(?:www\.)?zdf\.de$",                                                   featured: false },
    ExtractorPattern { name: "nhk",         host_regex: r"^(?:www3?\.)?nhk\.or\.jp$",                                             featured: false },
    ExtractorPattern { name: "nbcnews",     host_regex: r"^(?:www\.)?nbcnews\.com$",                                              featured: false },
    ExtractorPattern { name: "espn",        host_regex: r"^(?:www\.)?espn\.com$",                                                 featured: false },

    // ── Subscription / Patreon-like ────────────────────────────────────────
    ExtractorPattern { name: "patreon",     host_regex: r"^(?:www\.)?patreon\.com$",                                              featured: false },
    ExtractorPattern { name: "newgrounds",  host_regex: r"^(?:www\.)?newgrounds\.com$",                                           featured: false },

    // ── File hosting / Cloud (yt-dlp can grab direct MP4) ─────────────────
    ExtractorPattern { name: "gdrive",      host_regex: r"^drive\.google\.com$|^docs\.google\.com$",                              featured: false },
    ExtractorPattern { name: "dropbox",     host_regex: r"^(?:www\.)?dropbox\.com$",                                              featured: false },

    // ── Vietnamese / SEA ──────────────────────────────────────────────────
    ExtractorPattern { name: "zingmp3",     host_regex: r"^(?:mp3\.)?zing\.vn$|^zingmp3\.vn$",                                    featured: false },
    ExtractorPattern { name: "nhaccuatui",  host_regex: r"^(?:www\.)?nhaccuatui\.com$",                                           featured: false },
    ExtractorPattern { name: "vidio",       host_regex: r"^(?:www\.)?vidio\.com$",                                                featured: false },
];

static COMPILED: OnceLock<Vec<(Regex, &'static str, bool)>> = OnceLock::new();

fn compiled() -> &'static [(Regex, &'static str, bool)] {
    COMPILED.get_or_init(|| {
        EXTRACTORS
            .iter()
            .map(|e| (Regex::new(e.host_regex).unwrap(), e.name, e.featured))
            .collect()
    })
}

/// Match a hostname (lowercased) against the extractor table.
/// Returns the extractor name (e.g., "youtube") or `None` if no entry matches.
/// Caller passes only the host part (no scheme, no port).
pub fn match_host(host: &str) -> Option<&'static str> {
    let host_lc = host.to_lowercase();
    for (re, name, _featured) in compiled() {
        if re.is_match(&host_lc) {
            return Some(name);
        }
    }
    None
}

/// Returns true if the extractor is one of the featured platforms shown
/// prominently on the UI.
pub fn is_featured(extractor: &str) -> bool {
    EXTRACTORS
        .iter()
        .find(|e| e.name == extractor)
        .map(|e| e.featured)
        .unwrap_or(false)
}

/// Returns the full table of `ExtractorPattern` for use by the frontend
/// `list_extractors` command (Tauri-serializable view created by the caller).
pub fn list_all() -> &'static [ExtractorPattern] {
    EXTRACTORS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_matches() {
        assert_eq!(match_host("www.youtube.com"), Some("youtube"));
    }

    #[test]
    fn youtu_be_matches() {
        assert_eq!(match_host("youtu.be"), Some("youtube"));
    }

    #[test]
    fn x_com_matches_twitter() {
        assert_eq!(match_host("x.com"), Some("twitter"));
    }

    #[test]
    fn unknown_host() {
        assert!(match_host("randomsite.local").is_none());
    }

    #[test]
    fn viralhog_matches() {
        assert_eq!(match_host("viralhog.com"), Some("viralhog"));
        assert_eq!(match_host("www.viralhog.com"), Some("viralhog"));
    }

    #[test]
    fn kick_matches() {
        assert_eq!(match_host("kick.com"), Some("kick"));
    }

    #[test]
    fn streamable_matches() {
        assert_eq!(match_host("streamable.com"), Some("streamable"));
    }

    #[test]
    fn all_regexes_compile() {
        // Trigger compilation of every regex; will panic if any malformed.
        let n = compiled().len();
        assert!(n > 30, "expected >30 extractors, got {n}");
    }
}
