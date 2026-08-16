//! PROBE CHẨN ĐOÁN — gọi THẬT API kênh Douyin và in ra thân phản hồi.
//!
//! Không chạy trong lượt test thường (`#[ignore]`). Chạy tay:
//!   cargo test --test douyin_probe -- --ignored --nocapture
//!
//! LUẬT: KHÔNG in cookie/token/sec_uid đầy đủ. Che bớt trước khi in.

use bqhungdown_lib::douyin_sign::{ABogus, DOUYIN_UA};
use std::time::Duration;

/// Che chuỗi nhạy cảm: giữ 8 ký tự đầu + độ dài.
fn mask(s: &str) -> String {
    let head: String = s.chars().take(8).collect();
    format!("{head}…<che, len={}>", s.len())
}

/// Che mọi thứ trông giống token/cookie trong thân phản hồi trước khi in.
fn mask_body(s: &str) -> String {
    let re = regex::Regex::new(r"(?i)(ttwid|msToken|sessionid|passport|odin_tt|s_v_web_id|sec_uid|sec_user_id)([\x22'=:\s]+)([A-Za-z0-9_%+/=.\-|]{8,})").unwrap();
    re.replace_all(s, "$1$2<CHE>").to_string()
}

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

async fn fetch_ttwid() -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
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

/// ĐỐI CHỨNG SỐ THẬT: hỏi thẳng Douyin kênh này có bao nhiêu video
/// (`user.aweme_count` trên trang hồ sơ) để so với số app lấy được.
#[tokio::test]
#[ignore]
async fn so_video_that_cua_kenh() {
    let sec_uid = std::env::var("BQD_SEC_UID")
        .unwrap_or_else(|_| "MS4wLjABAAAA7yRYvacLzSF5V0J8mrM0eE-PdL3O9_dNdDJTb-0peRw".into());
    let cookie = std::env::var("BQD_COOKIE_FILE").ok().and_then(|p| {
        let raw = std::fs::read_to_string(p).ok()?;
        bqhungdown_lib::douyin_scraper::netscape_to_cookie_header(&raw, "douyin.com")
    });

    let params = format!(
        "device_platform=webapp&aid=6383&channel=channel_pc_web&sec_user_id={sec_uid}\
&publish_video_strategy_type=2&version_code=290100&version_name=29.1.0&cookie_enabled=true\
&screen_width=1536&screen_height=864&browser_language=zh-CN&browser_platform=Win32\
&browser_name=Chrome&browser_version=90.0.4430.212&browser_online=true&engine_name=Blink\
&engine_version=90.0&os_name=Windows&os_version=10&cpu_core_num=8&device_memory=8\
&platform=PC&downlink=10&effective_type=4g&round_trip_time=50"
    );
    let ab = ABogus::new();
    let url = format!(
        "https://www.douyin.com/aweme/v1/web/user/profile/other/?{params}&a_bogus={}",
        pct_encode(&ab.get_value(&params, "GET"))
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(DOUYIN_UA)
        .build()
        .unwrap();
    let mut req = client
        .get(&url)
        .header("Referer", format!("https://www.douyin.com/user/{sec_uid}"));
    if let Some(c) = &cookie {
        req = req.header("Cookie", c.as_str());
    }
    let resp = req.send().await.expect("gửi lỗi");
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    match serde_json::from_str::<serde_json::Value>(&body) {
        Ok(v) => {
            let u = v.get("user");
            println!("== status_code = {:?}", v.get("status_code"));
            println!(
                "== SỐ VIDEO THẬT (user.aweme_count) = {:?}",
                u.and_then(|u| u.get("aweme_count"))
            );
            println!(
                "== nickname = {:?}",
                u.and_then(|u| u.get("nickname")).and_then(|x| x.as_str())
            );
        }
        Err(e) => {
            println!("== HTTP {status}, không bóc được JSON: {e}");
            let head: String = body.chars().take(300).collect();
            println!("== đầu thân: {}", mask_body(&head));
        }
    }
}

#[tokio::test]
#[ignore]
async fn probe_douyin_channel_raw_body() {
    // sec_uid từ ảnh lỗi anh Hùng gửi.
    let sec_uid = std::env::var("BQD_SEC_UID")
        .unwrap_or_else(|_| "MS4wLjABAAAA7yRYvacLzSF5V0J8mrM0eE-PdL3O9_dNdDJTb-0peRw".into());
    println!("== sec_uid: {}", mask(&sec_uid));

    let ttwid = fetch_ttwid().await;
    match &ttwid {
        Some(t) => println!("== ttwid: LẤY ĐƯỢC {}", mask(t)),
        None => println!("== ttwid: KHÔNG LẤY ĐƯỢC (None)"),
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(DOUYIN_UA)
        .build()
        .unwrap();

    let ab = ABogus::new();
    let params = build_post_params(&sec_uid, 0);
    let a_bogus = ab.get_value(&params, "GET");
    println!("== a_bogus: {}", mask(&a_bogus));

    let url = format!(
        "https://www.douyin.com/aweme/v1/web/aweme/post/?{params}&a_bogus={}",
        pct_encode(&a_bogus)
    );

    let referer = format!("https://www.douyin.com/user/{sec_uid}");
    let mut req = client.get(&url).header("Referer", &referer);
    if let Some(tt) = &ttwid {
        req = req.header("Cookie", format!("ttwid={tt}"));
    }

    let resp = req.send().await.expect("gửi request thất bại");
    println!("== HTTP status: {}", resp.status());
    for (k, v) in resp.headers().iter() {
        let kn = k.as_str().to_lowercase();
        if kn == "content-type" || kn == "content-length" || kn == "server" || kn.starts_with("x-") {
            println!("== header {kn}: {}", v.to_str().unwrap_or("<binary>"));
        }
        if kn == "set-cookie" {
            println!("== header set-cookie: <CHE>");
        }
    }

    let body = resp.text().await.unwrap_or_default();
    println!("== body len = {} byte", body.len());
    println!("== body rỗng? {}", body.trim().is_empty());
    let head: String = body.chars().take(500).collect();
    println!("== 500 KÝ TỰ ĐẦU (đã che) ==\n{}\n== HẾT ==", mask_body(&head));

    // Thử bóc JSON để tái hiện đúng lỗi anh Hùng thấy.
    match serde_json::from_str::<serde_json::Value>(&body) {
        Ok(v) => {
            println!("== BÓC JSON OK");
            if let Some(sc) = v.get("status_code") {
                println!("== status_code = {sc}");
            }
            if let Some(list) = v.get("aweme_list") {
                println!(
                    "== aweme_list = {}",
                    match list {
                        serde_json::Value::Null => "null".to_string(),
                        serde_json::Value::Array(a) => format!("mảng {} phần tử", a.len()),
                        _ => "kiểu khác".to_string(),
                    }
                );
            }
            println!("== has_more = {:?}", v.get("has_more"));
            println!("== max_cursor = {:?}", v.get("max_cursor"));
        }
        Err(e) => println!("== BÓC JSON HỎNG: {e}"),
    }
}

/// PROBE: Douyin CÓ trả lượt xem thật + ngày đăng trong gói `aweme_list` không?
///
/// Chạy tay (cần cookie đăng nhập):
///   BQD_COOKIE_FILE=<duong-dan> cargo test --test douyin_probe \
///     -- --ignored probe_view_va_ngay --nocapture
///
/// LUẬT: KHÔNG in cookie/sec_uid đầy đủ.
#[tokio::test]
#[ignore]
async fn probe_view_va_ngay() {
    let sec_uid = std::env::var("BQD_SEC_UID")
        .unwrap_or_else(|_| "MS4wLjABAAAA7yRYvacLzSF5V0J8mrM0eE-PdL3O9_dNdDJTb-0peRw".into());
    println!("== sec_uid: {}", mask(&sec_uid));

    let cookie = std::env::var("BQD_COOKIE_FILE").ok().and_then(|p| {
        let raw = std::fs::read_to_string(p).ok()?;
        bqhungdown_lib::douyin_scraper::netscape_to_cookie_header(&raw, "douyin.com")
    });
    println!(
        "== cookie đăng nhập: {}",
        match &cookie {
            Some(c) => format!("CÓ ({} byte)", c.len()),
            None => "KHÔNG".into(),
        }
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(DOUYIN_UA)
        .build()
        .unwrap();
    let ab = ABogus::new();
    let params = build_post_params(&sec_uid, 0);
    let url = format!(
        "https://www.douyin.com/aweme/v1/web/aweme/post/?{params}&a_bogus={}",
        pct_encode(&ab.get_value(&params, "GET"))
    );
    let mut req = client
        .get(&url)
        .header("Referer", format!("https://www.douyin.com/user/{sec_uid}"));
    if let Some(c) = &cookie {
        req = req.header("Cookie", c.as_str());
    }
    let resp = req.send().await.expect("gửi request thất bại");
    println!("== HTTP status: {}", resp.status());
    let body = resp.text().await.unwrap_or_default();
    println!("== body len = {} byte", body.len());

    let v: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            println!("== BÓC JSON HỎNG: {e}");
            let head: String = body.chars().take(300).collect();
            println!("== đầu thân: {}", mask_body(&head));
            return;
        }
    };
    println!("== status_code = {:?}", v.get("status_code"));
    let list = match v.get("aweme_list").and_then(|x| x.as_array()) {
        Some(a) => a,
        None => {
            println!("== aweme_list KHÔNG phải mảng: {:?}", v.get("aweme_list"));
            return;
        }
    };
    println!("== aweme_list = {} bài\n", list.len());

    let (mut co_play, mut co_time, mut co_digg) = (0usize, 0usize, 0usize);
    for (i, a) in list.iter().enumerate() {
        let st = a.get("statistics");
        let play = st.and_then(|s| s.get("play_count")).and_then(|x| x.as_i64());
        let digg = st.and_then(|s| s.get("digg_count")).and_then(|x| x.as_i64());
        let cmt = st.and_then(|s| s.get("comment_count")).and_then(|x| x.as_i64());
        let shr = st.and_then(|s| s.get("share_count")).and_then(|x| x.as_i64());
        let ct = a.get("create_time").and_then(|x| x.as_i64());
        if play.unwrap_or(0) > 0 { co_play += 1; }
        if digg.unwrap_or(0) > 0 { co_digg += 1; }
        if ct.unwrap_or(0) > 0 { co_time += 1; }
        if i < 6 {
            let ngay = ct
                .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| "<không có>".into());
            let tieu_de: String = a
                .get("desc")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .chars()
                .take(24)
                .collect();
            println!(
                "  [{i}] create_time={ct:?} ({ngay}) play={play:?} digg={digg:?} cmt={cmt:?} share={shr:?} | {tieu_de}"
            );
        }
    }
    println!("\n== TỔNG KẾT trên {} bài:", list.len());
    println!("   create_time > 0 : {co_time}/{}", list.len());
    println!("   play_count  > 0 : {co_play}/{}", list.len());
    println!("   digg_count  > 0 : {co_digg}/{}", list.len());
    println!(
        "   => lượt xem THẬT? {}",
        if co_play > 0 { "CÓ" } else { "KHÔNG (Douyin trả 0 cho web API)" }
    );
    if let Some(st) = list.first().and_then(|a| a.get("statistics")) {
        println!("== các khoá trong `statistics` của bài đầu:");
        if let Some(o) = st.as_object() {
            let mut ks: Vec<&String> = o.keys().collect();
            ks.sort();
            println!("   {ks:?}");
        }
    }
}
