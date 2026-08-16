//! PROBE PHÂN TRANG — bò hết kênh Douyin đúng như app đang làm, đếm video,
//! và BẮT ngay trang nào trả về thứ không phải JSON (lỗi anh Hùng gặp).
//!
//! Chạy tay:
//!   cargo test --test douyin_pages -- --ignored --nocapture
//!
//! LUẬT: KHÔNG in cookie/token/sec_uid đầy đủ.

use bqhungdown_lib::douyin_sign::{ABogus, DOUYIN_UA};
use std::collections::HashSet;
use std::time::Duration;

fn mask(s: &str) -> String {
    let head: String = s.chars().take(8).collect();
    format!("{head}…<che, len={}>", s.len())
}

fn build_post_params(sec_uid: &str, max_cursor: i64, count: u32) -> String {
    format!(
        "device_platform=webapp&aid=6383&channel=channel_pc_web&sec_user_id={sec_uid}\
&max_cursor={max_cursor}&locate_query=false&show_live_replay_strategy=1&need_time_list=1\
&time_list_query=0&whale_cut_token=&cut_version=1&count={count}&publish_video_strategy_type=2\
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

#[tokio::test]
#[ignore]
async fn crawl_all_pages() {
    let sec_uid = std::env::var("BQD_SEC_UID")
        .unwrap_or_else(|_| "MS4wLjABAAAA7yRYvacLzSF5V0J8mrM0eE-PdL3O9_dNdDJTb-0peRw".into());
    let count: u32 = std::env::var("BQD_COUNT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(18);
    let sleep_ms: u64 = std::env::var("BQD_SLEEP_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(400);
    println!("== sec_uid {} | count={count} | nghỉ {sleep_ms}ms", mask(&sec_uid));

    let ttwid = fetch_ttwid().await;
    println!("== ttwid: {}", if ttwid.is_some() { "có" } else { "KHÔNG" });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(DOUYIN_UA)
        .build()
        .unwrap();
    let ab = ABogus::new();
    let referer = format!("https://www.douyin.com/user/{sec_uid}");

    let mut seen: HashSet<String> = HashSet::new();
    let mut cursor: i64 = 0;
    let mut total_raw = 0usize;
    let mut dup_total = 0usize;
    let mut author_aweme_count: Option<i64> = None;
    const MAX_PAGES: usize = 120;

    for page in 0..MAX_PAGES {
        let params = build_post_params(&sec_uid, cursor, count);
        let a_bogus = ab.get_value(&params, "GET");
        let url = format!(
            "https://www.douyin.com/aweme/v1/web/aweme/post/?{params}&a_bogus={}",
            pct_encode(&a_bogus)
        );
        let mut req = client.get(&url).header("Referer", &referer);
        if let Some(tt) = &ttwid {
            req = req.header("Cookie", format!("ttwid={tt}"));
        }
        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                println!("!! trang {page}: GỬI THẤT BẠI: {e}");
                break;
            }
        };
        let status = resp.status();
        let ctype = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("?")
            .to_string();
        let body = match resp.text().await {
            Ok(b) => b,
            Err(e) => {
                println!("!! trang {page}: ĐỌC THÂN THẤT BẠI: {e} (app sẽ coi là RỖNG)");
                break;
            }
        };

        if body.trim().is_empty() {
            println!("!! trang {page}: THÂN RỖNG (HTTP {status}, ctype={ctype}) -> app dừng ở đây");
            break;
        }

        let v: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => {
                println!("!! trang {page}: KHÔNG PHẢI JSON: {e}");
                println!("!! HTTP {status} ctype={ctype} len={}", body.len());
                let head: String = body.chars().take(500).collect();
                println!("!! 500 KÝ TỰ ĐẦU ==\n{head}\n!! HẾT");
                break;
            }
        };

        let sc = v.get("status_code").and_then(|x| x.as_i64()).unwrap_or(-1);
        let has_more = v.get("has_more").and_then(|x| x.as_i64()).unwrap_or(0);
        let max_cursor = v.get("max_cursor").and_then(|x| x.as_i64()).unwrap_or(0);
        let list = v.get("aweme_list").and_then(|x| x.as_array());
        let n = list.map(|a| a.len()).unwrap_or(0);
        total_raw += n;

        if author_aweme_count.is_none() {
            if let Some(a) = list.and_then(|l| l.first()) {
                author_aweme_count = a
                    .get("author")
                    .and_then(|au| au.get("aweme_count"))
                    .and_then(|x| x.as_i64());
            }
        }

        let mut dup_here = 0;
        if let Some(arr) = list {
            for a in arr {
                if let Some(id) = a.get("aweme_id").and_then(|x| x.as_str()) {
                    if !seen.insert(id.to_string()) {
                        dup_here += 1;
                    }
                }
            }
        }
        dup_total += dup_here;

        println!(
            "trang {page:3}: sc={sc} n={n:2} trùng={dup_here:2} tổng-duy-nhất={:4} has_more={has_more} cursor={cursor} -> {max_cursor}",
            seen.len()
        );

        if sc != 0 {
            println!("!! status_code = {sc} -> app dừng");
            break;
        }
        if has_more != 1 || max_cursor == 0 || max_cursor == cursor {
            println!(
                "== DỪNG TỰ NHIÊN ở trang {page}: has_more={has_more} max_cursor={max_cursor} (cursor cũ={cursor})"
            );
            break;
        }
        cursor = max_cursor;
        tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
    }

    println!("\n===== KẾT QUẢ =====");
    println!("Video DUY NHẤT lấy được : {}", seen.len());
    println!("Tổng bản ghi thô        : {total_raw}");
    println!("Bản ghi TRÙNG bị bỏ     : {dup_total}");
    match author_aweme_count {
        Some(c) => println!("author.aweme_count (SỐ THẬT Douyin khai): {c}"),
        None => println!("author.aweme_count: không có trong phản hồi"),
    }
}
