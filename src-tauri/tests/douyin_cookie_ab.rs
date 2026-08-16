//! NGHIỆM THU BẢN VÁ — dùng ĐÚNG các hàm đã ship trong lib
//! (`netscape_to_cookie_header`, `classify_body`) để chứng minh:
//!   A) đường cũ (chỉ ttwid ẩn danh)      -> Douyin cắt ở ~20 video
//!   B) đường mới (cookie đăng nhập user) -> lấy đủ cả kênh
//!
//! Chạy tay:
//!   BQD_COOKIE_FILE="C:\...\ck.txt" cargo test --test douyin_cookie_ab -- --ignored --nocapture
//!
//! LUẬT: CHỈ ĐỌC file cookie, KHÔNG in giá trị cookie.

use bqhungdown_lib::douyin_scraper::{classify_body, netscape_to_cookie_header, DouyinFail};
use bqhungdown_lib::douyin_sign::{ABogus, DOUYIN_UA};
use std::collections::HashSet;
use std::time::Duration;

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

/// Bò hết trang, phân loại phản hồi bằng `classify_body` của lib.
async fn crawl(sec_uid: &str, cookie: Option<&str>, nhan: &str) -> (usize, usize) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(DOUYIN_UA)
        .build()
        .unwrap();
    let ab = ABogus::new();
    let referer = format!("https://www.douyin.com/user/{sec_uid}");
    let mut seen: HashSet<String> = HashSet::new();
    let mut cursor: i64 = 0;
    let mut pages = 0usize;

    for page in 0..200 {
        let params = build_post_params(sec_uid, cursor);
        let a_bogus = ab.get_value(&params, "GET");
        let url = format!(
            "https://www.douyin.com/aweme/v1/web/aweme/post/?{params}&a_bogus={}",
            pct_encode(&a_bogus)
        );
        let mut req = client.get(&url).header("Referer", &referer);
        if let Some(c) = cookie {
            req = req.header("Cookie", c);
        }
        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                println!("[{nhan}] trang {page}: gửi lỗi {e}");
                break;
            }
        };
        let status = resp.status().as_u16();
        let ctype = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("?")
            .to_string();
        let body = resp.text().await.unwrap_or_default();
        pages = page + 1;

        let v = match classify_body(status, &ctype, &body) {
            Ok(v) => v,
            Err(f) => {
                println!("[{nhan}] trang {page}: DỪNG vì {f:?}");
                if f == DouyinFail::Blocked {
                    println!("[{nhan}]   -> app mới sẽ nghỉ rồi thử lại, và giữ kết quả đã lấy");
                }
                break;
            }
        };

        let has_more = v.get("has_more").and_then(|x| x.as_i64()).unwrap_or(0);
        let max_cursor = v.get("max_cursor").and_then(|x| x.as_i64()).unwrap_or(0);
        let n = v
            .get("aweme_list")
            .and_then(|x| x.as_array())
            .map(|a| {
                for it in a {
                    if let Some(id) = it.get("aweme_id").and_then(|x| x.as_str()) {
                        seen.insert(id.to_string());
                    }
                }
                a.len()
            })
            .unwrap_or(0);
        println!("[{nhan}] trang {page:3}: n={n:2} tổng={:4} has_more={has_more}", seen.len());
        if has_more != 1 || max_cursor == 0 || max_cursor == cursor {
            break;
        }
        cursor = max_cursor;
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    (seen.len(), pages)
}

#[tokio::test]
#[ignore]
async fn ab_khong_cookie_vs_co_cookie() {
    let sec_uid = std::env::var("BQD_SEC_UID")
        .unwrap_or_else(|_| "MS4wLjABAAAA7yRYvacLzSF5V0J8mrM0eE-PdL3O9_dNdDJTb-0peRw".into());
    let cookie_file = std::env::var("BQD_COOKIE_FILE").expect("cần BQD_COOKIE_FILE");

    // ĐÚNG hàm mà app dùng để đọc cookie của user.
    let raw = std::fs::read_to_string(&cookie_file).expect("đọc file cookie");
    let ck = netscape_to_cookie_header(&raw, "douyin.com").expect("file phải có cookie douyin");
    let co_login = ["sessionid=", "sid_guard=", "sid_tt="]
        .iter()
        .any(|k| ck.contains(k));
    println!(
        "== lib bóc được {} cookie douyin.com | đã đăng nhập: {co_login}",
        ck.split("; ").count()
    );
    assert!(co_login, "cookie của anh Hùng phải là cookie ĐÃ ĐĂNG NHẬP");

    let ttwid = fetch_ttwid().await;
    let anon = ttwid.as_ref().map(|t| format!("ttwid={t}"));
    println!("\n--- A: ĐƯỜNG CŨ (chỉ ttwid ẩn danh, app <= 0.1.139) ---");
    let (a_n, a_p) = crawl(&sec_uid, anon.as_deref(), "CŨ").await;

    println!("\n--- B: ĐƯỜNG MỚI (cookie đăng nhập của user) ---");
    let (b_n, b_p) = crawl(&sec_uid, Some(&ck), "MỚI").await;

    println!("\n===== NGHIỆM THU =====");
    println!("CŨ  (không cookie): {a_n} video / {a_p} trang");
    println!("MỚI (có cookie)   : {b_n} video / {b_p} trang");
    assert!(
        b_n > a_n * 2,
        "bản vá phải lấy được NHIỀU HƠN HẲN: cũ={a_n} mới={b_n}"
    );
}
