//! Tự sinh chữ ký `a_bogus` cho Web API của Douyin (KHÔNG nhúng blob JS lạ).
//!
//! Douyin bắt buộc tham số `a_bogus` khi gọi API (vd lấy danh sách video của
//! một kênh). Thuật toán = SM3 (băm chuẩn TQ) + RC4 + base64 bảng tuỳ biến.
//! Module này viết lại thuật toán bằng Rust thuần (đọc/kiểm tra được), tham
//! khảo cấu trúc từ bản Python GPL của TikTokDownloader / f2 / Evil0ctal.
//!
//! Kiểm chứng: SM3 khớp test vector chuẩn sm3("abc"), RC4 round-trip đúng,
//! và ua_code cứng lấy nguyên từ bản tham chiếu (ứng với `DOUYIN_UA`) — luôn
//! gửi đúng UA đó nên chữ ký khớp phía server.
//!
//! LƯU Ý: Douyin đổi thuật toán vài tháng/lần — khi API trả rỗng dù chữ ký
//! sinh ra, khả năng cao thuật toán đã đổi và cần cập nhật lại module này.

use sm3::{Digest, Sm3};

/// Bảng base64 tuỳ biến "s4" mà Douyin dùng cho bước cuối.
const S4: &[u8] = b"Dkdpgh2ZmsQB80/MfvV36XI1R45-WUAlEixNLwoqYTOPuzKFjJnry79HbGcaStCe";
/// Chuỗi phụ nối vào params/method trước khi băm.
const END_STRING: &str = "cus";

/// User-Agent mà `UA_CODE` được sinh ra từ đó. PHẢI gửi đúng UA này trong
/// HTTP header của mọi request Douyin (nếu không server tính lại từ UA header
/// sẽ lệch với chữ ký → trả rỗng).
pub const DOUYIN_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/90.0.4430.212 Safari/537.36";

/// ua_code cứng ứng với `DOUYIN_UA` (bản tham chiếu ship sẵn). a_bogus chỉ
/// dùng phần tử [23] và [24], nhưng giữ đủ 32 cho khớp thuật toán gốc.
const UA_CODE: [i64; 32] = [
    76, 98, 15, 131, 97, 245, 224, 133, 122, 199, 241, 166, 79, 34, 90, 191, 128, 126, 122, 98, 66,
    11, 14, 40, 49, 110, 110, 173, 67, 96, 138, 252,
];
/// Browser fingerprint mặc định (giữ y bản tham chiếu — combo đã kiểm chứng).
const BROWSER: &str = "1536|742|1536|864|0|0|0|0|1536|864|1536|864|1536|742|24|24|MacIntel";

/// SM3 của `data`, trả 32 byte digest.
fn sm3_bytes(data: &[u8]) -> [u8; 32] {
    let mut h = Sm3::new();
    h.update(data);
    let out = h.finalize();
    let mut r = [0u8; 32];
    r.copy_from_slice(&out);
    r
}

/// SM3 kép: `sm3(sm3(data))` — dùng cho params_code / method_code.
fn sm3_double(data: &[u8]) -> [u8; 32] {
    sm3_bytes(&sm3_bytes(data))
}

/// RC4: nhận plaintext là dãy code-point (có thể >255 sau này), key là byte.
/// Trả dãy code-point (XOR có thể ra >255 khi plaintext >255).
fn rc4(plaintext: &[i64], key: &[u8]) -> Vec<i64> {
    let mut s: Vec<u8> = (0..=255).collect();
    let mut j = 0usize;
    for i in 0..256 {
        j = (j + s[i] as usize + key[i % key.len()] as usize) & 0xff;
        s.swap(i, j);
    }
    let mut out = Vec::with_capacity(plaintext.len());
    let (mut i, mut j) = (0usize, 0usize);
    for &p in plaintext {
        i = (i + 1) & 0xff;
        j = (j + s[i] as usize) & 0xff;
        s.swap(i, j);
        let t = (s[i] as usize + s[j] as usize) & 0xff;
        out.push((s[t] as i64) ^ p);
    }
    out
}

/// random_list gốc — trả 4 byte cuối (s1..s4). `r` là số ngẫu nhiên nền.
fn random_list(r: f64, b: i64, c: i64, d: i64, e: i64, f: i64, g: i64) -> [i64; 4] {
    let ri = r as i64;
    let r255 = ri & 255;
    let r8 = ri >> 8;
    [
        (r255 & b) | d,
        (r255 & c) | e,
        (r8 & b) | f,
        (r8 & c) | g,
    ]
}

fn list_1(r: f64) -> [i64; 4] {
    random_list(r, 170, 85, 1, 2, 5, 45 & 170)
}
fn list_2(r: f64) -> [i64; 4] {
    random_list(r, 170, 85, 1, 0, 0, 0)
}
fn list_3(r: f64) -> [i64; 4] {
    random_list(r, 170, 85, 1, 0, 5, 0)
}

fn end_check_num(a: &[i64]) -> i64 {
    a.iter().fold(0i64, |acc, &x| acc ^ x)
}

/// Bảng vị trí cố định (list_4 gốc) — trả 44 phần tử.
#[allow(clippy::too_many_arguments)]
fn list_4(
    a: i64, b: i64, c: i64, d: i64, e: i64, f: i64, g: i64, h: i64, i: i64, j: i64, k: i64, m: i64,
    n: i64, o: i64, p: i64, q: i64, r: i64,
) -> Vec<i64> {
    vec![
        44, a, 0, 0, 0, 0, 24, b, n, 0, c, d, 0, 0, 0, 1, 0, 239, e, o, f, g, 0, 0, 0, 0, h, 0, 0,
        14, i, j, 0, k, m, 3, p, 1, q, 1, r, 0, 0, 0,
    ]
}

/// base64 bảng tuỳ biến (s4) trên dãy code-point. Các mask giới hạn 24 bit
/// thấp nên chỉ số luôn nằm 0..63 kể cả khi code-point >255.
fn generate_result(s: &[i64]) -> String {
    let n_len = s.len();
    let masks: [(u32, i64); 4] = [
        (18, 0xFC_0000),
        (12, 0x03_F000),
        (6, 0x00_0FC0),
        (0, 0x00_003F),
    ];
    let mut out = String::new();
    let mut i = 0usize;
    while i < n_len {
        let n: i64 = if i + 2 < n_len {
            (s[i] << 16) | (s[i + 1] << 8) | s[i + 2]
        } else if i + 1 < n_len {
            (s[i] << 16) | (s[i + 1] << 8)
        } else {
            s[i] << 16
        };
        for (j, k) in masks {
            if j == 6 && i + 1 >= n_len {
                break;
            }
            if j == 0 && i + 2 >= n_len {
                break;
            }
            let idx = ((n & k) >> j) as usize;
            out.push(S4[idx] as char);
        }
        i += 3;
    }
    let pad = (4 - out.len() % 4) % 4;
    for _ in 0..pad {
        out.push('=');
    }
    out
}

/// Bộ sinh a_bogus. Tạo 1 lần rồi gọi `get_value` cho mỗi request.
pub struct ABogus {
    ua_code: Vec<i64>,
    browser_code: Vec<i64>,
    browser_len: i64,
}

impl Default for ABogus {
    fn default() -> Self {
        Self::new()
    }
}

impl ABogus {
    /// Dùng ua_code cứng ứng với `DOUYIN_UA`. Caller PHẢI gửi `DOUYIN_UA`
    /// làm User-Agent header cho mọi request.
    pub fn new() -> Self {
        Self {
            ua_code: UA_CODE.to_vec(),
            browser_code: BROWSER.bytes().map(|b| b as i64).collect(),
            browser_len: BROWSER.len() as i64,
        }
    }

    fn generate_string_1(&self, r1: f64, r2: f64, r3: f64) -> Vec<i64> {
        let mut v = Vec::with_capacity(12);
        v.extend_from_slice(&list_1(r1));
        v.extend_from_slice(&list_2(r2));
        v.extend_from_slice(&list_3(r3));
        v
    }

    fn generate_string_2(&self, params: &str, method: &str, start: i64, end: i64) -> Vec<i64> {
        let params_arr = sm3_double(format!("{params}{END_STRING}").as_bytes());
        let method_arr = sm3_double(format!("{method}{END_STRING}").as_bytes());
        let pa = |i: usize| params_arr[i] as i64;
        let ma = |i: usize| method_arr[i] as i64;
        let uc = |i: usize| self.ua_code[i];

        let mut a = list_4(
            (end >> 24) & 255,
            pa(21),
            uc(23),
            (end >> 16) & 255,
            pa(22),
            uc(24),
            (end >> 8) & 255,
            end & 255,
            (start >> 24) & 255,
            (start >> 16) & 255,
            (start >> 8) & 255,
            start & 255,
            ma(21),
            ma(22),
            end >> 32,
            start >> 32,
            self.browser_len,
        );
        let e = end_check_num(&a);
        a.extend_from_slice(&self.browser_code);
        a.push(e);
        rc4(&a, b"y")
    }

    /// Sinh a_bogus cho `params` (chuỗi query ĐÚNG như sẽ gửi, chưa gồm
    /// a_bogus). `start`/`end` là mốc thời gian ms; `r1..r3` là số ngẫu nhiên
    /// nền (0..10000). Tách tham số ra để test tất định.
    pub fn get_value_with(
        &self,
        params: &str,
        method: &str,
        start: i64,
        end: i64,
        r1: f64,
        r2: f64,
        r3: f64,
    ) -> String {
        let mut s = self.generate_string_1(r1, r2, r3);
        s.extend(self.generate_string_2(params, method, start, end));
        generate_result(&s)
    }

    /// Bản dùng thật: tự lấy thời gian hiện tại + số ngẫu nhiên.
    pub fn get_value(&self, params: &str, method: &str) -> String {
        let start = now_ms();
        let (r1, r2, r3) = rand_seeds(start);
        let end = start + 4 + (r1 as i64 % 5); // +4..8ms như bản gốc
        self.get_value_with(params, method, start, end, r1, r2, r3)
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 3 số giả-ngẫu nhiên 0..10000 từ seed (xorshift nhẹ — chỉ để làm nhiễu,
/// không cần chất lượng mật mã).
fn rand_seeds(seed: i64) -> (f64, f64, f64) {
    let mut x = (seed as u64) ^ 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x % 10000) as f64
    };
    (next(), next(), next())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SM3 test vector chuẩn: sm3("abc") = 66c7f0f4...
    #[test]
    fn sm3_known_vector() {
        let h = sm3_bytes(b"abc");
        let hex: String = h.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "66c7f0f462eeedd9d1f2d46bdc10e4e24167c4875cf2f7a2297da02b8f4ba8e0"
        );
    }

    /// RC4 tất định: cùng input → cùng output, và giải mã lại ra plaintext.
    #[test]
    fn rc4_roundtrip() {
        let pt: Vec<i64> = b"hello douyin".iter().map(|&b| b as i64).collect();
        let enc = rc4(&pt, b"y");
        let dec = rc4(&enc, b"y");
        assert_eq!(dec, pt);
        assert_ne!(enc, pt);
    }

    /// a_bogus tất định với input cố định — chỉ ký tự bảng s4 + '='.
    #[test]
    fn abogus_deterministic_charset() {
        let ab = ABogus::new();
        let params = "device_platform=webapp&aid=6383&sec_user_id=MS4wLjABAAAAtest&max_cursor=0&count=18";
        let v1 = ab.get_value_with(params, "GET", 1_700_000_000_000, 1_700_000_000_005, 1234.0, 5678.0, 9012.0);
        let v2 = ab.get_value_with(params, "GET", 1_700_000_000_000, 1_700_000_000_005, 1234.0, 5678.0, 9012.0);
        assert_eq!(v1, v2, "cùng input phải cho cùng a_bogus");
        assert!(v1.len() > 100, "a_bogus phải đủ dài: {}", v1.len());
        assert!(
            v1.chars().all(|c| S4.contains(&(c as u8)) || c == '='),
            "chỉ chứa ký tự bảng s4 + '=': {v1}"
        );
    }
}
