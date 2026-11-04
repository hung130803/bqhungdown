//! DÂY CHUYỀN TỰ ĐỘNG — Đợt 1 (04/11/2026, anh Hùng duyệt).
//!
//! Chỉ THÊM cạnh code cũ, công tắc `Settings.lich_tai_enabled` mặc định TẮT —
//! tắt thì app chạy y như trước. Gồm:
//! - **Lịch tải** (`spawn_scheduler`): đến giờ thì quét LẦN LƯỢT từng nhóm theo
//!   thứ tự anh xếp, gọi đúng `watcher::check_all` mà nút ▶ đang dùng (không
//!   viết lại nghiệp vụ: hạn mức ngày, nguồn dự phòng, cổng live… giữ nguyên).
//! - **Phiếu đợt tải** (INTEGRATION.md mục 11): mỗi nhóm mỗi ngày 1 phiếu ở
//!   `<daychuyen_dir>\dot\<dot_id>.json`. Phiếu chỉ để BÁO CÁO + KÍCH SỚM, không
//!   bao giờ là điều kiện bắt buộc của tool cắt.
//! - **File trạng thái bận** (mục 12) cho PCBot quyết định ngủ đông.
//! - Kiểm tra ổ đĩa trước khi mở đợt, khớp kênh THEO THƯ MỤC (hàm chuẩn hoá chung).
//!
//! Phần logic là HÀM THUẦN (nhận "đồng hồ" + "quan sát hàng đợi" từ ngoài) để
//! test bằng đồng hồ giả và thư mục tạm — xem `mod tests` (ca D1–D21).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, FixedOffset, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use crate::models::{DownloadItem, DownloadState, HistoryEntry, HistoryStatus, Settings};

/// Phiên bản giao kèo phiếu mà app này ghi.
pub const SCHEMA: u32 = 1;
/// Gốc phiếu mặc định (cài đặt `daychuyen_dir` đổi được).
pub const THU_MUC_MAC_DINH: &str = r"C:\ProgramData\BQDayChuyen";
/// Thư mục trao đổi của PCBot — agent SYSTEM đọc qua `safe_exchange_path`.
pub const THU_MUC_EXCHANGE: &str = r"C:\ProgramData\PCBot\exchange";
pub const TEN_FILE_BAN: &str = "ban_bqhungdown.json";
/// Ổ chứa video dưới ngưỡng này thì KHÔNG mở đợt mới (#20).
pub const O_VIDEO_TOI_THIEU_GB: u64 = 30;
/// Ổ C dưới ngưỡng này thì KHÔNG mở đợt mới (#20).
pub const O_C_TOI_THIEU_GB: u64 = 10;
/// Phiếu cũ hơn ngần này ngày thì tự xoá (luật đọc/ghi 6).
const GIU_PHIEU_NGAY: i64 = 14;
const GB: u64 = 1024 * 1024 * 1024;

// ───────────────────────── chuẩn hoá đường dẫn (#12 #13) ─────────────────────────

/// KHOÁ so khớp thư mục kênh giữa 2 tool (luật đọc/ghi 4): Unicode NFC, `/` →
/// `\`, chữ thường, bỏ `\` cuối. Đo thật: DB tool cắt lưu `D:/video ssmatool/…`
/// còn tool tải lưu `D:\video ssmatool\…` — so nguyên chuỗi lệch 154/154 kênh.
/// Ca mẫu dùng CHUNG với V18 của BQ Hung Video (test D18).
pub fn chuan_hoa_duong_dan(p: &str) -> String {
    let mut s = nfc(p).replace('/', "\\").to_lowercase();
    while s.ends_with('\\') {
        s.pop();
    }
    s
}

/// Unicode NFC qua `NormalizeString` của Windows (không thêm thư viện ngoài).
#[cfg(windows)]
fn nfc(s: &str) -> String {
    #[link(name = "normaliz")]
    extern "system" {
        fn NormalizeString(form: i32, src: *const u16, src_len: i32, dst: *mut u16, dst_len: i32) -> i32;
    }
    const NORMALIZATION_C: i32 = 1;
    if s.is_ascii() {
        return s.to_string();
    }
    let src: Vec<u16> = s.encode_utf16().collect();
    // SAFETY: src sống suốt lời gọi; dst = null + độ dài 0 chỉ hỏi kích thước.
    let mut cap = unsafe { NormalizeString(NORMALIZATION_C, src.as_ptr(), src.len() as i32, std::ptr::null_mut(), 0) };
    for _ in 0..4 {
        if cap <= 0 {
            break;
        }
        let mut dst = vec![0u16; cap as usize];
        // SAFETY: dst có đúng `cap` phần tử.
        let n = unsafe { NormalizeString(NORMALIZATION_C, src.as_ptr(), src.len() as i32, dst.as_mut_ptr(), cap) };
        if n > 0 {
            dst.truncate(n as usize);
            return String::from_utf16_lossy(&dst);
        }
        // Thiếu chỗ: trả về số ÂM = ước lượng mới.
        cap = -n;
    }
    s.to_string()
}

#[cfg(not(windows))]
fn nfc(s: &str) -> String {
    s.to_string()
}

// ───────────────────────── ghi nguyên tử (luật đọc/ghi 1) ─────────────────────────

/// File tạm cạnh file thật: `<tên>.json.tmp` — KHÔNG mang đuôi video / `.part`
/// (bộ dọn rác 2 phút/lần xoá mấy đuôi đó).
pub fn tmp_cua(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!("{name}.tmp"))
}

/// Ghi tmp → flush xuống đĩa → đổi tên đè. Đổi tên vướng khoá (tool cắt đang
/// đọc phiếu) thì thử lại 5 lần × 200 ms. Không bao giờ ghi thẳng vào file thật.
pub fn ghi_nguyen_tu(path: &Path, data: &[u8]) -> std::io::Result<()> {
    ghi_nguyen_tu_voi(path, data, 5, Duration::from_millis(200))
}

fn ghi_nguyen_tu_voi(path: &Path, data: &[u8], thu_lai: u32, nghi: Duration) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = tmp_cua(path);
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    let mut lan = 0;
    loop {
        match std::fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) if lan < thu_lai => {
                let _ = e;
                lan += 1;
                std::thread::sleep(nghi);
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(e);
            }
        }
    }
}

/// Tên phiếu hợp lệ: `YYYYMMDD-NN.json` — chỉ dọn/xoá đúng mẫu này.
fn la_ten_phieu(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".json") else { return false };
    let b = stem.as_bytes();
    b.len() == 11 && b[8] == b'-' && b[..8].iter().chain(&b[9..]).all(u8::is_ascii_digit)
}

/// Mở lại sau khi bị giết giữa lúc ghi: xoá `.json.tmp` mồ côi (đúng mẫu tên
/// phiếu). File phiếu thật không bị đụng. Trả số file đã dọn.
pub fn don_tmp_mo_coi(dir: &Path) -> usize {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    rd.flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.strip_suffix(".tmp").is_some_and(la_ten_phieu)
        })
        .filter(|e| std::fs::remove_file(e.path()).is_ok())
        .count()
}

/// Xoá phiếu CỦA MÌNH cũ hơn 14 ngày (theo ngày trong tên), chỉ đúng mẫu tên.
pub fn don_phieu_cu(dir: &Path, hom_nay: chrono::NaiveDate) -> usize {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    rd.flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            la_ten_phieu(&n)
                && chrono::NaiveDate::parse_from_str(&n[..8], "%Y%m%d")
                    .is_ok_and(|d| (hom_nay - d).num_days() > GIU_PHIEU_NGAY)
        })
        .filter(|e| std::fs::remove_file(e.path()).is_ok())
        .count()
}

// ───────────────────────── phiếu đợt tải (mục 11) ─────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoPhieu {
    pub id: String,
    pub trang_thai: String,
    pub file: Option<String>,
    pub size: Option<u64>,
    pub xong_luc: Option<String>,
    pub loi: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KenhPhieu {
    pub thu_muc: String,
    pub ten_dich: String,
    pub video: Vec<VideoPhieu>,
}

/// Đúng các trường của giao kèo — file `dot\<dot_id>.json` là bản serialize này.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhieuTai {
    pub schema: u32,
    pub dot_id: String,
    pub nhom: String,
    pub seq: u64,
    pub mo_luc: String,
    pub han_chot: String,
    pub trang_thai: String,
    pub dong_luc: Option<String>,
    pub ly_do_dong: Option<String>,
    pub kenh: Vec<KenhPhieu>,
    pub kenh_khong_co_video: Vec<String>,
}

/// 1 job của đợt — id lấy THẲNG từ đường enqueue của đợt (không đoán theo hàng).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobRef {
    pub job: String,
    pub kenh: usize,
    pub video: usize,
    /// Đã có kết quả CUỐI (xong / bỏ qua / lỗi vĩnh viễn) — không đổi nữa.
    #[serde(default)]
    pub cuoi: bool,
}

/// Phiếu + phần app tự giữ (danh sách id job, đã quét xong nhóm chưa). Lưu ở
/// `<config>/lich_tai.json` — phiếu ngoài đĩa chỉ là bản xuất ra cho tool cắt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DotNoiBo {
    pub phieu: PhieuTai,
    /// Tên nhóm GỐC truyền cho `check_all` ("" = Chưa phân nhóm).
    pub nhom_goc: String,
    pub jobs: Vec<JobRef>,
    pub quet_xong: bool,
    /// Phiếu ngoài đĩa CHƯA khớp bản trong sổ (vừa đổi, hoặc lần ghi trước
    /// lỗi sau 5 lần thử) → nhịp sau ghi lại, kể cả phiếu đã đóng.
    #[serde(default)]
    pub can_ghi: bool,
}

impl DotNoiBo {
    pub fn dang_mo(&self) -> bool {
        self.phieu.trang_thai == "dang_tai"
    }
}

/// Mục đợt vừa xếp hàng — watcher đẩy vào `JobSink` ngay tại `enqueue_new`.
#[derive(Debug, Clone, PartialEq)]
pub struct EnqueuedJob {
    pub job_id: String,
    /// `<extractor>:<id video>` (vd `youtube:abc123`).
    pub video_id: String,
    pub folder: PathBuf,
    pub ten_dich: String,
}

pub type JobSink = Arc<Mutex<Vec<EnqueuedJob>>>;

/// Trạng thái của 1 job nhìn từ hàng đợi + lịch sử (đầu vào thuần của phiếu).
#[derive(Debug, Clone, PartialEq)]
pub enum QuanSat {
    Cho,
    DangTai,
    Xong { path: Option<PathBuf>, luc: Option<DateTime<Utc>> },
    BoQua(String),
    Loi { msg: String, cuoi: bool },
    /// Không còn trong hàng lẫn lịch sử (đã xoá / huỷ rồi bị bỏ khi mở lại app).
    KhongThay,
}

/// Quy đổi mục hàng đợi/lịch sử → trạng thái phiếu. Hàng chỉ giữ 300 mục đã
/// xong và bỏ mục xong khi mở lại app → luôn tra thêm lịch sử. Lịch sử ghi
/// Completed SAU bước đổi tên Douyin nên `output_path` là TÊN CUỐI.
/// "Failed" chưa phải cuối cùng (người dùng có thể Thử lại) — trừ lỗi vĩnh viễn.
pub fn quan_sat_job(item: Option<&DownloadItem>, hist: Option<&HistoryEntry>) -> QuanSat {
    if let Some(it) = item.filter(|it| it.state == DownloadState::Completed) {
        return QuanSat::Xong { path: it.output_path.clone(), luc: it.finished_at };
    }
    if let Some(h) = hist.filter(|h| h.status == HistoryStatus::Completed) {
        return QuanSat::Xong { path: h.output_path.clone(), luc: Some(h.finished_at) };
    }
    let Some(it) = item else { return QuanSat::KhongThay };
    let msg = it.error_message.clone().unwrap_or_default();
    match it.state {
        DownloadState::Queued | DownloadState::Paused => QuanSat::Cho,
        DownloadState::Downloading => QuanSat::DangTai,
        DownloadState::Completed => unreachable!(),
        DownloadState::Skipped if msg == crate::queue::LIVE_SKIP_MESSAGE => {
            QuanSat::BoQua("live / chờ live / phát lại live".into())
        }
        DownloadState::Skipped => QuanSat::BoQua("đã tải trước đó (sổ đã tải)".into()),
        DownloadState::Cancelled => QuanSat::Loi { msg: "đã huỷ".into(), cuoi: true },
        DownloadState::Failed if crate::error::is_age_restricted_error(&msg) => {
            QuanSat::BoQua("18+ (cần cookie đăng nhập)".into())
        }
        DownloadState::Failed => {
            let cuoi = crate::error::is_permanent_fail(&msg);
            QuanSat::Loi { msg: if msg.is_empty() { "tải lỗi".into() } else { msg }, cuoi }
        }
    }
}

pub fn iso(t: DateTime<FixedOffset>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Secs, false)
}

fn iso_utc(t: DateTime<Utc>, off: FixedOffset) -> String {
    iso(t.with_timezone(&off))
}

/// `dot_id` mới cho ngày `yyyymmdd`: NN = lớn nhất đã có trong ngày + 1.
pub fn dot_id_moi(yyyymmdd: &str, da_co: &[String]) -> String {
    let max = da_co
        .iter()
        .filter_map(|d| d.strip_prefix(yyyymmdd)?.strip_prefix('-')?.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    format!("{yyyymmdd}-{:02}", max + 1)
}

pub fn ten_nhom_hien_thi(nhom: &str) -> String {
    if nhom.is_empty() { "Chưa phân nhóm".into() } else { nhom.to_string() }
}

/// Mở đợt mới cho 1 nhóm. `han_chot` = mở + `han_gio` giờ (mặc định 3, #16).
pub fn mo_dot(dot_id: String, nhom: &str, now: DateTime<FixedOffset>, han_gio: u32) -> DotNoiBo {
    let han = now + chrono::Duration::hours(han_gio.clamp(1, 24) as i64);
    DotNoiBo {
        phieu: PhieuTai {
            schema: SCHEMA,
            dot_id,
            nhom: ten_nhom_hien_thi(nhom),
            seq: 1,
            mo_luc: iso(now),
            han_chot: iso(han),
            trang_thai: "dang_tai".into(),
            dong_luc: None,
            ly_do_dong: None,
            kenh: Vec::new(),
            kenh_khong_co_video: Vec::new(),
        },
        nhom_goc: nhom.to_string(),
        jobs: Vec::new(),
        quet_xong: false,
        can_ghi: true,
    }
}

/// Gắn job vừa xếp hàng vào phiếu — kênh khớp THEO THƯ MỤC đã chuẩn hoá (2
/// nguồn dự phòng cùng thư mục đích = 1 dòng kênh). Job/video đã có thì bỏ
/// qua (mở lại app quét nối nhóm dở không ghi 2 lần). Trả `true` khi có thêm.
pub fn them_job(dot: &mut DotNoiBo, j: &EnqueuedJob) -> bool {
    if !dot.dang_mo() || dot.jobs.iter().any(|r| r.job == j.job_id) {
        return false;
    }
    let thu_muc = j.folder.to_string_lossy().into_owned();
    let khoa = chuan_hoa_duong_dan(&thu_muc);
    let k = match dot.phieu.kenh.iter().position(|k| chuan_hoa_duong_dan(&k.thu_muc) == khoa) {
        Some(i) => i,
        None => {
            dot.phieu.kenh.push(KenhPhieu { thu_muc, ten_dich: j.ten_dich.clone(), video: Vec::new() });
            dot.phieu.kenh.len() - 1
        }
    };
    if dot.phieu.kenh[k].video.iter().any(|v| v.id == j.video_id) {
        return false;
    }
    dot.phieu.kenh[k].video.push(VideoPhieu {
        id: j.video_id.clone(),
        trang_thai: "cho".into(),
        file: None,
        size: None,
        xong_luc: None,
        loi: None,
    });
    let v = dot.phieu.kenh[k].video.len() - 1;
    dot.jobs.push(JobRef { job: j.job_id.clone(), kenh: k, video: v, cuoi: false });
    dot.phieu.kenh_khong_co_video.retain(|t| chuan_hoa_duong_dan(t) != khoa);
    dot.phieu.seq += 1;
    dot.can_ghi = true;
    true
}

/// Quét nhóm xong: kênh của nhóm không có video nào lượt này → `kenh_khong_co_video`.
pub fn chot_quet(dot: &mut DotNoiBo, thu_muc_nhom: &[String]) {
    if !dot.dang_mo() {
        return;
    }
    let co: std::collections::HashSet<String> =
        dot.phieu.kenh.iter().map(|k| chuan_hoa_duong_dan(&k.thu_muc)).collect();
    let mut list: Vec<String> = Vec::new();
    let mut thay = std::collections::HashSet::new();
    for t in thu_muc_nhom {
        let k = chuan_hoa_duong_dan(t);
        if !co.contains(&k) && thay.insert(k) {
            list.push(t.clone());
        }
    }
    dot.phieu.kenh_khong_co_video = list;
    dot.quet_xong = true;
    dot.phieu.seq += 1;
    dot.can_ghi = true;
}

/// Cập nhật phiếu theo hàng đợi + đóng phiếu khi đủ điều kiện. Phiếu ĐÃ ĐÓNG
/// không bao giờ sửa/mở lại. Trả `true` khi phiếu đổi (cần ghi lại ra đĩa).
pub fn cap_nhat_dot(
    dot: &mut DotNoiBo,
    quan_sat: &dyn Fn(&str) -> QuanSat,
    kich_thuoc: &dyn Fn(&Path) -> Option<u64>,
    now: DateTime<FixedOffset>,
) -> bool {
    if !dot.dang_mo() {
        return false;
    }
    let off = *now.offset();
    let truoc = dot.phieu.clone();
    for r in dot.jobs.iter_mut() {
        if r.cuoi {
            continue;
        }
        let Some(v) = dot.phieu.kenh.get_mut(r.kenh).and_then(|k| k.video.get_mut(r.video)) else {
            continue;
        };
        match quan_sat(&r.job) {
            QuanSat::Cho => v.trang_thai = "cho".into(),
            QuanSat::DangTai => v.trang_thai = "dang_tai".into(),
            // `xong` CHỈ khi đã biết tên file cuối (Douyin đổi tên xong mới
            // có) — chưa có thì vẫn là đang tải, nhịp sau xét lại.
            QuanSat::Xong { path: None, .. } => v.trang_thai = "dang_tai".into(),
            QuanSat::Xong { path, luc } => {
                v.trang_thai = "xong".into();
                v.file = path.as_deref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned());
                v.size = path.as_deref().and_then(kich_thuoc);
                v.xong_luc = Some(luc.map(|t| iso_utc(t, off)).unwrap_or_else(|| iso(now)));
                v.loi = None;
                r.cuoi = true;
            }
            QuanSat::BoQua(ly_do) => {
                v.trang_thai = "bo_qua".into();
                v.loi = Some(ly_do);
                r.cuoi = true;
            }
            QuanSat::Loi { msg, cuoi } => {
                v.trang_thai = "loi".into();
                v.loi = Some(msg);
                r.cuoi = cuoi;
            }
            QuanSat::KhongThay => {
                v.trang_thai = "loi".into();
                v.loi = Some("không còn trong hàng đợi (đã xoá/huỷ)".into());
                r.cuoi = true;
            }
        }
    }
    let qua_han = DateTime::parse_from_rfc3339(&dot.phieu.han_chot).is_ok_and(|h| now >= h);
    if qua_han {
        // Video chưa xong ghi `tre`: vẫn tải tiếp, xong thì tool cắt vẫn cắt
        // (theo quét thư mục) — chỉ phiếu này không đổi nữa.
        for k in dot.phieu.kenh.iter_mut() {
            for v in k.video.iter_mut() {
                if v.trang_thai == "cho" || v.trang_thai == "dang_tai" {
                    v.trang_thai = "tre".into();
                }
            }
        }
        dong(dot, "het_han", now);
    } else if dot.quet_xong && dot.jobs.iter().all(|r| r.cuoi) {
        dong(dot, "xong", now);
    }
    if dot.phieu != truoc {
        dot.phieu.seq = truoc.seq + 1;
        dot.can_ghi = true;
        true
    } else {
        false
    }
}

fn dong(dot: &mut DotNoiBo, ly_do: &str, now: DateTime<FixedOffset>) {
    dong_phieu(&mut dot.phieu, ly_do, now);
}

fn dong_phieu(p: &mut PhieuTai, ly_do: &str, now: DateTime<FixedOffset>) {
    p.trang_thai = "da_dong".into();
    p.dong_luc = Some(iso(now));
    p.ly_do_dong = Some(ly_do.into());
}

/// Phiếu MỒ CÔI: còn `dang_tai` trên đĩa mà sổ `lich_tai.json` không biết (sổ
/// hỏng/mất) → không ai cập nhật nữa, tool cắt sẽ chờ mãi. Đóng `het_han`,
/// video chưa có kết quả ghi `tre`. Chỉ đụng file đúng mẫu tên. Trả số phiếu đã đóng.
pub fn dong_phieu_mo_coi(dir: &Path, biet: &[String], now: DateTime<FixedOffset>) -> usize {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    let mut n = 0;
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !la_ten_phieu(&name) || biet.iter().any(|b| format!("{b}.json") == name) {
            continue;
        }
        let Some(mut p) = std::fs::read(e.path()).ok().and_then(|b| serde_json::from_slice::<PhieuTai>(&b).ok()) else {
            continue; // đọc không ra thì để nguyên, không ghi đè thứ mình không hiểu
        };
        if p.trang_thai != "dang_tai" || p.schema > SCHEMA {
            continue;
        }
        for k in p.kenh.iter_mut() {
            for v in k.video.iter_mut() {
                if v.trang_thai == "cho" || v.trang_thai == "dang_tai" {
                    v.trang_thai = "tre".into();
                }
            }
        }
        dong_phieu(&mut p, "het_han", now);
        p.seq += 1;
        if ghi_nguyen_tu(&e.path(), &serde_json::to_vec_pretty(&p).unwrap_or_default()).is_ok() {
            n += 1;
        }
    }
    n
}

// ───────────────────────── lịch tải (#4) ─────────────────────────

/// Cảnh báo cho bot Telegram đọc qua `/api/daychuyen` (mỗi `id` báo 1 lần).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanhBao {
    pub id: String,
    pub luc: String,
    pub text: String,
}

/// "Đã chạy hôm nay" + các đợt — lưu `<config>/lich_tai.json`, bền qua restart.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TrangThaiLich {
    /// Ngày (YYYY-MM-DD giờ máy) của lượt Lịch tải gần nhất.
    pub ngay: Option<String>,
    /// Nhóm ĐÃ QUÉT XONG trong `ngay` — mở lại app không quét lại, không cấp thêm suất.
    pub nhom_da_quet: Vec<String>,
    /// Đã làm hết danh sách nhóm của `ngay`.
    pub xong_ngay: bool,
    pub dot: Vec<DotNoiBo>,
    pub canh_bao: Vec<CanhBao>,
    /// Lý do đang KHÔNG mở đợt (ổ đầy…) — ghi vào file trạng thái bận.
    pub ly_do_chan: Option<String>,
}

fn parse_gio(gio: &str) -> Option<chrono::NaiveTime> {
    chrono::NaiveTime::parse_from_str(gio.trim(), "%H:%M").ok()
}

pub fn gio_hop_le(gio: &str) -> bool {
    parse_gio(gio).is_some()
}

/// Đến lúc chạy Lịch tải chưa: qua giờ bắt đầu của HÔM NAY và hôm nay chưa
/// làm xong. Mở app sau giờ bắt đầu (06:40, lịch 06:00) → chạy ngay (D2).
pub fn den_gio_chay(now: DateTime<FixedOffset>, gio: &str, st: &TrangThaiLich) -> bool {
    let Some(t) = parse_gio(gio) else { return false };
    let hom_nay = now.format("%Y-%m-%d").to_string();
    if st.ngay.as_deref() == Some(hom_nay.as_str()) && st.xong_ngay {
        return false;
    }
    now.time() >= t
}

/// Sang ngày mới thì xoá sổ "đã quét" của ngày cũ (đợt cũ còn mở vẫn theo dõi
/// tiếp tới khi đóng). Đợt đã đóng từ trước hôm qua thì bỏ khỏi sổ nội bộ
/// (file phiếu trên đĩa vẫn giữ 14 ngày).
pub fn vao_ngay(st: &mut TrangThaiLich, now: DateTime<FixedOffset>) {
    let hom_nay = now.format("%Y-%m-%d").to_string();
    if st.ngay.as_deref() != Some(hom_nay.as_str()) {
        st.ngay = Some(hom_nay);
        st.nhom_da_quet.clear();
        st.xong_ngay = false;
        st.ly_do_chan = None;
    }
    let hom_qua = (now - chrono::Duration::days(1)).format("%Y%m%d").to_string();
    st.dot.retain(|d| d.dang_mo() || d.phieu.dot_id[..8] >= *hom_qua);
    let cut = now - chrono::Duration::days(2);
    st.canh_bao.retain(|c| DateTime::parse_from_rfc3339(&c.luc).is_ok_and(|t| t >= cut));
}

/// Nhóm còn phải quét hôm nay, ĐÚNG thứ tự anh xếp (bỏ nhóm đã quét xong).
pub fn nhom_can_quet(thu_tu: &[String], da_quet: &[String]) -> Vec<String> {
    thu_tu.iter().filter(|g| !da_quet.contains(g)).cloned().collect()
}

/// Đợt HÔM NAY của nhóm (đang mở) — có thì nối tiếp, không mở đợt mới (D3, D15).
pub fn tim_dot_hom_nay<'a>(st: &'a mut TrangThaiLich, nhom: &str, now: DateTime<FixedOffset>) -> Option<&'a mut DotNoiBo> {
    let ngay = now.format("%Y%m%d").to_string();
    st.dot.iter_mut().find(|d| d.nhom_goc == nhom && d.phieu.dot_id.starts_with(&ngay) && d.dang_mo())
}

/// Nhịp quét cũ (`watch_auto_enabled`) chỉ chạy khi Lịch tải TẮT — không chạy
/// cả hai (D4). KHÔNG sửa giá trị `watch_auto_enabled` của anh.
pub fn nhip_cu_duoc_chay(s: &Settings) -> bool {
    s.watch_auto_enabled && !s.lich_tai_enabled
}

/// Nhịp quét cũ: GIỮ đúng khoảng cũ 1–1440 phút (tắt Lịch = y như cũ).
pub fn nhip_cu_phut(s: &Settings) -> u32 {
    s.watch_interval_min.clamp(1, 1440)
}

/// #25 chỉ là KHUYẾN NGHỊ: nhịp cũ đang bật mà < 30 phút thì cảnh báo (155
/// kênh mỗi phút dễ bị YouTube chặn bot), không ép.
pub fn nhip_cu_qua_day(s: &Settings) -> bool {
    nhip_cu_duoc_chay(s) && s.watch_interval_min < 30
}

// ───────────────────────── ổ đĩa (#20) ─────────────────────────

/// Gốc ổ của đường dẫn (`D:\`), hoặc None nếu không xác định được.
pub fn goc_o(p: &Path) -> Option<PathBuf> {
    use std::path::Component;
    let mut c = p.components();
    match c.next()? {
        Component::Prefix(pre) => {
            let mut g = PathBuf::from(pre.as_os_str());
            g.push(std::path::MAIN_SEPARATOR_STR);
            Some(g)
        }
        _ => None,
    }
}

/// Trước khi mở đợt: ổ chứa video dưới 30 GB hoặc ổ C dưới 10 GB → trả lý do
/// (không mở đợt, báo Telegram). `trong` = dung lượng trống (byte) của 1 ổ.
pub fn kiem_tra_o_dia(thu_muc: &[PathBuf], trong: &dyn Fn(&Path) -> Option<u64>) -> Option<String> {
    let mut can: Vec<(PathBuf, u64)> = Vec::new();
    let mut dat = |g: PathBuf, gb: u64| {
        let k = chuan_hoa_duong_dan(&g.to_string_lossy());
        match can.iter_mut().find(|(x, _)| chuan_hoa_duong_dan(&x.to_string_lossy()) == k) {
            Some((_, n)) => *n = (*n).max(gb),
            None => can.push((g, gb)),
        }
    };
    dat(PathBuf::from("C:\\"), O_C_TOI_THIEU_GB);
    for t in thu_muc {
        if let Some(g) = goc_o(t) {
            dat(g, O_VIDEO_TOI_THIEU_GB);
        }
    }
    let mut thieu = Vec::new();
    for (g, gb) in &can {
        if let Some(b) = trong(g) {
            if b < gb * GB {
                thieu.push(format!("ổ {} chỉ còn {:.1} GB (cần ≥ {} GB)", g.display(), b as f64 / GB as f64, gb));
            }
        }
    }
    (!thieu.is_empty()).then(|| format!("Không mở đợt tải: {}", thieu.join("; ")))
}

#[cfg(windows)]
pub fn dung_luong_trong(p: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetDiskFreeSpaceExW(dir: *const u16, free_caller: *mut u64, total: *mut u64, total_free: *mut u64) -> i32;
    }
    let w: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
    let (mut a, mut b, mut c) = (0u64, 0u64, 0u64);
    // SAFETY: w kết thúc NUL, 3 con trỏ trỏ vào biến sống trên stack.
    let ok = unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut a, &mut b, &mut c) };
    (ok != 0).then_some(a)
}

#[cfg(not(windows))]
pub fn dung_luong_trong(_p: &Path) -> Option<u64> {
    None
}

// ───────────────────────── file trạng thái bận (mục 12) ─────────────────────────

/// Nội dung `ban_bqhungdown.json`. Bận khi còn mục chờ/đang tải/tạm dừng hoặc
/// còn đợt đang mở (hoặc Lịch đang quét).
pub fn trang_thai_ban(
    items: &[DownloadItem],
    co_dot_mo: bool,
    ly_do: Option<String>,
    now: DateTime<FixedOffset>,
    phien_ban: &str,
) -> serde_json::Value {
    let cho = items.iter().filter(|i| matches!(i.state, DownloadState::Queued | DownloadState::Paused)).count();
    let dang_lam = items.iter().filter(|i| i.state == DownloadState::Downloading).count();
    serde_json::json!({
        "app": "bqhungdown",
        "ban": cho + dang_lam > 0 || co_dot_mo,
        "cho": cho,
        "dang_lam": dang_lam,
        "cap_nhat": iso(now),
        "pid": std::process::id(),
        "phien_ban": phien_ban,
        "ly_do": ly_do,
    })
}

/// Ghi file bận vào `dir`. Thư mục không tồn tại (chưa cài PCBot) hoặc lỗi
/// quyền ghi → bỏ qua im lặng, ghi log 1 lần; KHÔNG tạo thư mục của PCBot.
pub fn ghi_file_ban(dir: &Path, v: &serde_json::Value) -> bool {
    if !dir.is_dir() {
        log_mot_lan("ban-thieu-thu-muc", &format!("[daychuyen] không có thư mục {} — bỏ qua file trạng thái bận", dir.display()));
        return false;
    }
    let data = serde_json::to_vec(v).unwrap_or_default();
    match ghi_nguyen_tu(&dir.join(TEN_FILE_BAN), &data) {
        Ok(()) => true,
        Err(e) => {
            log_mot_lan("ban-loi-ghi", &format!("[daychuyen] không ghi được file trạng thái bận: {e}"));
            false
        }
    }
}

static DA_LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn log_mot_lan(khoa: &str, msg: &str) {
    let mut g = DA_LOG.lock().unwrap_or_else(|e| e.into_inner());
    if !g.iter().any(|k| k == khoa) {
        g.push(khoa.to_string());
        eprintln!("{msg}");
    }
}

// ───────────────────────── phần chạy thật (cần AppHandle) ─────────────────────────

/// Đợt Lịch đang quét (hiện cho nút ▶ "Chạy tất cả" + bot, #29).
static DOT_DANG_CHAY: Mutex<Option<String>> = Mutex::new(None);

pub fn dot_dang_chay() -> Option<String> {
    DOT_DANG_CHAY.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn dat_dot_dang_chay(v: Option<String>) {
    *DOT_DANG_CHAY.lock().unwrap_or_else(|e| e.into_inner()) = v;
}

/// Cờ chặn quét tay LIỀN MẠCH suốt 1 lượt Lịch (từ lúc kiểm ổ tới hết nhóm
/// cuối) — không có khe "None" giữa các bước. Rơi guard (kể cả return sớm) là gỡ.
pub struct CoChay;

impl CoChay {
    pub fn bat(nhan: String) -> Self {
        dat_dot_dang_chay(Some(nhan));
        CoChay
    }
    pub fn doi(&self, nhan: String) {
        dat_dot_dang_chay(Some(nhan));
    }
}

impl Drop for CoChay {
    fn drop(&mut self) {
        dat_dot_dang_chay(None);
    }
}

/// Lời báo khi bấm ▶ Chạy tất cả đúng lúc Lịch tải đang quét (#29).
pub fn bao_dang_chay(dot: &str) -> String {
    format!("Lịch tải đang chạy đợt {dot} — chờ đợt quét xong rồi bấm lại (tránh 2 lượt quét chồng nhau).")
}

pub fn thu_muc_phieu(s: &Settings) -> PathBuf {
    let goc = s.daychuyen_dir.as_deref().map(str::trim).filter(|d| !d.is_empty()).unwrap_or(THU_MUC_MAC_DINH);
    PathBuf::from(goc).join("dot")
}

pub struct DayChuyen {
    state_path: PathBuf,
    st: Mutex<TrangThaiLich>,
    /// Sink của nhóm đang quét — nhịp cập nhật rút job ra phiếu NGAY (kích sớm).
    sink: Mutex<Option<(String, JobSink)>>,
    dang_quet: std::sync::atomic::AtomicBool,
    /// Ổ đầy → chưa kiểm tra lại trước mốc này (10 phút/lần, khỏi ghi sổ mỗi nhịp).
    kiem_o_lai_luc: Mutex<Option<std::time::Instant>>,
    /// Sổ `lich_tai.json` đổi, chưa ghi xuống đĩa.
    st_doi: std::sync::atomic::AtomicBool,
    /// Tuần tự hoá MỌI lần ghi đĩa (phiếu, sổ, dọn rác) — 2 nhịp không ghi chồng
    /// cùng file tạm. Chỉ giữ ở luồng chặn (`spawn_blocking`); khoá trạng thái
    /// `st` KHÔNG BAO GIỜ bị giữ trong lúc chờ đổi tên file.
    ghi: Mutex<()>,
}

impl DayChuyen {
    pub fn load(config_dir: &Path) -> Arc<Self> {
        let state_path = config_dir.join("lich_tai.json");
        let st = match std::fs::read_to_string(&state_path) {
            Err(_) => TrangThaiLich::default(),
            Ok(t) => serde_json::from_str(&t).unwrap_or_else(|e| {
                // Sổ hỏng: CẤT bản hỏng để xem lại, chạy tiếp với sổ rỗng.
                // Phiếu còn `dang_tai` trên đĩa mà sổ không biết sẽ được
                // `dong_phieu_mo_coi` đóng ở lượt dọn đầu tiên.
                eprintln!("[daychuyen] lich_tai.json hỏng ({e}) — cất sang lich_tai.json.hong");
                let _ = std::fs::rename(&state_path, state_path.with_extension("json.hong"));
                TrangThaiLich::default()
            }),
        };
        Arc::new(Self {
            state_path,
            st: Mutex::new(st),
            sink: Mutex::new(None),
            dang_quet: std::sync::atomic::AtomicBool::new(false),
            kiem_o_lai_luc: Mutex::new(None),
            st_doi: std::sync::atomic::AtomicBool::new(false),
            ghi: Mutex::new(()),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, TrangThaiLich> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn snapshot(&self) -> TrangThaiLich {
        self.lock().clone()
    }

    fn danh_dau_doi(&self) {
        self.st_doi.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn co_dot_mo(&self) -> bool {
        self.dang_quet.load(std::sync::atomic::Ordering::SeqCst) || self.lock().dot.iter().any(DotNoiBo::dang_mo)
    }

    /// Rút job mới từ sink, đối chiếu hàng đợi, đóng phiếu (xong / quá hạn).
    /// KHÔNG phụ thuộc công tắc Lịch: tắt Lịch giữa chừng thì phiếu đang mở
    /// vẫn được cập nhật và tự đóng `het_han`. Không ghi đĩa ở đây.
    pub fn cap_nhat_voi(
        &self,
        qs: &dyn Fn(&str) -> QuanSat,
        size: &dyn Fn(&Path) -> Option<u64>,
        now: DateTime<FixedOffset>,
    ) {
        let moi: Option<(String, Vec<EnqueuedJob>)> = self
            .sink
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|(id, sink)| (id.clone(), std::mem::take(&mut *sink.lock().unwrap_or_else(|e| e.into_inner()))));
        let mut st = self.lock();
        if st.dot.iter().all(|d| !d.dang_mo()) && moi.as_ref().is_none_or(|(_, j)| j.is_empty()) {
            return;
        }
        let truoc = st.dot.clone();
        for d in st.dot.iter_mut() {
            if let Some((id, jobs)) = &moi {
                if &d.phieu.dot_id == id {
                    for j in jobs {
                        them_job(d, j);
                    }
                }
            }
            cap_nhat_dot(d, qs, size, now);
        }
        if st.dot != truoc {
            self.danh_dau_doi();
        }
    }

    fn cap_nhat(&self, queue: &crate::queue::QueueManager, history: &crate::history_store::HistoryStore) {
        let qs = |job: &str| quan_sat_job(queue.get(job).as_ref(), history.get(job).ok().flatten().as_ref());
        let size = |p: &Path| std::fs::metadata(p).ok().map(|m| m.len());
        self.cap_nhat_voi(&qs, &size, chrono::Local::now().fixed_offset());
    }

    /// Ghi mọi phiếu `can_ghi` + sổ (nếu đổi). Phiếu ghi lỗi sau 5 lần thử thì
    /// GIỮ cờ `can_ghi` → nhịp sau ghi lại (cả lần ghi ĐÓNG phiếu). Gọi ở luồng
    /// chặn: có thể chờ tool cắt nhả file. Trả số phiếu còn chưa ghi được.
    pub fn ghi_xuong_dia(&self, dir: &Path) -> usize {
        let _g = self.ghi.lock().unwrap_or_else(|e| e.into_inner());
        let (phieu, so) = {
            let mut st = self.lock();
            let co_phieu = st.dot.iter().any(|d| d.can_ghi);
            // Sổ chụp TRƯỚC khi gỡ cờ: bị giết giữa chừng thì mở lại vẫn biết phải ghi lại.
            let so = (self.st_doi.swap(false, std::sync::atomic::Ordering::SeqCst) || co_phieu)
                .then(|| serde_json::to_vec_pretty(&*st).unwrap_or_default());
            let phieu: Vec<(String, Vec<u8>)> = st
                .dot
                .iter_mut()
                .filter(|d| d.can_ghi)
                .map(|d| {
                    d.can_ghi = false;
                    (d.phieu.dot_id.clone(), serde_json::to_vec_pretty(&d.phieu).unwrap_or_default())
                })
                .collect();
            (phieu, so)
        };
        let mut loi: Vec<String> = Vec::new();
        if !phieu.is_empty() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                log_mot_lan("phieu-thu-muc", &format!("[daychuyen] không tạo được {}: {e}", dir.display()));
                loi = phieu.iter().map(|(id, _)| id.clone()).collect();
            } else {
                for (id, data) in &phieu {
                    if let Err(e) = ghi_nguyen_tu(&dir.join(format!("{id}.json")), data) {
                        log_mot_lan(&format!("phieu-ghi-{id}"), &format!("[daychuyen] ghi phiếu {id} lỗi (sẽ ghi lại): {e}"));
                        loi.push(id.clone());
                    }
                }
            }
        }
        if !loi.is_empty() {
            let mut st = self.lock();
            for d in st.dot.iter_mut().filter(|d| loi.contains(&d.phieu.dot_id)) {
                d.can_ghi = true;
            }
            self.danh_dau_doi();
        }
        if let Some(data) = so {
            if let Err(e) = ghi_nguyen_tu(&self.state_path, &data) {
                log_mot_lan("luu-lich", &format!("[daychuyen] không lưu được lich_tai.json: {e}"));
                self.danh_dau_doi();
            }
        }
        loi.len()
    }

    /// Dọn MỖI NGÀY 1 lần (không chỉ lúc mở app): `.json.tmp` mồ côi, phiếu
    /// của mình cũ hơn 14 ngày, và đóng phiếu mồ côi sổ không biết (sổ hỏng).
    pub fn don_hang_ngay(&self, dir: &Path, now: DateTime<FixedOffset>) {
        let _g = self.ghi.lock().unwrap_or_else(|e| e.into_inner());
        don_tmp_mo_coi(dir);
        don_phieu_cu(dir, now.date_naive());
        let biet: Vec<String> = self.lock().dot.iter().map(|d| d.phieu.dot_id.clone()).collect();
        let n = dong_phieu_mo_coi(dir, &biet, now);
        if n > 0 {
            eprintln!("[daychuyen] đã đóng {n} phiếu mồ côi (sổ lich_tai.json không biết)");
        }
    }

    fn them_canh_bao(st: &mut TrangThaiLich, id: String, text: String, now: DateTime<FixedOffset>) {
        if !st.canh_bao.iter().any(|c| c.id == id) {
            st.canh_bao.push(CanhBao { id, luc: iso(now), text });
            let n = st.canh_bao.len();
            if n > 20 {
                st.canh_bao.drain(..n - 20);
            }
        }
    }

    /// File trạng thái bận HIỆN TẠI — ghi LUÔN khi app chạy (mục 12 không phụ
    /// thuộc Lịch). Lý do chặn (ổ đầy) chỉ kèm khi Lịch đang bật.
    pub fn trang_thai_ban_hien_tai(
        &self,
        items: &[DownloadItem],
        lich_bat: bool,
        now: DateTime<FixedOffset>,
        phien_ban: &str,
    ) -> serde_json::Value {
        let ly_do = if lich_bat { self.lock().ly_do_chan.clone() } else { None };
        trang_thai_ban(items, self.co_dot_mo(), ly_do, now, phien_ban)
    }

    /// Ghi `ban=false` lần cuối khi thoát êm (luôn ghi, không phụ thuộc Lịch).
    pub fn ghi_ban_khi_thoat(&self, phien_ban: &str) {
        ghi_ban_thoat_vao(Path::new(THU_MUC_EXCHANGE), chrono::Local::now().fixed_offset(), phien_ban);
    }
}

fn ghi_ban_thoat_vao(dir: &Path, now: DateTime<FixedOffset>, phien_ban: &str) -> bool {
    let mut v = trang_thai_ban(&[], false, Some("app đã thoát".into()), now, phien_ban);
    v["ban"] = serde_json::Value::Bool(false);
    ghi_file_ban(dir, &v)
}

/// Chạy ở luồng chặn: cập nhật phiếu + ghi đĩa (+ dọn hằng ngày, + file bận).
async fn nhip_nen(
    dc: &Arc<DayChuyen>,
    queue: &Arc<crate::queue::QueueManager>,
    history: &Arc<crate::history_store::HistoryStore>,
    dir: PathBuf,
    don: bool,
    ban: Option<(bool, String)>,
) {
    let (dc, queue, history) = (dc.clone(), queue.clone(), history.clone());
    let _ = tauri::async_runtime::spawn_blocking(move || {
        let now = chrono::Local::now().fixed_offset();
        if don {
            dc.don_hang_ngay(&dir, now);
        }
        dc.cap_nhat(&queue, &history);
        dc.ghi_xuong_dia(&dir);
        if let Some((lich_bat, phien_ban)) = ban {
            let v = dc.trang_thai_ban_hien_tai(&queue.list(), lich_bat, now, &phien_ban);
            ghi_file_ban(Path::new(THU_MUC_EXCHANGE), &v);
        }
    })
    .await;
}

/// Thư mục đích (đúng `resolve_watch_folder`) + tên kênh đích của các kênh
/// đang bật trong nhóm.
fn thu_muc_cua_nhom(
    store: &crate::watchlist_store::WatchlistStore,
    s: &Settings,
    nhom: Option<&str>,
) -> Vec<(String, String)> {
    store
        .list()
        .into_iter()
        .filter(|c| c.enabled && nhom.is_none_or(|g| c.group.as_deref().unwrap_or("") == g))
        .map(|c| {
            let f = crate::watcher::resolve_watch_folder(&c.dest_dir, &c.target_name, &s.watch_root, &s.default_folder);
            (f.to_string_lossy().into_owned(), ten_dich_cua(&c))
        })
        .collect()
}

pub fn ten_dich_cua(c: &crate::models::WatchedChannel) -> String {
    c.target_name
        .clone()
        .filter(|t| !t.trim().is_empty())
        .or_else(|| c.title.clone())
        .unwrap_or_else(|| c.url.clone())
}

/// Vòng Lịch tải — chạy CẠNH `watcher::spawn_monitor`. File trạng thái bận,
/// cập nhật/đóng phiếu đang mở và dọn hằng ngày chạy LUÔN; chỉ việc MỞ lượt
/// quét mới cần công tắc `lich_tai_enabled` (mặc định TẮT).
pub fn spawn_scheduler(
    app: tauri::AppHandle,
    store: Arc<crate::watchlist_store::WatchlistStore>,
    queue: Arc<crate::queue::QueueManager>,
    settings: Arc<crate::settings_store::SettingsStore>,
    history: Arc<crate::history_store::HistoryStore>,
    dc: Arc<DayChuyen>,
) {
    tauri::async_runtime::spawn(async move {
        // Như vòng theo dõi cũ: chờ hàng đợi khôi phục xong rồi mới chạy.
        tokio::time::sleep(Duration::from_secs(60)).await;
        let mut ngay_don: Option<chrono::NaiveDate> = None;
        let mut lan_ghi_ban = std::time::Instant::now() - Duration::from_secs(3600);
        let phien_ban = app.package_info().version.to_string();
        loop {
            let s = settings.get();
            let now = chrono::Local::now().fixed_offset();
            let don = ngay_don != Some(now.date_naive());
            if don {
                ngay_don = Some(now.date_naive());
            }
            let ban = if lan_ghi_ban.elapsed() >= Duration::from_secs(60) {
                lan_ghi_ban = std::time::Instant::now();
                Some((s.lich_tai_enabled, phien_ban.clone()))
            } else {
                None
            };
            nhip_nen(&dc, &queue, &history, thu_muc_phieu(&s), don, ban).await;
            if s.lich_tai_enabled {
                let cho_o = dc
                    .kiem_o_lai_luc
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .is_some_and(|t| std::time::Instant::now() < t);
                let chay = !cho_o
                    && !dc.dang_quet.load(std::sync::atomic::Ordering::SeqCst)
                    && den_gio_chay(now, &s.lich_tai_gio, &dc.lock());
                if chay {
                    dc.dang_quet.store(true, std::sync::atomic::Ordering::SeqCst);
                    let (app, store, queue, settings, history, dc) =
                        (app.clone(), store.clone(), queue.clone(), settings.clone(), history.clone(), dc.clone());
                    tauri::async_runtime::spawn(async move {
                        chay_ngay(&app, &store, &queue, &settings, &history, &dc).await;
                        *dc.sink.lock().unwrap_or_else(|e| e.into_inner()) = None;
                        dc.dang_quet.store(false, std::sync::atomic::Ordering::SeqCst);
                    });
                }
            }
            tokio::time::sleep(Duration::from_secs(15)).await;
        }
    });
}

/// Có lượt quét tay / nhịp cũ đang chạy dở → chờ nó xong rồi mới quét nhóm
/// (khỏi vướng khoá `CHECKING` 10 phút). Tối đa 30 phút rồi đi tiếp —
/// `check_channel` vẫn tự chờ khoá từng kênh như mọi lượt.
async fn cho_luot_quet_khac() {
    let mut waited = 0u32;
    while crate::watcher::co_luot_quet_khac() && waited < 900 {
        tokio::time::sleep(Duration::from_secs(2)).await;
        waited += 1;
    }
}

/// CHỜ lượt quét khác xong RỒI mới mở/nối đợt — `mo_luc`/`han_chot` tính từ
/// lúc nhóm thật sự bắt đầu quét, lượt quét tay đang chạy dở không ăn vào
/// hạn chót 3 giờ của đợt. `dong_ho` đọc giờ SAU khi chờ.
pub async fn mo_dot_sau_khi_cho<F, T>(
    cho: F,
    dong_ho: impl Fn() -> DateTime<FixedOffset>,
    mo: impl FnOnce(DateTime<FixedOffset>) -> T,
) -> T
where
    F: std::future::Future<Output = ()>,
{
    cho.await;
    mo(dong_ho())
}

/// Mở đợt mới cho nhóm, hoặc nối đợt hôm nay đang mở. Trả `dot_id`.
fn mo_hoac_noi_dot(st: &mut TrangThaiLich, nhom: &str, now: DateTime<FixedOffset>, han_gio: u32, dir: &Path) -> String {
    if let Some(d) = tim_dot_hom_nay(st, nhom, now) {
        return d.phieu.dot_id.clone();
    }
    let mut da_co: Vec<String> = st.dot.iter().map(|d| d.phieu.dot_id.clone()).collect();
    if let Ok(rd) = std::fs::read_dir(dir) {
        da_co.extend(rd.flatten().filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            la_ten_phieu(&n).then(|| n.trim_end_matches(".json").to_string())
        }));
    }
    let d = mo_dot(dot_id_moi(&now.format("%Y%m%d").to_string(), &da_co), nhom, now, han_gio);
    let id = d.phieu.dot_id.clone();
    st.dot.push(d);
    id
}

/// 1 lượt Lịch tải trong ngày: kiểm tra ổ → quét LẦN LƯỢT từng nhóm (mỗi nhóm
/// 1 phiếu). Nhóm trước xếp hết vào hàng là sang nhóm sau — không chờ tải
/// xong. Không giữ khoá `CHECKING` xuyên nhóm (mỗi nhóm 1 lần `check_all`),
/// không gọi `reconcile_all` (check_channel tự gọi như mọi lượt quét).
async fn chay_ngay(
    app: &tauri::AppHandle,
    store: &Arc<crate::watchlist_store::WatchlistStore>,
    queue: &Arc<crate::queue::QueueManager>,
    settings: &Arc<crate::settings_store::SettingsStore>,
    history: &Arc<crate::history_store::HistoryStore>,
    dc: &Arc<DayChuyen>,
) {
    // Cờ chặn quét tay bật NGAY từ bước kiểm ổ, giữ liền tới hết nhóm cuối.
    let co = CoChay::bat("chuẩn bị (kiểm tra ổ đĩa)".into());
    let s = settings.get();
    let now = chrono::Local::now().fixed_offset();
    let dir = thu_muc_phieu(&s);
    vao_ngay(&mut dc.lock(), now);
    dc.danh_dau_doi();
    // #20: ổ trống trước khi mở đợt.
    let folders: Vec<PathBuf> = s
        .lich_tai_nhom
        .iter()
        .flat_map(|g| thu_muc_cua_nhom(store, &s, Some(g)))
        .map(|(f, _)| PathBuf::from(f))
        .collect();
    if let Some(ly_do) = kiem_tra_o_dia(&folders, &dung_luong_trong) {
        {
            let mut st = dc.lock();
            if st.ly_do_chan.as_deref() != Some(ly_do.as_str()) {
                log_mot_lan(&format!("o-dia-{}", now.format("%Y%m%d")), &format!("[daychuyen] {ly_do}"));
            }
            st.ly_do_chan = Some(ly_do.clone());
            DayChuyen::them_canh_bao(&mut st, format!("o-dia-{}", now.format("%Y%m%d")), ly_do, now);
        }
        dc.danh_dau_doi();
        *dc.kiem_o_lai_luc.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(std::time::Instant::now() + Duration::from_secs(600));
        nhip_nen(dc, queue, history, dir, false, None).await;
        return;
    }
    dc.lock().ly_do_chan = None;
    let con_lai = nhom_can_quet(&s.lich_tai_nhom, &dc.lock().nhom_da_quet);
    for nhom in &con_lai {
        if !settings.get().lich_tai_enabled {
            break; // anh tắt giữa chừng → dừng sau nhóm đang chạy
        }
        co.doi(format!("nhóm {} (chờ lượt quét tay xong)", ten_nhom_hien_thi(nhom)));
        let dot_id = mo_dot_sau_khi_cho(
            cho_luot_quet_khac(),
            || chrono::Local::now().fixed_offset(),
            |now| mo_hoac_noi_dot(&mut dc.lock(), nhom, now, s.lich_tai_han_chot_gio, &dir),
        )
        .await;
        dc.danh_dau_doi();
        co.doi(format!("{dot_id} ({})", ten_nhom_hien_thi(nhom)));
        nhip_nen(dc, queue, history, dir.clone(), false, None).await;
        let sink: JobSink = Arc::default();
        let thu_muc: Vec<String> = thu_muc_cua_nhom(store, &s, Some(nhom)).into_iter().map(|(f, _)| f).collect();
        *dc.sink.lock().unwrap_or_else(|e| e.into_inner()) = Some((dot_id.clone(), sink.clone()));
        let fut = crate::watcher::check_all_with(app, store, queue, settings, history, Some(nhom.clone()), Some(sink.clone()));
        tokio::pin!(fut);
        // Trong lúc quét: 2 giây rút job ra phiếu 1 lần (app bị giết giữa
        // chừng thì job đã xếp hàng vẫn có trong sổ — D16).
        loop {
            tokio::select! {
                _ = &mut fut => break,
                _ = tokio::time::sleep(Duration::from_secs(2)) => {
                    nhip_nen(dc, queue, history, dir.clone(), false, None).await;
                }
            }
        }
        nhip_nen(dc, queue, history, dir.clone(), false, None).await;
        *dc.sink.lock().unwrap_or_else(|e| e.into_inner()) = None;
        {
            let mut st = dc.lock();
            if let Some(d) = st.dot.iter_mut().find(|d| d.phieu.dot_id == dot_id) {
                chot_quet(d, &thu_muc);
            }
            if !st.nhom_da_quet.contains(nhom) {
                st.nhom_da_quet.push(nhom.clone());
            }
        }
        dc.danh_dau_doi();
        nhip_nen(dc, queue, history, dir.clone(), false, None).await;
    }
    if settings.get().lich_tai_enabled {
        dc.lock().xong_ngay = true;
        dc.danh_dau_doi();
    }
    nhip_nen(dc, queue, history, dir, false, None).await;
    drop(co);
}

// ───────────────────────── test ─────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ConflictPolicy, DownloadMode, DownloadRequest};

    fn t(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(s).unwrap()
    }

    fn tmp_dir(ten: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bqd-daychuyen-{ten}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn item(id: &str, state: DownloadState, err: Option<&str>, out: Option<&str>) -> DownloadItem {
        DownloadItem {
            short_id: id.into(),
            request: DownloadRequest {
                url: format!("https://www.youtube.com/watch?v={id}"),
                mode: DownloadMode::Video,
                format_id: None,
                save_folder: PathBuf::from(r"D:\v\kênh 1"),
                sub_langs: vec![],
                auto_translate_to: None,
                on_conflict: ConflictPolicy::Rename,
                use_aria2c: false,
                playlist_all: false,
                polite: true,
                force_redownload: false,
                max_height: None,
            },
            title: "t".into(),
            thumbnail: None,
            channel: None,
            extractor: "youtube".into(),
            state,
            bytes_downloaded: 0,
            bytes_total: None,
            speed_bps: None,
            eta_sec: None,
            attempt: 0,
            bot_retries: 0,
            error_message: err.map(Into::into),
            output_path: out.map(PathBuf::from),
            created_at: Utc::now(),
            finished_at: out.map(|_| Utc::now()),
        }
    }

    fn job(id: &str, folder: &str) -> EnqueuedJob {
        EnqueuedJob {
            job_id: format!("j{id}"),
            video_id: format!("youtube:{id}"),
            folder: PathBuf::from(folder),
            ten_dich: folder.rsplit('\\').next().unwrap().into(),
        }
    }

    fn khong_size(_: &Path) -> Option<u64> {
        None
    }

    /// Hàng đợi giả: job id → quan sát.
    fn qs(map: &std::collections::HashMap<String, QuanSat>) -> impl Fn(&str) -> QuanSat + '_ {
        move |j| map.get(j).cloned().unwrap_or(QuanSat::KhongThay)
    }

    const MO: &str = "2026-11-04T06:00:12+07:00";

    // ── D18: ca mẫu CHUNG với V18 (lấy đúng trong INTEGRATION.md luật 4) ──
    #[test]
    fn d18_chuan_hoa_duong_dan_ca_mau_chung() {
        let nfd_my = "m\u{0079}\u{0303}"; // "mỹ" dạng NFD: y + dấu ngã tổ hợp
        let a = chuan_hoa_duong_dan("D:/video ssmatool/video mỹ/kênh 3");
        let b = chuan_hoa_duong_dan(r"D:\video ssmatool\video mỹ\kênh 3\");
        let c = chuan_hoa_duong_dan(&format!(r"d:\VIDEO SSMATOOL\video {nfd_my}\kênh 3"));
        assert_eq!(a, r"d:\video ssmatool\video mỹ\kênh 3");
        assert_eq!(a, b);
        assert_eq!(a, c, "dạng NFD phải về cùng khoá NFC");
        // Chữ Nhật (dakuten tổ hợp), chữ Hàn (jamo rời), `/` lẫn `\`, `\` cuối.
        assert_eq!(
            chuan_hoa_duong_dan("D:/video nhật/チャンネル/"),
            chuan_hoa_duong_dan("d:\\VIDEO NHẬT\\チャンネル\\\\"),
        );
        assert_eq!(chuan_hoa_duong_dan("D:/한국/채널"), chuan_hoa_duong_dan("D:\\\u{1112}\u{1161}\u{11AB}국\\채널"));
        assert_eq!(chuan_hoa_duong_dan("D:/video nhật/ガ"), chuan_hoa_duong_dan("D:\\video nhật\\\u{30AB}\u{3099}"));
    }

    // ── D13: ghi nguyên tử, bị giết giữa lúc ghi .tmp ──
    #[test]
    fn d13_bi_giet_luc_ghi_tmp_phieu_that_van_la_ban_truoc() {
        let d = tmp_dir("d13");
        let p = d.join("20261104-01.json");
        ghi_nguyen_tu(&p, br#"{"seq":1}"#).unwrap();
        // Giả bị giết giữa chừng: file .tmp viết dở, chưa kịp đổi tên.
        std::fs::write(tmp_cua(&p), br#"{"seq":2, "kenh":[{"#).unwrap();
        let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        assert_eq!(doc["seq"], 1, "file phiếu thật vẫn là bản trước và đọc được");
        assert_eq!(tmp_cua(&p).extension().unwrap(), "tmp");
        assert!(!tmp_cua(&p).to_string_lossy().contains(".part"));
        // Mở lại app: dọn .tmp mồ côi đúng mẫu tên, không đụng file khác.
        std::fs::write(d.join("khac.json.tmp"), b"x").unwrap();
        assert_eq!(don_tmp_mo_coi(&d), 1);
        assert!(!tmp_cua(&p).exists());
        assert!(d.join("khac.json.tmp").exists(), "chỉ xoá file đúng mẫu tên");
        assert!(p.exists());
        std::fs::remove_dir_all(d).ok();
    }

    // ── D14: tiến trình khác đang mở đọc phiếu lúc đổi tên ──
    #[cfg(windows)]
    #[test]
    fn d14_dang_bi_doc_thi_thu_lai_khong_mat_cap_nhat() {
        use std::os::windows::fs::OpenOptionsExt;
        let d = tmp_dir("d14");
        let p = d.join("20261104-01.json");
        ghi_nguyen_tu(&p, br#"{"seq":1}"#).unwrap();
        // Bên đọc giữ file KHÔNG cho xoá/đổi tên (như Python open trên Windows).
        let reader = std::fs::OpenOptions::new().read(true).share_mode(0x1).open(&p).unwrap();
        let h = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(450));
            drop(reader);
        });
        ghi_nguyen_tu(&p, br#"{"seq":2}"#).expect("thử lại tới khi bên đọc nhả");
        h.join().unwrap();
        let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        assert_eq!(doc["seq"], 2, "không mất cập nhật");
        // Bị khoá MÃI: hết lượt thử lại thì trả lỗi, không crash, bản cũ còn nguyên.
        let reader = std::fs::OpenOptions::new().read(true).share_mode(0x1).open(&p).unwrap();
        let r = ghi_nguyen_tu_voi(&p, br#"{"seq":3}"#, 2, Duration::from_millis(10));
        assert!(r.is_err());
        drop(reader);
        let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        assert_eq!(doc["seq"], 2);
        assert!(!tmp_cua(&p).exists(), "không để lại .tmp");
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn don_phieu_cu_hon_14_ngay_chi_dung_mau_ten() {
        let d = tmp_dir("don14");
        for n in ["20261020-01.json", "20261021-01.json", "20261104-01.json", "ghi-chu.json"] {
            std::fs::write(d.join(n), b"{}").unwrap();
        }
        let hom_nay = chrono::NaiveDate::from_ymd_opt(2026, 11, 4).unwrap();
        assert_eq!(don_phieu_cu(&d, hom_nay), 1);
        assert!(!d.join("20261020-01.json").exists());
        assert!(d.join("20261021-01.json").exists() && d.join("ghi-chu.json").exists());
        std::fs::remove_dir_all(d).ok();
    }

    // ── D1, D2, D3: giờ chạy + chạy đúng 1 lần/ngày ──
    #[test]
    fn d1_den_gio_thi_chay_dung_1_lan() {
        let mut st = TrangThaiLich::default();
        assert!(!den_gio_chay(t("2026-11-04T05:59:00+07:00"), "06:00", &st), "chưa tới giờ");
        let now = t("2026-11-04T06:00:05+07:00");
        assert!(den_gio_chay(now, "06:00", &st));
        vao_ngay(&mut st, now);
        st.nhom_da_quet = vec!["A".into(), "B".into()];
        st.xong_ngay = true;
        assert!(!den_gio_chay(t("2026-11-04T06:00:20+07:00"), "06:00", &st), "đã chạy xong hôm nay");
        assert!(!den_gio_chay(t("2026-11-04T23:59:00+07:00"), "06:00", &st));
        // Hôm sau chạy lại; sang ngày thì sổ "đã quét" về rỗng.
        let mai = t("2026-11-05T06:00:01+07:00");
        assert!(den_gio_chay(mai, "06:00", &st));
        vao_ngay(&mut st, mai);
        assert!(st.nhom_da_quet.is_empty() && !st.xong_ngay);
        assert!(!den_gio_chay(now, "6h", &TrangThaiLich::default()), "giờ sai định dạng thì không chạy");
        // Các nhóm làm ĐÚNG thứ tự đã xếp (không theo bảng chữ cái / thứ tự nhóm của trang Theo dõi).
        let thu_tu: Vec<String> = ["Mỹ mới", "HÀN", "", "MỸ"].iter().map(|s| s.to_string()).collect();
        assert_eq!(nhom_can_quet(&thu_tu, &[]), thu_tu);
        assert_eq!(nhom_can_quet(&thu_tu, &["Mỹ mới".into()]), vec!["HÀN".to_string(), "".into(), "MỸ".into()]);
        assert!(nhom_can_quet(&thu_tu, &thu_tu).is_empty());
    }

    #[test]
    fn d2_mo_app_sau_gio_bat_dau_thi_chay_ngay() {
        let st = TrangThaiLich::default();
        assert!(den_gio_chay(t("2026-11-04T06:40:00+07:00"), "06:00", &st));
    }

    #[test]
    fn d3_mo_lai_trong_ngay_noi_dot_dang_mo_khong_mo_dot_moi() {
        let now = t(MO);
        let mut st = TrangThaiLich::default();
        vao_ngay(&mut st, now);
        st.dot.push(mo_dot("20261104-01".into(), "A", now, 3));
        st.nhom_da_quet.push("A".into());
        // App tắt rồi mở lại: nạp từ đĩa.
        let mut st: TrangThaiLich = serde_json::from_str(&serde_json::to_string(&st).unwrap()).unwrap();
        let later = t("2026-11-04T07:10:00+07:00");
        vao_ngay(&mut st, later);
        assert_eq!(st.nhom_da_quet, vec!["A".to_string()], "không quét lại nhóm đã quét → không cấp thêm suất");
        assert_eq!(tim_dot_hom_nay(&mut st, "A", later).unwrap().phieu.dot_id, "20261104-01");
        assert!(tim_dot_hom_nay(&mut st, "B", later).is_none());
        assert_eq!(dot_id_moi("20261104", &["20261104-01".into(), "20261103-07".into()]), "20261104-02");
        assert_eq!(dot_id_moi("20261105", &["20261104-01".into()]), "20261105-01");
    }

    // ── D4: Lịch tải + nhịp cũ cùng bật ──
    #[test]
    fn d4_lich_tai_bat_thi_nhip_cu_tu_tat_khong_sua_gia_tri() {
        let mut s = Settings::default();
        s.watch_auto_enabled = true;
        assert!(nhip_cu_duoc_chay(&s));
        s.lich_tai_enabled = true;
        assert!(!nhip_cu_duoc_chay(&s), "không chạy cả hai");
        assert!(s.watch_auto_enabled, "giá trị của anh giữ nguyên");
    }

    // ── Sửa 1 (S1): Lịch TẮT = y như cũ — giữ khoảng 1–1440, < 30 chỉ cảnh báo ──
    #[test]
    fn s1_nhip_cu_giu_khoang_cu_duoi_30_phut_chi_canh_bao() {
        let mut s = Settings::default();
        s.watch_auto_enabled = true;
        s.watch_interval_min = 1;
        assert_eq!(nhip_cu_phut(&s), 1, "không ép lên 30 phút");
        assert!(nhip_cu_qua_day(&s), "nhịp < 30 phút thì cảnh báo");
        s.watch_interval_min = 0;
        assert_eq!(nhip_cu_phut(&s), 1);
        s.watch_interval_min = 5000;
        assert_eq!(nhip_cu_phut(&s), 1440);
        s.watch_interval_min = 30;
        assert!(!nhip_cu_qua_day(&s));
        s.watch_interval_min = 1;
        s.lich_tai_enabled = true;
        assert!(!nhip_cu_qua_day(&s), "Lịch bật thì nhịp cũ nghỉ — không cảnh báo");
        s.lich_tai_enabled = false;
        s.watch_auto_enabled = false;
        assert!(!nhip_cu_qua_day(&s), "nhịp cũ tắt — không cảnh báo");
    }

    #[test]
    fn mac_dinh_lich_tai_tat_va_settings_cu_van_doc_duoc() {
        let s = Settings::default();
        assert!(!s.lich_tai_enabled && !s.mo_cung_windows);
        assert_eq!(s.lich_tai_gio, "06:00");
        assert_eq!(s.lich_tai_han_chot_gio, 3);
        let cu: Settings = serde_json::from_str(r#"{"defaultFolder":"D:/V","maxConcurrency":3,"theme":"dark",
            "language":"vi","clipboardWatcher":true,"notifications":true,"aria2cEnabled":false}"#).unwrap();
        assert!(!cu.lich_tai_enabled && cu.lich_tai_nhom.is_empty() && cu.daychuyen_dir.is_none());
        assert_eq!(cu.lich_tai_gio, "06:00");
        assert_eq!(thu_muc_phieu(&cu), PathBuf::from(r"C:\ProgramData\BQDayChuyen\dot"));
    }

    // ── D5: nhóm 58 kênh, 3 kênh không có video mới ──
    #[test]
    fn d5_phieu_ghi_55_video_va_3_kenh_khong_co_video() {
        let mut d = mo_dot("20261104-01".into(), "Mỹ mới", t(MO), 3);
        let thu_muc: Vec<String> = (1..=58).map(|i| format!(r"D:\video ssmatool\video mỹ\kênh {i}")).collect();
        for (i, f) in thu_muc.iter().enumerate().skip(3) {
            assert!(them_job(&mut d, &job(&format!("v{i}"), f)));
        }
        chot_quet(&mut d, &thu_muc);
        let so_video: usize = d.phieu.kenh.iter().map(|k| k.video.len()).sum();
        assert_eq!(so_video, 55);
        assert_eq!(d.phieu.kenh_khong_co_video.len(), 3);
        assert_eq!(d.phieu.kenh_khong_co_video[0], r"D:\video ssmatool\video mỹ\kênh 1");
        assert!(d.quet_xong && d.dang_mo());
        // Đúng các khoá của giao kèo mục 11.
        let v = serde_json::to_value(&d.phieu).unwrap();
        for k in ["schema", "dot_id", "nhom", "seq", "mo_luc", "han_chot", "trang_thai", "dong_luc",
                  "ly_do_dong", "kenh", "kenh_khong_co_video"] {
            assert!(v.get(k).is_some(), "thiếu khoá {k}");
        }
        assert_eq!(v["schema"], 1);
        assert_eq!(v["han_chot"], "2026-11-04T09:00:12+07:00", "hạn chót mặc định 3 giờ");
        assert_eq!(v["kenh"][0]["video"][0]["id"], "youtube:v3");
        for k in ["id", "trang_thai", "file", "size", "xong_luc", "loi"] {
            assert!(v["kenh"][0]["video"][0].get(k).is_some(), "video thiếu khoá {k}");
        }
    }

    // ── D6: mọi video có kết quả cuối → đóng `xong` ──
    #[test]
    fn d6_moi_video_co_ket_qua_cuoi_thi_dong_xong_seq_tang() {
        let mut d = mo_dot("20261104-01".into(), "A", t(MO), 3);
        them_job(&mut d, &job("a", r"D:\v\kênh 1"));
        them_job(&mut d, &job("b", r"D:\v\kênh 2"));
        let mut m = std::collections::HashMap::new();
        m.insert("ja".into(), QuanSat::DangTai);
        m.insert("jb".into(), QuanSat::Cho);
        chot_quet(&mut d, &[r"D:\v\kênh 1".into(), r"D:\v\kênh 2".into()]);
        let seq0 = d.phieu.seq;
        assert!(cap_nhat_dot(&mut d, &qs(&m), &khong_size, t("2026-11-04T06:05:00+07:00")));
        assert_eq!(d.phieu.kenh[0].video[0].trang_thai, "dang_tai");
        assert!(d.dang_mo(), "còn video chưa xong thì chưa đóng");
        assert!(!cap_nhat_dot(&mut d, &qs(&m), &khong_size, t("2026-11-04T06:05:30+07:00")), "không đổi thì không ghi");
        let luc = DateTime::parse_from_rfc3339("2026-11-03T23:07:40Z").unwrap().with_timezone(&Utc);
        m.insert("ja".into(), QuanSat::Xong { path: Some(r"D:\v\kênh 1\Tên video.mp4".into()), luc: Some(luc) });
        m.insert("jb".into(), QuanSat::BoQua("live / chờ live / phát lại live".into()));
        assert!(cap_nhat_dot(&mut d, &qs(&m), &|_: &Path| Some(123), t("2026-11-04T06:08:00+07:00")));
        assert_eq!(d.phieu.trang_thai, "da_dong");
        assert_eq!(d.phieu.ly_do_dong.as_deref(), Some("xong"));
        assert_eq!(d.phieu.dong_luc.as_deref(), Some("2026-11-04T06:08:00+07:00"));
        assert!(d.phieu.seq > seq0);
        let v = &d.phieu.kenh[0].video[0];
        assert_eq!((v.file.as_deref(), v.size), (Some("Tên video.mp4"), Some(123)), "tên CUỐI từ mục Completed");
        assert_eq!(v.xong_luc.as_deref(), Some("2026-11-04T06:07:40+07:00"));
        // Đã đóng thì không bao giờ sửa nữa.
        m.insert("jb".into(), QuanSat::DangTai);
        assert!(!cap_nhat_dot(&mut d, &qs(&m), &khong_size, t("2026-11-04T07:00:00+07:00")));
        assert!(!them_job(&mut d, &job("c", r"D:\v\kênh 3")));
    }

    #[test]
    fn nhom_khong_co_video_nao_dong_xong_ngay_sau_khi_quet() {
        let mut d = mo_dot("20261104-02".into(), "", t(MO), 3);
        assert_eq!(d.phieu.nhom, "Chưa phân nhóm");
        assert!(!cap_nhat_dot(&mut d, &|_| QuanSat::Cho, &khong_size, t(MO)), "chưa quét xong thì chưa đóng");
        chot_quet(&mut d, &[r"D:\v\kênh 9".into()]);
        assert!(cap_nhat_dot(&mut d, &|_| QuanSat::Cho, &khong_size, t(MO)));
        assert_eq!(d.phieu.ly_do_dong.as_deref(), Some("xong"));
    }

    // ── D7: 1 video bị chặn bot, qua hạn chót ──
    #[test]
    fn d7_qua_han_chot_dong_het_han_video_ghi_tre_phieu_khong_bi_sua() {
        let mut d = mo_dot("20261104-01".into(), "A", t(MO), 3);
        them_job(&mut d, &job("a", r"D:\v\kênh 1"));
        them_job(&mut d, &job("b", r"D:\v\kênh 2"));
        chot_quet(&mut d, &[]);
        let mut m = std::collections::HashMap::new();
        m.insert("ja".into(), QuanSat::Xong { path: Some(r"D:\v\kênh 1\x.mp4".into()), luc: None });
        m.insert("jb".into(), QuanSat::Cho); // đang chờ thử lại vì bị chặn bot
        cap_nhat_dot(&mut d, &qs(&m), &khong_size, t("2026-11-04T08:59:59+07:00"));
        assert!(d.dang_mo());
        assert!(cap_nhat_dot(&mut d, &qs(&m), &khong_size, t("2026-11-04T09:00:12+07:00")));
        assert_eq!(d.phieu.ly_do_dong.as_deref(), Some("het_han"));
        assert_eq!(d.phieu.kenh[1].video[0].trang_thai, "tre");
        assert_eq!(d.phieu.kenh[0].video[0].trang_thai, "xong");
        let dong = d.phieu.clone();
        // Video trễ tải xong sau đó: phiếu đã đóng KHÔNG bị sửa (tool cắt vẫn
        // cắt theo quét thư mục — file về đúng thư mục của job).
        m.insert("jb".into(), QuanSat::Xong { path: Some(r"D:\v\kênh 2\y.mp4".into()), luc: None });
        assert!(!cap_nhat_dot(&mut d, &qs(&m), &khong_size, t("2026-11-04T10:00:00+07:00")));
        assert_eq!(d.phieu, dong);
    }

    // ── D8: live / chờ live / was_live → bo_qua, không chiếm suất ──
    #[test]
    fn d8_live_bi_cong_tai_chan_thi_ghi_bo_qua_kem_ly_do() {
        let it = item("x", DownloadState::Skipped, Some(crate::queue::LIVE_SKIP_MESSAGE), None);
        match quan_sat_job(Some(&it), None) {
            QuanSat::BoQua(l) => assert!(l.contains("live")),
            o => panic!("phải bo_qua, ra {o:?}"),
        }
        // Lọc lúc quét (is_live/is_upcoming/post_live/was_live) + cổng tải cuối
        // vẫn là của lõi — phiếu không đổi luật live. Bộ lọc vẫn chặn was_live.
        assert!(crate::args_builder::LIVE_FILTER.contains("was_live"));
        // Không chiếm suất ngày: reconcile trả suất khi mục bị chặn vì live
        // (test lõi `live_bi_chan_mai_thi_bo_han_khong_lap_vo_tan` + reconcile).
        let mut d = mo_dot("20261104-01".into(), "A", t(MO), 3);
        them_job(&mut d, &job("x", r"D:\v\kênh 1"));
        chot_quet(&mut d, &[]);
        cap_nhat_dot(&mut d, &|_| quan_sat_job(Some(&it), None), &khong_size, t(MO));
        assert_eq!(d.phieu.kenh[0].video[0].trang_thai, "bo_qua");
        assert_eq!(d.phieu.ly_do_dong.as_deref(), Some("xong"), "bỏ qua là kết quả cuối");
    }

    #[test]
    fn quy_doi_trang_thai_hang_doi_sang_phieu() {
        let q = |s, e: Option<&str>| quan_sat_job(Some(&item("a", s, e, None)), None);
        assert_eq!(q(DownloadState::Queued, None), QuanSat::Cho);
        assert_eq!(q(DownloadState::Paused, None), QuanSat::Cho);
        assert_eq!(q(DownloadState::Downloading, None), QuanSat::DangTai);
        assert!(matches!(q(DownloadState::Failed, Some("HTTP Error 500")), QuanSat::Loi { cuoi: false, .. }),
                "Failed chưa phải cuối cùng (còn Thử lại được)");
        assert!(matches!(q(DownloadState::Failed, Some("Private video. Sign in")), QuanSat::Loi { cuoi: true, .. })
                || matches!(q(DownloadState::Failed, Some("Private video. Sign in")), QuanSat::BoQua(_)));
        assert!(matches!(q(DownloadState::Cancelled, None), QuanSat::Loi { cuoi: true, .. }));
        assert_eq!(quan_sat_job(None, None), QuanSat::KhongThay);
    }

    // ── D12: 2 nguồn cùng 1 thư mục đích ──
    #[test]
    fn d12_hai_nguon_cung_thu_muc_dich_chi_1_dong_kenh() {
        let mut d = mo_dot("20261104-01".into(), "Mỹ BKT", t(MO), 3);
        // Nguồn chính lấy suất (lõi `sibling_taken_today` chặn nguồn dự phòng),
        // đường dẫn 2 nguồn viết khác dấu gạch / `\` cuối vẫn là 1 kênh đích.
        them_job(&mut d, &job("a", r"D:\v\kênh 2"));
        chot_quet(&mut d, &[r"D:\v\kênh 2".into(), "D:/v/kênh 2/".into()]);
        assert_eq!(d.phieu.kenh.len(), 1);
        assert!(d.phieu.kenh_khong_co_video.is_empty(), "nguồn dự phòng không thành 'kênh không có video'");
        // Cả 2 nguồn đều có job (vd ➕ Tải thêm) vẫn chung 1 dòng kênh.
        them_job(&mut d, &job("b", "d:/V/kênh 2"));
        assert_eq!(d.phieu.kenh.len(), 1);
        assert_eq!(d.phieu.kenh[0].video.len(), 2);
    }

    // ── D15, D16: tắt êm / bị giết giữa đợt rồi mở lại ──
    #[test]
    fn d15_d16_mo_lai_giua_dot_noi_tiep_khong_ghi_trung_khong_tai_lai_video_xong() {
        let now = t(MO);
        let mut st = TrangThaiLich::default();
        vao_ngay(&mut st, now);
        let mut d = mo_dot("20261104-01".into(), "A", now, 3);
        them_job(&mut d, &job("a", r"D:\v\kênh 1")); // sẽ xong trước khi tắt
        them_job(&mut d, &job("b", r"D:\v\kênh 2")); // đang tải lúc tắt
        st.dot.push(d);
        // Trước khi tắt: a xong (vào lịch sử), b đang tải.
        let a_xong = item("ja", DownloadState::Completed, None, Some(r"D:\v\kênh 1\a.mp4"));
        let b_dang = item("jb", DownloadState::Downloading, None, None);
        let qs1 = |j: &str| match j {
            "ja" => quan_sat_job(Some(&a_xong), None),
            _ => quan_sat_job(Some(&b_dang), None),
        };
        cap_nhat_dot(&mut st.dot[0], &qs1, &khong_size, t("2026-11-04T06:10:00+07:00"));
        let luu = serde_json::to_string(&st).unwrap();
        // Mở lại (D15 tắt êm / D16 bị giết): hàng đợi khôi phục bỏ mục
        // Completed, đưa mục đang tải về Queued → tải lại; a chỉ còn trong lịch sử.
        let mut st: TrangThaiLich = serde_json::from_str(&luu).unwrap();
        let reopened = t("2026-11-04T06:20:00+07:00");
        vao_ngay(&mut st, reopened);
        let hist_a = HistoryEntry {
            short_id: "ja".into(), url: String::new(), title: "a".into(), extractor: "youtube".into(),
            format_id: None, mode: DownloadMode::Video, save_folder: r"D:\v\kênh 1".into(),
            output_path: Some(r"D:\v\kênh 1\a.mp4".into()), status: HistoryStatus::Completed, error: None,
            finished_at: Utc::now(), channel: None, thumbnail: None, edited: false, edited_at: None,
        };
        let b_lai = item("jb", DownloadState::Queued, None, None);
        let qs2 = |j: &str| match j {
            "ja" => quan_sat_job(None, Some(&hist_a)),
            _ => quan_sat_job(Some(&b_lai), None),
        };
        let d = tim_dot_hom_nay(&mut st, "A", reopened).expect("đợt nối tiếp");
        // Lượt quét nối nhóm dở có thể đưa lại đúng job/video → không ghi 2 lần.
        assert!(!them_job(d, &job("a", r"D:\v\kênh 1")));
        assert!(!them_job(d, &EnqueuedJob { job_id: "jkhac".into(), ..job("b", r"D:\v\kênh 2") }));
        cap_nhat_dot(d, &qs2, &khong_size, reopened);
        assert_eq!(d.phieu.kenh[0].video[0].trang_thai, "xong", "video đã xong giữ xong (tra lịch sử)");
        assert_eq!(d.phieu.kenh[1].video[0].trang_thai, "cho", "video đang tải được tải lại");
        let tong: usize = d.phieu.kenh.iter().map(|k| k.video.len()).sum();
        assert_eq!(tong, 2, "không video nào bị ghi 2 lần");
        assert!(d.jobs[0].cuoi, "job đã xong không đổi nữa");
    }

    // ── D19: file trạng thái bận ──
    #[test]
    fn d19_file_ban_hang_rong_dang_tai_tam_dung() {
        let now = t("2026-11-04T10:00:00+07:00");
        let v = trang_thai_ban(&[], false, None, now, "0.1.0");
        assert_eq!(v["ban"], false);
        let v = trang_thai_ban(&[item("a", DownloadState::Downloading, None, None)], false, None, now, "0.1.0");
        assert_eq!((v["ban"].as_bool(), v["dang_lam"].as_u64()), (Some(true), Some(1)));
        let v = trang_thai_ban(&[item("a", DownloadState::Paused, None, None)], false, None, now, "0.1.0");
        assert_eq!((v["ban"].as_bool(), v["cho"].as_u64()), (Some(true), Some(1)));
        assert_eq!(trang_thai_ban(&[], true, None, now, "x")["ban"], true, "còn đợt mở = bận");
        let done = item("a", DownloadState::Completed, None, Some("x.mp4"));
        assert_eq!(trang_thai_ban(&[done], false, None, now, "x")["ban"], false);
        // Đúng khoá giao kèo mục 12, `cap_nhat` có múi giờ.
        for k in ["app", "ban", "cho", "dang_lam", "cap_nhat", "pid", "phien_ban", "ly_do"] {
            assert!(v.get(k).is_some(), "thiếu {k}");
        }
        assert_eq!(v["app"], "bqhungdown");
        assert_eq!(v["cap_nhat"], "2026-11-04T10:00:00+07:00");
        // Ghi thật vào thư mục tạm: đọc lại được, cap_nhat mới.
        let d = tmp_dir("d19");
        assert!(ghi_file_ban(&d, &trang_thai_ban(&[], false, None, chrono::Local::now().fixed_offset(), "x")));
        let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(d.join(TEN_FILE_BAN)).unwrap()).unwrap();
        let cap = DateTime::parse_from_rfc3339(doc["cap_nhat"].as_str().unwrap()).unwrap();
        assert!((chrono::Local::now().fixed_offset() - cap).num_seconds().abs() < 60);
        std::fs::remove_dir_all(d).ok();
    }

    // ── D20: chưa cài PCBot ──
    #[test]
    fn d20_thu_muc_exchange_khong_ton_tai_khong_crash_khong_tao() {
        let d = std::env::temp_dir().join(format!("bqd-khong-co-pcbot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let v = trang_thai_ban(&[], false, None, t(MO), "x");
        assert!(!ghi_file_ban(&d, &v));
        assert!(!ghi_file_ban(&d, &v), "gọi lại vẫn êm (log chỉ 1 lần)");
        assert!(!d.exists(), "không tự tạo thư mục của PCBot");
    }

    // ── D21: ổ chứa video dưới ngưỡng ──
    #[test]
    fn d21_o_duoi_nguong_khong_mo_dot_bao_ly_do() {
        let folders = vec![PathBuf::from(r"D:\video ssmatool\video mỹ\kênh 1"), PathBuf::from(r"D:\x\kênh 2")];
        let du = |_: &Path| Some(500 * GB);
        assert_eq!(kiem_tra_o_dia(&folders, &du), None);
        let d_day = |p: &Path| Some(if p.to_string_lossy().starts_with('D') { 12 * GB } else { 500 * GB });
        let ly_do = kiem_tra_o_dia(&folders, &d_day).expect("ổ D dưới 30 GB");
        assert!(ly_do.contains("D:") && ly_do.contains("30 GB"), "{ly_do}");
        let c_day = |p: &Path| Some(if p.to_string_lossy().starts_with('C') { 8 * GB } else { 500 * GB });
        assert!(kiem_tra_o_dia(&folders, &c_day).unwrap().contains("C:"));
        // Video nằm trên ổ C → ngưỡng ổ C là 30 GB.
        let c20 = |p: &Path| Some(if p.to_string_lossy().starts_with('C') { 20 * GB } else { 500 * GB });
        assert!(kiem_tra_o_dia(&[PathBuf::from(r"C:\v\k")], &c20).is_some());
        assert!(kiem_tra_o_dia(&[], &c20).is_none());
        // File trạng thái: ban=false kèm lý do.
        let v = trang_thai_ban(&[], false, Some(ly_do.clone()), t(MO), "x");
        assert_eq!(v["ban"], false);
        assert_eq!(v["ly_do"], ly_do.as_str());
        // Cảnh báo cho bot chỉ thêm 1 lần mỗi ngày.
        let mut st = TrangThaiLich::default();
        DayChuyen::them_canh_bao(&mut st, "o-dia-20261104".into(), ly_do.clone(), t(MO));
        DayChuyen::them_canh_bao(&mut st, "o-dia-20261104".into(), ly_do, t(MO));
        assert_eq!(st.canh_bao.len(), 1);
    }


    fn dc_tam(ten: &str) -> (PathBuf, Arc<DayChuyen>) {
        let d = tmp_dir(ten);
        let dc = DayChuyen::load(&d);
        (d, dc)
    }

    // ── Sửa 2 (S2): file bận ghi LUÔN, không phụ thuộc công tắc Lịch ──
    #[test]
    fn s2_file_ban_khong_phu_thuoc_lich_tai_va_thoat_em_ghi_ban_false() {
        let (d, dc) = dc_tam("s2");
        dc.lock().ly_do_chan = Some("ổ đầy".into());
        let dang = [item("a", DownloadState::Downloading, None, None)];
        // Lịch TẮT: vẫn có nội dung đúng (bận vì đang tải), không kèm lý do chặn cũ.
        let v = dc.trang_thai_ban_hien_tai(&dang, false, t(MO), "x");
        assert_eq!((v["ban"].as_bool(), v["ly_do"].is_null()), (Some(true), true));
        let v = dc.trang_thai_ban_hien_tai(&dang, true, t(MO), "x");
        assert_eq!(v["ly_do"], "ổ đầy");
        // Tắt Lịch lúc đang chạy: nhịp sau ghi theo hàng thật → không kẹt `ban=true` cũ.
        assert!(ghi_file_ban(&d, &dc.trang_thai_ban_hien_tai(&dang, true, t(MO), "x")));
        assert!(ghi_file_ban(&d, &dc.trang_thai_ban_hien_tai(&[], false, t(MO), "x")));
        let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(d.join(TEN_FILE_BAN)).unwrap()).unwrap();
        assert_eq!(doc["ban"], false);
        // Thoát êm: ghi ban=false dù Lịch đang tắt.
        assert!(ghi_file_ban(&d, &dc.trang_thai_ban_hien_tai(&dang, false, t(MO), "x")));
        assert!(ghi_ban_thoat_vao(&d, t(MO), "x"));
        let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(d.join(TEN_FILE_BAN)).unwrap()).unwrap();
        assert_eq!(doc["ban"], false);
        assert_eq!(doc["ly_do"], "app đã thoát");
        std::fs::remove_dir_all(d).ok();
    }

    // ── Sửa 3 (S3): Lịch đã tắt — phiếu đang mở vẫn cập nhật + tự đóng het_han ──
    #[test]
    fn s3_phieu_dang_mo_van_dong_het_han_khi_lich_da_tat() {
        let (d, dc) = dc_tam("s3");
        let dir = d.join("dot");
        {
            let mut st = dc.lock();
            let mut dot = mo_dot("20261104-01".into(), "A", t(MO), 3);
            them_job(&mut dot, &job("a", r"D:\v\kênh 1"));
            chot_quet(&mut dot, &[]);
            st.dot.push(dot);
        }
        // `cap_nhat_voi`/`ghi_xuong_dia` không nhận công tắc Lịch → chạy y như nhau.
        dc.cap_nhat_voi(&|_| QuanSat::DangTai, &khong_size, t("2026-11-04T07:00:00+07:00"));
        assert_eq!(dc.ghi_xuong_dia(&dir), 0);
        assert!(dc.co_dot_mo());
        dc.cap_nhat_voi(&|_| QuanSat::DangTai, &khong_size, t("2026-11-04T09:00:12+07:00"));
        assert_eq!(dc.ghi_xuong_dia(&dir), 0);
        let p: PhieuTai = serde_json::from_slice(&std::fs::read(dir.join("20261104-01.json")).unwrap()).unwrap();
        assert_eq!((p.trang_thai.as_str(), p.ly_do_dong.as_deref()), ("da_dong", Some("het_han")));
        assert_eq!(p.kenh[0].video[0].trang_thai, "tre");
        assert!(!dc.co_dot_mo());
        // Sổ cũng được lưu: mở lại app thấy phiếu đã đóng.
        let dc2 = DayChuyen::load(&d);
        assert!(!dc2.snapshot().dot[0].dang_mo());
        std::fs::remove_dir_all(d).ok();
    }

    // ── Sửa 4 (S4): ghi phiếu lỗi → đánh dấu bẩn, nhịp sau ghi lại (cả lần đóng) ──
    #[test]
    fn s4_ghi_phieu_loi_thi_ghi_lai_o_nhip_sau_ke_ca_lan_dong() {
        let (d, dc) = dc_tam("s4");
        let hong = d.join("khong-phai-thu-muc");
        std::fs::write(&hong, b"x").unwrap(); // "thư mục" phiếu thực ra là FILE → ghi hỏng
        {
            let mut st = dc.lock();
            let mut dot = mo_dot("20261104-01".into(), "A", t(MO), 3);
            them_job(&mut dot, &job("a", r"D:\v\kênh 1"));
            chot_quet(&mut dot, &[]);
            st.dot.push(dot);
        }
        let xong = |_: &str| QuanSat::Xong { path: Some(r"D:\v\kênh 1\a.mp4".into()), luc: None };
        dc.cap_nhat_voi(&xong, &khong_size, t("2026-11-04T06:10:00+07:00"));
        assert_eq!(dc.snapshot().dot[0].phieu.ly_do_dong.as_deref(), Some("xong"), "đã đóng trong sổ");
        assert_eq!(dc.ghi_xuong_dia(&hong), 1, "lần ghi đóng phiếu lỗi");
        assert!(dc.snapshot().dot[0].can_ghi, "giữ cờ bẩn");
        assert_eq!(dc.ghi_xuong_dia(&hong), 1, "vẫn lỗi thì vẫn giữ");
        // Đĩa hết lỗi: nhịp sau ghi lại dù phiếu đã ĐÓNG và không đổi gì thêm.
        std::fs::remove_file(&hong).unwrap();
        assert_eq!(dc.ghi_xuong_dia(&hong), 0);
        let p: PhieuTai = serde_json::from_slice(&std::fs::read(hong.join("20261104-01.json")).unwrap()).unwrap();
        assert_eq!(p.ly_do_dong.as_deref(), Some("xong"));
        assert!(!dc.snapshot().dot[0].can_ghi);
        // Ghi xong rồi thì nhịp sau không ghi lại vô ích.
        let truoc = std::fs::metadata(hong.join("20261104-01.json")).unwrap().modified().unwrap();
        std::thread::sleep(Duration::from_millis(20));
        dc.ghi_xuong_dia(&hong);
        assert_eq!(std::fs::metadata(hong.join("20261104-01.json")).unwrap().modified().unwrap(), truoc);
        std::fs::remove_dir_all(d).ok();
    }

    // ── Sửa 5 (S5): cờ chặn liền mạch từ kiểm ổ tới hết nhóm cuối ──
    #[test]
    fn s5_co_chan_quet_tay_lien_mach_va_tu_go_khi_xong() {
        assert_eq!(dot_dang_chay(), None);
        {
            let co = CoChay::bat("chuẩn bị (kiểm tra ổ đĩa)".into());
            assert!(dot_dang_chay().is_some(), "chặn ngay từ bước kiểm ổ");
            co.doi("20261104-01 (A)".into());
            assert_eq!(dot_dang_chay().as_deref(), Some("20261104-01 (A)"));
            co.doi("20261104-02 (B)".into());
            assert!(dot_dang_chay().is_some(), "giữa 2 nhóm không có khe None");
        }
        assert_eq!(dot_dang_chay(), None, "hết lượt (kể cả return sớm) là gỡ");
        // Phía chờ lượt quét khác: xem test `luot_quet_khac_dem_ca_check_all_lan_khoa_tung_kenh` (watcher).
    }

    // ── N1: chờ lượt quét tay xong RỒI mới mở đợt (hạn chót tính từ lúc mở thật) ──
    #[tokio::test]
    async fn n1_cho_luot_quet_khac_xong_roi_moi_mo_dot() {
        let gio = std::sync::Arc::new(Mutex::new(t(MO)));
        let mo_luc_cho = {
            let gio = gio.clone();
            async move {
                // Lượt quét tay chạy dở thêm 30 phút.
                tokio::task::yield_now().await;
                *gio.lock().unwrap() = t("2026-11-04T06:30:12+07:00");
            }
        };
        let mut st = TrangThaiLich::default();
        let dir = std::env::temp_dir().join(format!("bqd-n1-khong-co-{}", std::process::id()));
        let g = gio.clone();
        let id = mo_dot_sau_khi_cho(mo_luc_cho, move || *g.lock().unwrap(), |now| {
            assert!(st.dot.is_empty(), "chưa được mở đợt trong lúc còn chờ");
            mo_hoac_noi_dot(&mut st, "A", now, 3, &dir)
        })
        .await;
        assert_eq!(id, "20261104-01");
        assert_eq!(st.dot[0].phieu.mo_luc, "2026-11-04T06:30:12+07:00", "mở SAU khi chờ");
        assert_eq!(st.dot[0].phieu.han_chot, "2026-11-04T09:30:12+07:00", "đủ 3 giờ, không bị ăn mất 30 phút");
        // Mở lại trong ngày: nối đúng đợt cũ, không mở đợt mới.
        let id2 = mo_hoac_noi_dot(&mut st, "A", t("2026-11-04T07:00:00+07:00"), 3, &dir);
        assert_eq!((id2.as_str(), st.dot.len()), ("20261104-01", 1));
    }

    // ── Sửa 6: `xong` chỉ khi có output_path ──
    #[test]
    fn sua6_xong_chi_khi_co_output_path() {
        let mut d = mo_dot("20261104-01".into(), "A", t(MO), 3);
        them_job(&mut d, &job("a", r"D:\v\kênh 1"));
        chot_quet(&mut d, &[]);
        let chua = |_: &str| QuanSat::Xong { path: None, luc: None };
        cap_nhat_dot(&mut d, &chua, &khong_size, t(MO));
        assert_eq!(d.phieu.kenh[0].video[0].trang_thai, "dang_tai");
        assert!(d.dang_mo() && !d.jobs[0].cuoi, "chưa có tên file cuối thì chưa xong");
        let co = |_: &str| QuanSat::Xong { path: Some(r"D:\v\kênh 1\Tên thật.mp4".into()), luc: None };
        cap_nhat_dot(&mut d, &co, &khong_size, t(MO));
        assert_eq!(d.phieu.kenh[0].video[0].trang_thai, "xong");
        assert_eq!(d.phieu.kenh[0].video[0].file.as_deref(), Some("Tên thật.mp4"));
    }

    // ── Sửa 7: dọn hằng ngày (tmp mồ côi + phiếu 14 ngày) qua `don_hang_ngay` ──
    #[test]
    fn sua7_don_hang_ngay_tmp_va_phieu_cu() {
        let (d, dc) = dc_tam("sua7");
        let dir = d.join("dot");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("20261001-01.json"), b"{}").unwrap();
        std::fs::write(dir.join("20261104-01.json.tmp"), b"{").unwrap();
        dc.don_hang_ngay(&dir, t(MO));
        assert!(!dir.join("20261001-01.json").exists() && !dir.join("20261104-01.json.tmp").exists());
        std::fs::remove_dir_all(d).ok();
    }

    // ── Sửa 9: sổ lich_tai.json hỏng → không để phiếu `dang_tai` mồ côi ──
    #[test]
    fn sua9_so_hong_thi_dong_phieu_mo_coi_het_han() {
        let d = tmp_dir("sua9");
        let dir = d.join("dot");
        std::fs::create_dir_all(&dir).unwrap();
        let mut a = mo_dot("20261104-01".into(), "A", t(MO), 3);
        them_job(&mut a, &job("x", r"D:\v\kênh 1"));
        std::fs::write(dir.join("20261104-01.json"), serde_json::to_vec(&a.phieu).unwrap()).unwrap();
        let mut b = mo_dot("20261104-02".into(), "B", t(MO), 3);
        chot_quet(&mut b, &[]);
        dong(&mut b, "xong", t(MO));
        std::fs::write(dir.join("20261104-02.json"), serde_json::to_vec(&b.phieu).unwrap()).unwrap();
        std::fs::write(dir.join("20261104-03.json"), b"{ hong").unwrap();
        std::fs::write(d.join("lich_tai.json"), b"{\"ngay\": [hong").unwrap();
        let dc = DayChuyen::load(&d);
        assert_eq!(dc.snapshot(), TrangThaiLich::default(), "sổ hỏng → chạy với sổ rỗng");
        assert!(d.join("lich_tai.json.hong").exists(), "cất bản hỏng");
        let now = t("2026-11-04T07:00:00+07:00");
        dc.don_hang_ngay(&dir, now);
        let pa: PhieuTai = serde_json::from_slice(&std::fs::read(dir.join("20261104-01.json")).unwrap()).unwrap();
        assert_eq!((pa.trang_thai.as_str(), pa.ly_do_dong.as_deref()), ("da_dong", Some("het_han")));
        assert_eq!(pa.kenh[0].video[0].trang_thai, "tre");
        assert!(pa.seq > a.phieu.seq);
        let pb: PhieuTai = serde_json::from_slice(&std::fs::read(dir.join("20261104-02.json")).unwrap()).unwrap();
        assert_eq!(pb, b.phieu, "phiếu đã đóng không bị sửa");
        assert_eq!(std::fs::read(dir.join("20261104-03.json")).unwrap(), b"{ hong", "không ghi đè thứ đọc không ra");
        // Phiếu sổ CÒN biết (đang theo dõi) thì không bị đóng nhầm.
        let mut c = mo_dot("20261104-04".into(), "C", t(MO), 3);
        c.can_ghi = false;
        std::fs::write(dir.join("20261104-04.json"), serde_json::to_vec(&c.phieu).unwrap()).unwrap();
        dc.lock().dot.push(c);
        dc.don_hang_ngay(&dir, now);
        let pc: PhieuTai = serde_json::from_slice(&std::fs::read(dir.join("20261104-04.json")).unwrap()).unwrap();
        assert_eq!(pc.trang_thai, "dang_tai");
        std::fs::remove_dir_all(d).ok();
    }

    #[cfg(windows)]
    #[test]
    fn dung_luong_o_that_doc_duoc() {
        assert!(dung_luong_trong(Path::new("C:\\")).unwrap_or(0) > 0);
        assert_eq!(goc_o(Path::new(r"D:\a\b")), Some(PathBuf::from("D:\\")));
    }
}
