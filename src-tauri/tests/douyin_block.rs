//! Tái hiện lỗi "expected value at line 1 column 1" — bắn liên tiếp cho tới khi
//! Douyin trả về thứ KHÔNG phải JSON, rồi in nguyên 500 ký tự đầu (đã che).
//!
//!   cargo test --test douyin_block -- --ignored --nocapture

use bqhungdown_lib::douyin_sign::{ABogus, DOUYIN_UA};
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

/// serde_json nói gì với các thân phản hồi khác nhau — để biết thông báo lỗi
/// anh Hùng thấy ứng với thân nào.
#[test]
fn serde_noi_gi_voi_tung_loai_than() {
    for (nhan, raw) in [
        ("rỗng hoàn toàn", ""),
        ("chỉ khoảng trắng", "   \n"),
        ("HTML", "<!DOCTYPE html><html><head>"),
        ("chữ trần", "blocked"),
        ("JSON hợp lệ", "{\"a\":1}"),
    ] {
        let r = serde_json::from_str::<serde_json::Value>(raw);
        match r {
            Ok(_) => println!("{nhan:20} -> OK"),
            Err(e) => println!("{nhan:20} -> {e}"),
        }
    }
}

#[tokio::test]
#[ignore]
async fn ban_lien_tiep_cho_toi_khi_bi_chan() {
    let sec_uid = std::env::var("BQD_SEC_UID")
        .unwrap_or_else(|_| "MS4wLjABAAAA7yRYvacLzSF5V0J8mrM0eE-PdL3O9_dNdDJTb-0peRw".into());
    let n: usize = std::env::var("BQD_N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(40);

    // KHÔNG cookie, KHÔNG ttwid — đúng kịch bản xấu nhất mà app rơi vào khi
    // fetch_ttwid() trả None (nó là best-effort, hỏng vẫn đi tiếp).
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(DOUYIN_UA)
        .build()
        .unwrap();
    let ab = ABogus::new();
    let referer = format!("https://www.douyin.com/user/{sec_uid}");

    let mut ok = 0usize;
    let mut rong = 0usize;
    for i in 0..n {
        let params = build_post_params(&sec_uid, 0);
        let a_bogus = ab.get_value(&params, "GET");
        let url = format!(
            "https://www.douyin.com/aweme/v1/web/aweme/post/?{params}&a_bogus={}",
            pct_encode(&a_bogus)
        );
        let resp = match client.get(&url).header("Referer", &referer).send().await {
            Ok(r) => r,
            Err(e) => {
                println!("lần {i}: gửi lỗi {e}");
                continue;
            }
        };
        let status = resp.status();
        let ctype = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("?")
            .to_string();
        let body = resp.text().await.unwrap_or_default();

        if body.trim().is_empty() {
            rong += 1;
            println!("lần {i:3}: HTTP {status} ctype={ctype} -> THÂN RỖNG ({rong} lần rỗng)");
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(&body) {
            Ok(v) => {
                ok += 1;
                let cnt = v
                    .get("aweme_list")
                    .and_then(|x| x.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                println!("lần {i:3}: OK n={cnt}");
            }
            Err(e) => {
                println!("\n!!!! TÁI HIỆN ĐƯỢC ở lần {i} !!!!");
                println!("lỗi serde: {e}");
                println!("HTTP {status} ctype={ctype} len={}", body.len());
                let head: String = body.chars().take(500).collect();
                println!("== 500 KÝ TỰ ĐẦU ==\n{head}\n== HẾT ==");
                return;
            }
        }
    }
    println!("\nHết {n} lần: OK={ok}, rỗng={rong}, KHÔNG tái hiện được lỗi không-phải-JSON");
}
