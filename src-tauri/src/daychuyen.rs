// Bản release dựng `panic = "abort"` (Cargo.toml): panic ở đây = CHẾT CẢ APP,
// guard/bắt panic chỉ là lớp phụ. Nên module này KHÔNG ĐƯỢC có đường panic.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]
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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
    b.len() == 11
        && b.get(8) == Some(&b'-')
        && b.iter().enumerate().all(|(i, c)| i == 8 || c.is_ascii_digit())
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
                && chrono::NaiveDate::parse_from_str(n.get(..8).unwrap_or_default(), "%Y%m%d")
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

/// Hộp nhận job của đợt đang quét. Watcher `day` vào NGAY tại `enqueue_new`;
/// `bao` đánh thức vòng ghi phiếu liền, không đợi nhịp 2 giây — app bị giết
/// ngay sau khi xếp hàng thì job vẫn kịp vào sổ (1b-D7).
#[derive(Default)]
pub struct HopJob {
    v: Mutex<Vec<EnqueuedJob>>,
    sot: Mutex<Vec<String>>,
    pub bao: tokio::sync::Notify,
}

impl HopJob {
    pub fn day(&self, j: EnqueuedJob) {
        self.v.lock().unwrap_or_else(|e| e.into_inner()).push(j);
        self.bao.notify_one();
    }

    /// Kênh Lịch đành bỏ qua vì chờ khoá quá lâu — báo bot, không lặng lẽ sót.
    pub fn bo_sot(&self, kenh: String) {
        self.sot.lock().unwrap_or_else(|e| e.into_inner()).push(kenh);
        self.bao.notify_one();
    }

    pub fn rut(&self) -> Vec<EnqueuedJob> {
        std::mem::take(&mut *self.v.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn rut_bo_sot(&self) -> Vec<String> {
        std::mem::take(&mut *self.sot.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

pub type JobSink = Arc<HopJob>;

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
        // Đã xét ở trên; viết rõ thay vì unreachable!() (release = abort).
        DownloadState::Completed => QuanSat::Xong { path: it.output_path.clone(), luc: it.finished_at },
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
    let han = now.checked_add_signed(chrono::Duration::hours(han_gio.clamp(1, 24) as i64)).unwrap_or(now);
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
            dot.phieu.kenh.len().saturating_sub(1)
        }
    };
    let Some(kenh) = dot.phieu.kenh.get_mut(k) else { return false };
    if kenh.video.iter().any(|v| v.id == j.video_id) {
        return false;
    }
    kenh.video.push(VideoPhieu {
        id: j.video_id.clone(),
        trang_thai: "cho".into(),
        file: None,
        size: None,
        xong_luc: None,
        loi: None,
    });
    let v = kenh.video.len().saturating_sub(1);
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

/// Bấm Dừng: đóng phiếu đang mở `huy`. Video chưa có kết quả ghi `tre` (file
/// đang tải KHÔNG bị đụng — xong vẫn được cắt theo thư mục); job đã bị huỷ
/// theo lựa chọn thì `cap_nhat_dot` đã ghi `loi` "đã huỷ" trước đó.
pub fn dong_huy(dot: &mut DotNoiBo, now: DateTime<FixedOffset>) -> bool {
    if !dot.dang_mo() {
        return false;
    }
    for k in dot.phieu.kenh.iter_mut() {
        for v in k.video.iter_mut() {
            if v.trang_thai == "cho" || v.trang_thai == "dang_tai" {
                v.trang_thai = "tre".into();
            }
        }
    }
    dong(dot, "huy", now);
    dot.phieu.seq += 1;
    dot.can_ghi = true;
    true
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
    /// Loại (bot gửi tối đa 1 tin/giờ mỗi loại): `o_day_giua`, `tre`, `ket`, `bo_sot`…
    #[serde(default)]
    pub loai: String,
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
    /// Ngày (YYYY-MM-DD) anh bấm "Bỏ qua hôm nay".
    #[serde(default)]
    pub bo_qua_ngay: Option<String>,
    /// Lý do hoãn / dừng gần nhất (kèm giờ) — hiện ở tin trạng thái.
    #[serde(default)]
    pub ly_do_hoan: Option<String>,
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
    let hom_nay = ngay_cua(now);
    match st.ngay.as_deref() {
        Some(n) if n == hom_nay && st.xong_ngay => return false,
        // Đồng hồ bị chỉnh lùi mà chưa qua `vao_ngay`: không chạy lại.
        Some(n) if n > hom_nay.as_str() => return false,
        _ => {}
    }
    if st.bo_qua_ngay.as_deref() == Some(hom_nay.as_str()) {
        return false;
    }
    now.time() >= t
}

fn ngay_cua(now: DateTime<FixedOffset>) -> String {
    now.format("%Y-%m-%d").to_string()
}

/// Lần Lịch tải chạy tới (tin trạng thái). Đã đến giờ mà chưa chạy → `now`.
/// `None` khi Lịch tắt hoặc giờ sai.
pub fn lan_chay_toi(now: DateTime<FixedOffset>, gio: &str, st: &TrangThaiLich, bat: bool) -> Option<DateTime<FixedOffset>> {
    let t = parse_gio(gio).filter(|_| bat)?;
    let hom_nay = ngay_cua(now);
    let xong = (st.ngay.as_deref() == Some(hom_nay.as_str()) && st.xong_ngay)
        || st.bo_qua_ngay.as_deref() == Some(hom_nay.as_str());
    let ngay = if xong { now.date_naive().succ_opt()? } else { now.date_naive() };
    let at = ngay.and_time(t).and_local_timezone(*now.offset()).single()?;
    Some(at.max(now))
}

/// Vì sao lượt Lịch dừng trước khi làm hết nhóm.
#[derive(Debug, Clone, PartialEq)]
pub enum LyDoDung {
    /// Anh bấm Dừng / Bỏ qua hôm nay.
    NguoiDung,
    /// Lịch bị tắt giữa chừng (lượt tự động).
    TatLich,
    /// Đã sang ngày khác so với lúc bắt đầu (máy ngủ đông qua đêm, qua nửa đêm).
    SangNgay,
    /// Ổ đĩa dưới ngưỡng.
    ODay(String),
}

/// Kiểm trước MỖI nhóm: còn được quét tiếp không. Ưu tiên: lệnh Dừng → Lịch
/// tắt → sang ngày khác → ổ đầy. `None` = quét tiếp.
pub fn truoc_nhom(
    dung: bool,
    cho_phep: bool,
    ngay_chay: &str,
    now: DateTime<FixedOffset>,
    o_day: Option<String>,
) -> Option<LyDoDung> {
    if dung {
        Some(LyDoDung::NguoiDung)
    } else if !cho_phep {
        Some(LyDoDung::TatLich)
    } else if ngay_cua(now) != ngay_chay {
        Some(LyDoDung::SangNgay)
    } else {
        o_day.map(LyDoDung::ODay)
    }
}

/// Nhóm trong Lịch không còn tồn tại (đã đổi tên / xoá). "" luôn còn.
pub fn nhom_mat(lich: &[String], co: &[String]) -> Vec<String> {
    lich.iter().filter(|g| !g.is_empty() && !co.contains(g)).cloned().collect()
}

/// Ghi phiếu / sổ lỗi liên tục quá ngần này mới báo bot ("kéo dài").
pub const GHI_LOI_KEO_DAI: Duration = Duration::from_secs(600);
/// 1 lượt Lịch chạy quá ngần này coi như KẸT.
pub const QUET_KET: Duration = Duration::from_secs(6 * 3600);

/// Đầu vào của `tinh_van_de` — chụp nhanh tình trạng hiện tại.
#[derive(Debug, Default, Clone)]
pub struct DauVaoVanDe {
    pub lich_bat: bool,
    pub nhom_rong: bool,
    pub ly_do_chan: Option<String>,
    pub nhom_mat: Vec<String>,
    pub ghi_loi: Option<(Duration, String)>,
    pub quet_lau: Option<Duration>,
    pub loi_tat: Option<String>,
}

/// Lỗi ĐANG CÓ theo loại (bot báo tối đa 1 lần/giờ mỗi loại; loại biến mất
/// → bot báo "đã ổn lại"). Sự kiện 1 lần (ổ đầy giữa chừng, đợt trễ…) đi
/// đường `canh_bao`.
pub fn tinh_van_de(d: &DauVaoVanDe) -> std::collections::BTreeMap<String, String> {
    let mut m = std::collections::BTreeMap::new();
    if d.lich_bat {
        if d.nhom_rong {
            m.insert("khong_chay".into(), "Lịch tải không chạy được: danh sách nhóm đang trống — vào Cài đặt chọn nhóm.".into());
        } else if let Some(l) = &d.ly_do_chan {
            m.insert("khong_chay".into(), format!("Lịch tải không chạy được: {l}"));
        }
        if !d.nhom_mat.is_empty() {
            let ds: Vec<String> = d.nhom_mat.iter().map(|g| format!("«{g}»")).collect();
            m.insert(
                "nhom_mat".into(),
                format!("Nhóm trong Lịch tải không còn: {} — Lịch bỏ qua nhóm này, vào Cài đặt sửa danh sách.", ds.join(", ")),
            );
        }
    }
    if let Some((bao_lau, msg)) = &d.ghi_loi {
        if *bao_lau >= GHI_LOI_KEO_DAI {
            m.insert("ghi_loi".into(), format!("Ghi phiếu đợt / lich_tai.json lỗi đã {} phút: {msg}", bao_lau.as_secs() / 60));
        }
    }
    if let Some(t) = d.quet_lau.filter(|t| *t >= QUET_KET) {
        m.insert("ket".into(), format!("Lượt Lịch tải chạy đã {} giờ chưa xong — bộ lịch có thể bị kẹt.", t.as_secs() / 3600));
    }
    if let Some(e) = &d.loi_tat {
        m.insert("loi_tat".into(), format!("Không tạo được lối tắt Mở cùng Windows: {e}"));
    }
    m
}

/// Sang ngày mới thì xoá sổ "đã quét" của ngày cũ (đợt cũ còn mở vẫn theo dõi
/// tiếp tới khi đóng). Đợt đã đóng từ trước hôm qua thì bỏ khỏi sổ nội bộ
/// (file phiếu trên đĩa vẫn giữ 14 ngày).
/// Trả `true` khi sổ đổi.
pub fn vao_ngay(st: &mut TrangThaiLich, now: DateTime<FixedOffset>) -> bool {
    let truoc = st.clone();
    let hom_nay = ngay_cua(now);
    match st.ngay.as_deref() {
        Some(n) if n == hom_nay => {}
        // Đồng hồ máy bị chỉnh LÙI: phần đã quét là làm "hôm nay" thật → giữ
        // nguyên tiến độ, chỉ sửa ngày (không quét lại cả lượt).
        Some(n) if n > hom_nay.as_str() => st.ngay = Some(hom_nay.clone()),
        _ => {
            st.ngay = Some(hom_nay.clone());
            st.nhom_da_quet.clear();
            st.xong_ngay = false;
            st.ly_do_chan = None;
        }
    }
    if st.bo_qua_ngay.as_deref().is_some_and(|b| b > hom_nay.as_str()) {
        st.bo_qua_ngay = Some(hom_nay);
    }
    let hom_qua = now.checked_sub_signed(chrono::Duration::days(1)).unwrap_or(now).format("%Y%m%d").to_string();
    // `get(..8)`: sổ sửa tay / hỏng có `dot_id` ngắn hoặc không phải ASCII thì
    // không được làm panic cả bộ lịch (1b-D4) — giữ lại cho chắc.
    st.dot.retain(|d| d.dang_mo() || d.phieu.dot_id.get(..8).map_or(true, |n| n >= hom_qua.as_str()));
    let cut = now.checked_sub_signed(chrono::Duration::days(2)).unwrap_or(now);
    st.canh_bao.retain(|c| DateTime::parse_from_rfc3339(&c.luc).is_ok_and(|t| t >= cut));
    *st != truoc
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
    dang_quet: AtomicBool,
    /// Ổ đầy / lượt trước panic → chưa chạy lại trước mốc này (10 phút).
    hoan_den: Mutex<Option<Instant>>,
    /// Cờ của lượt đang chạy (Dừng / Bỏ qua / đã chốt) — đọc-ghi CÙNG khoá với
    /// việc chiếm/nhả lượt, không có khe làm lệnh Dừng bị xoá mất (S3).
    co: Mutex<CoLuot>,
    /// Đã ghi nốt khi thoát — `Destroyed` gọi lần 2 SAU `queue.shutdown()`
    /// không được ghi đè (job bị ngắt sẽ trông như "đã huỷ") (S5).
    da_thoat: AtomicBool,
    bat_dau_quet: Mutex<Option<Instant>>,
    /// Lần ghi đĩa lỗi đầu tiên chưa được ghi lại thành công + lỗi gần nhất.
    ghi_loi: Mutex<Option<(Instant, String)>>,
    /// Giờ nhịp cuối của vòng lịch — bot thấy cũ quá thì báo "bộ lịch kẹt".
    nhip_cuoi: Mutex<Option<String>>,
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
            dang_quet: AtomicBool::new(false),
            hoan_den: Mutex::new(None),
            co: Mutex::new(CoLuot::default()),
            da_thoat: AtomicBool::new(false),
            bat_dau_quet: Mutex::new(None),
            ghi_loi: Mutex::new(None),
            nhip_cuoi: Mutex::new(None),
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
        let moi: Option<(String, Vec<EnqueuedJob>, Vec<String>)> = self
            .sink
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|(id, h)| (id.clone(), h.rut(), h.rut_bo_sot()));
        let mut st = self.lock();
        if st.dot.iter().all(|d| !d.dang_mo()) && moi.as_ref().map_or(true, |(_, j, b)| j.is_empty() && b.is_empty()) {
            return;
        }
        let truoc = st.dot.clone();
        let mut su_kien: Vec<(&str, String, String)> = Vec::new();
        for d in st.dot.iter_mut() {
            if let Some((id, jobs, _)) = &moi {
                if &d.phieu.dot_id == id {
                    for j in jobs {
                        them_job(d, j);
                    }
                }
            }
            let mo = d.dang_mo();
            cap_nhat_dot(d, qs, size, now);
            if mo && !d.dang_mo() && d.phieu.ly_do_dong.as_deref() == Some("het_han") {
                let tre = d.phieu.kenh.iter().flat_map(|k| &k.video).filter(|v| v.trang_thai == "tre").count();
                if tre > 0 {
                    su_kien.push((
                        "tre",
                        format!("tre-{}", d.phieu.dot_id),
                        format!(
                            "Đợt {} ({}) đóng vì quá hạn chót, còn {tre} video trễ — vẫn tải tiếp, xong vẫn được cắt.",
                            d.phieu.dot_id, d.phieu.nhom
                        ),
                    ));
                }
            }
        }
        if let Some((id, _, sot)) = &moi {
            for k in sot {
                su_kien.push((
                    "bo_sot",
                    format!("bo-sot-{id}-{k}"),
                    format!("Đợt {id}: bỏ sót kênh {k} (chờ lượt quét khác của kênh này quá 60 phút)."),
                ));
            }
        }
        let co_su_kien = !su_kien.is_empty();
        for (loai, id, text) in su_kien {
            Self::them_canh_bao_loai(&mut st, loai, id, text, now);
        }
        if st.dot != truoc || co_su_kien {
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
        let co_ghi = !phieu.is_empty() || so.is_some();
        let mut loi_msg: Option<String> = None;
        if !phieu.is_empty() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                log_mot_lan("phieu-thu-muc", &format!("[daychuyen] không tạo được {}: {e}", dir.display()));
                loi_msg = Some(format!("không tạo được thư mục {}: {e}", dir.display()));
                loi = phieu.iter().map(|(id, _)| id.clone()).collect();
            } else {
                for (id, data) in &phieu {
                    if let Err(e) = ghi_nguyen_tu(&dir.join(format!("{id}.json")), data) {
                        log_mot_lan(&format!("phieu-ghi-{id}"), &format!("[daychuyen] ghi phiếu {id} lỗi (sẽ ghi lại): {e}"));
                        loi_msg = Some(format!("ghi phiếu {id}: {e}"));
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
                loi_msg = Some(format!("lưu lich_tai.json: {e}"));
                self.danh_dau_doi();
            }
        }
        if co_ghi {
            let mut g = self.ghi_loi.lock().unwrap_or_else(|e| e.into_inner());
            match loi_msg {
                Some(m) => *g = Some((g.as_ref().map_or_else(Instant::now, |(t, _)| *t), m)),
                None => *g = None,
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

    #[allow(dead_code)]
    fn them_canh_bao(st: &mut TrangThaiLich, id: String, text: String, now: DateTime<FixedOffset>) {
        Self::them_canh_bao_loai(st, "o_day", id, text, now);
    }

    fn them_canh_bao_loai(st: &mut TrangThaiLich, loai: &str, id: String, text: String, now: DateTime<FixedOffset>) {
        if !st.canh_bao.iter().any(|c| c.id == id) {
            st.canh_bao.push(CanhBao { id, luc: iso(now), text, loai: loai.into() });
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

    /// Thoát êm: rút nốt job trong hộp (kể cả job xếp hàng trong 2 giây cuối),
    /// đối chiếu hàng đợi rồi ghi phiếu + sổ — GỌI TRƯỚC `queue.shutdown()`
    /// (sau đó mục đang tải bị ngắt sẽ trông như "đã huỷ").
    /// CHỈ CHẠY 1 LẦN — trả `false` nếu đã chạy (lần gọi thứ 2 từ `Destroyed`).
    pub fn thoat_em(&self, queue: &crate::queue::QueueManager, history: &crate::history_store::HistoryStore, dir: &Path) -> bool {
        let qs = |job: &str| quan_sat_job(queue.get(job).as_ref(), history.get(job).ok().flatten().as_ref());
        let size = |p: &Path| std::fs::metadata(p).ok().map(|m| m.len());
        self.thoat_em_voi(&qs, &size, chrono::Local::now().fixed_offset(), dir)
    }

    pub fn thoat_em_voi(
        &self,
        qs: &dyn Fn(&str) -> QuanSat,
        size: &dyn Fn(&Path) -> Option<u64>,
        now: DateTime<FixedOffset>,
        dir: &Path,
    ) -> bool {
        if self.da_thoat.swap(true, Ordering::SeqCst) {
            return false;
        }
        self.cap_nhat_voi(qs, size, now);
        self.ghi_xuong_dia(dir);
        true
    }

    pub fn dang_quet(&self) -> bool {
        self.dang_quet.load(Ordering::SeqCst)
    }

    fn co_lock(&self) -> std::sync::MutexGuard<'_, CoLuot> {
        self.co.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Chụp cờ lượt hiện tại (test + tin trạng thái).
    pub fn co_luot(&self) -> CoLuot {
        *self.co_lock()
    }

    /// Chiếm lượt chạy. Bấm nhiều lần / bộ lịch + nút cùng lúc → chỉ 1 thắng.
    /// Cờ Dừng của lượt cũ được xoá TRONG khoá cờ, trước khi ai kịp bấm Dừng
    /// cho lượt mới (S3). Mỗi lần chiếm phải đi kèm 1 `GiuQuet` để nhả.
    pub fn chiem_luot(&self) -> bool {
        let mut co = self.co_lock();
        if self.dang_quet.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return false;
        }
        *co = CoLuot::default();
        LUOT_LICH.fetch_add(1, Ordering::SeqCst);
        true
    }

    /// Nút "Chạy ngay" (app + bot): chạy các nhóm hôm nay CHƯA quét, bỏ qua giờ
    /// bắt đầu và lệnh "Bỏ qua hôm nay". Ok = đã chiếm lượt — người gọi chạy
    /// `khoi_chay(.., tay = true)`. Nhóm đã quét hôm nay không quét lại.
    pub fn chuan_bi_chay_tay(&self, s: &Settings, nhom_co: &[String], now: DateTime<FixedOffset>) -> Result<String, String> {
        if self.dang_quet() {
            return Err(Self::bao_trung());
        }
        if s.lich_tai_nhom.is_empty() {
            return Err("Chưa chọn nhóm nào cho Lịch tải — vào Cài đặt thêm nhóm trước.".into());
        }
        let con_lai: Vec<String> = {
            let mut st = self.lock();
            if vao_ngay(&mut st, now) {
                self.danh_dau_doi();
            }
            let con = nhom_can_quet(&s.lich_tai_nhom, &st.nhom_da_quet);
            let mat = nhom_mat(&con, nhom_co);
            con.into_iter().filter(|g| !mat.contains(g)).collect()
        };
        if con_lai.is_empty() {
            return Err("Hôm nay đã quét xong mọi nhóm của Lịch tải (hoặc nhóm không còn) — không có gì để chạy.".into());
        }
        if !self.chiem_luot() {
            return Err(Self::bao_trung());
        }
        {
            let mut st = self.lock();
            st.bo_qua_ngay = None;
            st.xong_ngay = false;
        }
        *self.hoan_den.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.danh_dau_doi();
        let ds: Vec<String> = con_lai.iter().map(|g| ten_nhom_hien_thi(g)).collect();
        Ok(format!("Bắt đầu Lịch tải {} nhóm: {}", ds.len(), ds.join(" → ")))
    }

    fn bao_trung() -> String {
        format!(
            "Lịch tải đang chạy rồi ({}) — không chạy thêm lượt thứ 2.",
            dot_dang_chay().unwrap_or_else(|| "đang chuẩn bị".into())
        )
    }

    /// Nút "Dừng" (app/bot đã hỏi xác nhận). KHÔNG đụng file đang tải.
    /// Lượt đang chạy (chưa chốt) → đặt cờ: nhóm đang quét xong thì dừng, không
    /// xếp nhóm sau, rồi đóng phiếu `huy` (+ huỷ job CHƯA BẮT ĐẦU nếu `huy_job`);
    /// hôm nay coi như xong (muốn chạy tiếp bấm Chạy ngay). Không có lượt (hoặc
    /// lượt đã qua bước kiểm tra cuối) mà còn phiếu mở → trả `(.., true)`: người
    /// gọi đóng NGAY; lượt Lịch hôm nay KHÔNG bị đụng (S7 — vd 01:00 đóng đợt
    /// đêm qua không được làm mất lượt 06:00).
    pub fn yeu_cau_dung(&self, huy_job: bool, now: DateTime<FixedOffset>) -> Result<(String, bool), String> {
        let lua_chon = |h: bool| if h { "huỷ job chưa tải" } else { "giữ job đã xếp" };
        {
            let mut co = self.co_lock();
            if self.dang_quet() && !co.chot {
                if co.dong_huy {
                    return Ok((
                        format!(
                            "Đã nhận lệnh Dừng từ trước — đang chờ nhóm đang quét xong (lựa chọn đang dùng: {}).",
                            lua_chon(co.huy_job)
                        ),
                        false,
                    ));
                }
                co.dung = true;
                co.dong_huy = true;
                co.huy_job = huy_job;
                drop(co);
                {
                    let mut st = self.lock();
                    vao_ngay(&mut st, now);
                    st.xong_ngay = true;
                    st.ly_do_hoan = Some(format!("{} — anh bấm Dừng", now.format("%H:%M")));
                }
                self.danh_dau_doi();
                return Ok((
                    format!(
                        "Đã nhận lệnh Dừng ({}): nhóm đang quét xong thì dừng, không xếp nhóm sau; phiếu đợt ghi «huy». \
                         Hôm nay Lịch không tự chạy lại (muốn chạy tiếp: Chạy ngay).",
                        lua_chon(huy_job)
                    ),
                    false,
                ));
            }
        }
        let n_mo = self.lock().dot.iter().filter(|d| d.dang_mo()).count();
        if n_mo == 0 {
            return Err(if self.dang_quet() {
                "Lượt Lịch tải vừa xong — không còn đợt nào đang mở để dừng.".into()
            } else {
                "Không có lượt Lịch tải nào đang chạy, cũng không còn đợt nào đang mở.".into()
            });
        }
        self.lock().ly_do_hoan = Some(format!("{} — anh bấm Dừng (đóng {n_mo} đợt đang mở)", now.format("%H:%M")));
        self.danh_dau_doi();
        Ok((
            format!(
                "Không có lượt đang quét — đã đóng {n_mo} đợt đang mở («huy», {}), file đang tải vẫn tải tiếp. \
                 Lượt Lịch hôm nay KHÔNG bị ảnh hưởng.",
                lua_chon(huy_job)
            ),
            true,
        ))
    }

    /// Nút "Bỏ qua hôm nay": hôm nay không tự chạy nữa (mai chạy lại đúng giờ).
    /// Đang quét → nhóm đang quét xong thì dừng; phiếu đang mở để tự đóng.
    pub fn bo_qua_hom_nay(&self, now: DateTime<FixedOffset>) -> String {
        let hom_nay = ngay_cua(now);
        {
            let mut st = self.lock();
            vao_ngay(&mut st, now);
            if st.bo_qua_ngay.as_deref() == Some(hom_nay.as_str()) {
                return "Hôm nay đã bấm Bỏ qua từ trước — mai Lịch chạy lại đúng giờ.".into();
            }
            st.bo_qua_ngay = Some(hom_nay);
            st.ly_do_hoan = Some(format!("{} — anh bấm Bỏ qua hôm nay", now.format("%H:%M")));
        }
        self.danh_dau_doi();
        let mut co = self.co_lock();
        if self.dang_quet() && !co.chot {
            co.dung = true;
            return "Đã bỏ qua hôm nay: nhóm đang quét xong thì dừng, không xếp nhóm sau. Mai chạy lại đúng giờ.".into();
        }
        "Đã bỏ qua Lịch tải hôm nay — mai chạy lại đúng giờ (muốn chạy hôm nay thì bấm Chạy ngay).".into()
    }

    /// Đóng MỌI phiếu đang mở `huy`. `huy` (tuỳ chọn) huỷ 1 job CHƯA BẮT ĐẦU,
    /// trả `true` nếu đã huỷ. Trả (số phiếu đóng, số job huỷ).
    pub fn dong_huy_voi(
        &self,
        huy: Option<&dyn Fn(&str) -> bool>,
        qs: &dyn Fn(&str) -> QuanSat,
        size: &dyn Fn(&Path) -> Option<u64>,
        now: DateTime<FixedOffset>,
    ) -> (usize, usize) {
        self.cap_nhat_voi(qs, size, now);
        let mut so_job = 0;
        if let Some(huy) = huy {
            let ids: Vec<String> = self
                .lock()
                .dot
                .iter()
                .filter(|d| d.dang_mo())
                .flat_map(|d| d.jobs.iter().filter(|r| !r.cuoi).map(|r| r.job.clone()))
                .collect();
            so_job = ids.iter().filter(|j| huy(j)).count();
            self.cap_nhat_voi(qs, size, now);
        }
        let n = self.lock().dot.iter_mut().map(|d| dong_huy(d, now)).filter(|x| *x).count();
        if n > 0 {
            self.danh_dau_doi();
        }
        (n, so_job)
    }

    /// Lượt chạy bị panic: ghi lý do, báo bot, hoãn 10 phút mới thử lại.
    pub fn bao_panic(&self, msg: &str, now: DateTime<FixedOffset>) {
        {
            let mut st = self.lock();
            st.ly_do_hoan = Some(format!("{} — lượt Lịch tải lỗi bất ngờ: {msg}", now.format("%H:%M")));
            Self::them_canh_bao_loai(
                &mut st,
                "ket",
                format!("ket-{}", now.format("%Y%m%d%H%M%S")),
                format!("Bộ Lịch tải gặp lỗi bất ngờ và đã tự nhả để chạy lại sau 10 phút: {msg}"),
                now,
            );
        }
        *self.hoan_den.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now().checked_add(Duration::from_secs(600));
        self.danh_dau_doi();
    }

    pub fn van_de(&self, s: &Settings, nhom_co: &[String]) -> std::collections::BTreeMap<String, String> {
        let st = self.snapshot();
        tinh_van_de(&DauVaoVanDe {
            lich_bat: s.lich_tai_enabled,
            nhom_rong: s.lich_tai_nhom.is_empty(),
            ly_do_chan: st.ly_do_chan,
            nhom_mat: nhom_mat(&s.lich_tai_nhom, nhom_co),
            ghi_loi: self.ghi_loi.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|(t, m)| (t.elapsed(), m.clone())),
            quet_lau: self.bat_dau_quet.lock().unwrap_or_else(|e| e.into_inner()).map(|t| t.elapsed()),
            loi_tat: if s.mo_cung_windows { crate::startup_shortcut::loi_gan_nhat() } else { None },
        })
    }

    /// Tình trạng cho app + bot (`/api/daychuyen`): tin trạng thái tự sửa đọc cái này.
    pub fn trang_thai_json(&self, s: &Settings, nhom_co: &[String], now: DateTime<FixedOffset>) -> serde_json::Value {
        let st = self.snapshot();
        let hom_nay8 = now.format("%Y%m%d").to_string();
        let dots: Vec<serde_json::Value> = st
            .dot
            .iter()
            .map(|d| {
                let vids: Vec<&VideoPhieu> = d.phieu.kenh.iter().flat_map(|k| &k.video).collect();
                let dem = |tt: &str| vids.iter().filter(|v| v.trang_thai == tt).count();
                serde_json::json!({
                    "dotId": d.phieu.dot_id, "nhom": d.phieu.nhom, "trangThai": d.phieu.trang_thai,
                    "lyDoDong": d.phieu.ly_do_dong, "hanChot": d.phieu.han_chot, "video": vids.len(),
                    "xong": dem("xong"), "loi": dem("loi"), "boQua": dem("bo_qua"), "tre": dem("tre"),
                })
            })
            .collect();
        let tre: usize = st
            .dot
            .iter()
            .filter(|d| d.phieu.dot_id.starts_with(&hom_nay8))
            .flat_map(|d| d.phieu.kenh.iter().flat_map(|k| &k.video))
            .filter(|v| v.trang_thai == "tre")
            .count();
        let van_de: Vec<serde_json::Value> = self
            .van_de(s, nhom_co)
            .into_iter()
            .map(|(loai, text)| serde_json::json!({ "loai": loai, "text": text }))
            .collect();
        let hom_nay = ngay_cua(now);
        serde_json::json!({
            "ok": true,
            "lichTaiEnabled": s.lich_tai_enabled,
            "gio": s.lich_tai_gio,
            "hanChotGio": s.lich_tai_han_chot_gio,
            "nhom": s.lich_tai_nhom,
            "nhomCo": nhom_co,
            "dangChay": dot_dang_chay(),
            "dangQuet": self.dang_quet(),
            "dangDung": self.dang_quet() && self.co_luot().dung,
            "lanToi": lan_chay_toi(now, &s.lich_tai_gio, &st, s.lich_tai_enabled).map(iso),
            "lyDoHoan": st.ly_do_hoan,
            "boQuaHomNay": st.bo_qua_ngay.as_deref() == Some(hom_nay.as_str()),
            "ngay": st.ngay,
            "xongNgay": st.xong_ngay,
            "nhomDaQuet": if st.ngay.as_deref() == Some(hom_nay.as_str()) { st.nhom_da_quet.clone() } else { Vec::new() },
            "lyDoChan": st.ly_do_chan,
            "dots": dots,
            "dotMo": st.dot.iter().filter(|d| d.dang_mo()).count(),
            "tre": tre,
            "canhBao": st.canh_bao,
            "vanDe": van_de,
            "nhipCuoi": self.nhip_cuoi.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        })
    }
}

/// Nhóm đang tồn tại: danh sách nhóm đã lưu + nhóm các kênh đang mang.
pub fn nhom_dang_co(store: &crate::watchlist_store::WatchlistStore, s: &Settings) -> Vec<String> {
    let mut v = s.watch_groups.clone();
    for c in store.list() {
        if let Some(g) = c.group.filter(|g| !g.is_empty()) {
            if !v.contains(&g) {
                v.push(g);
            }
        }
    }
    v
}

/// Cờ của 1 lượt Lịch.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct CoLuot {
    /// Dừng / Bỏ qua: hết nhóm đang quét thì dừng.
    pub dung: bool,
    /// Dừng kèm ĐÓNG phiếu `huy` (Bỏ qua hôm nay thì không).
    pub dong_huy: bool,
    /// Dừng kèm huỷ job của đợt CHƯA BẮT ĐẦU tải.
    pub huy_job: bool,
    /// Lượt đã qua bước kiểm tra cuối — Dừng bấm sau đó tự đóng phiếu ngay.
    pub chot: bool,
}

/// Số lượt Lịch đang chiếm (mọi `DayChuyen`) — nhịp quét cũ nhường khi > 0.
static LUOT_LICH: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Lịch đang chạy 1 lượt (kể cả Chạy ngay khi công tắc Lịch tắt, và lượt đang
/// chạy dở sau khi anh vừa tắt Lịch) → nhịp quét cũ NHƯỜNG, không chen vào
/// giữa nhóm làm job không vào phiếu (S2).
pub fn lich_dang_chay() -> bool {
    LUOT_LICH.load(Ordering::SeqCst) > 0 || dot_dang_chay().is_some()
}

/// Nhịp quét cũ được chạy NGAY BÂY GIỜ không (thuần, để test).
pub fn nhip_cu_chay_ngay(s: &Settings, lich_chay: bool) -> bool {
    nhip_cu_duoc_chay(s) && !lich_chay
}

/// Cờ "đang quét" + mốc bắt đầu — RƠI là nhả. Bản release là `panic = abort`
/// nên guard này chỉ giúp khi lượt bị huỷ/kết thúc sớm; chống kẹt thật là
/// module không có đường panic (lint đầu file).
struct GiuQuet(Arc<DayChuyen>);

impl GiuQuet {
    fn new(dc: Arc<DayChuyen>) -> Self {
        *dc.bat_dau_quet.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
        GiuQuet(dc)
    }
}

impl Drop for GiuQuet {
    fn drop(&mut self) {
        let dc = &self.0;
        *dc.sink.lock().unwrap_or_else(|e| e.into_inner()) = None;
        *dc.bat_dau_quet.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let mut co = dc.co_lock();
        *co = CoLuot::default();
        if dc.dang_quet.swap(false, Ordering::SeqCst) {
            let _ = LUOT_LICH.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1));
        }
    }
}

fn chu_panic(p: Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".into())
}

/// Chạy 1 lượt (đã `chiem_luot`) trong task riêng: kết thúc kiểu gì cũng nhả
/// cờ (guard). Bản debug/test bắt được panic → ghi lý do, báo bot, hoãn 10
/// phút; bản release panic = abort cả app → bot phát hiện app tắt bất thường.
pub async fn chay_co_bao_ve<F>(dc: Arc<DayChuyen>, fut: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let g = dc.clone();
    let h = tokio::spawn(async move {
        let _giu = GiuQuet::new(g);
        fut.await;
    });
    if let Err(e) = h.await {
        let msg = if e.is_panic() { chu_panic(e.into_panic()) } else { "lượt chạy bị huỷ".into() };
        eprintln!("[daychuyen] lượt Lịch tải lỗi: {msg}");
        dc.bao_panic(&msg, chrono::Local::now().fixed_offset());
    }
}

/// Bắt đầu 1 lượt Lịch (đã `chiem_luot`). `tay` = nút Chạy ngay: chạy cả khi
/// công tắc Lịch đang tắt.
pub fn khoi_chay(
    app: &tauri::AppHandle,
    store: &Arc<crate::watchlist_store::WatchlistStore>,
    queue: &Arc<crate::queue::QueueManager>,
    settings: &Arc<crate::settings_store::SettingsStore>,
    history: &Arc<crate::history_store::HistoryStore>,
    dc: &Arc<DayChuyen>,
    tay: bool,
) {
    let env = MoiTruongThat {
        app: app.clone(),
        store: store.clone(),
        queue: queue.clone(),
        settings: settings.clone(),
        history: history.clone(),
        dc: dc.clone(),
    };
    let dc = dc.clone();
    tauri::async_runtime::spawn(async move {
        let dc2 = dc.clone();
        chay_co_bao_ve(dc, async move {
            chay_luot(&env, &dc2, tay).await;
        })
        .await;
    });
}

/// Nút Dừng ở app/bot (đã hỏi xác nhận). Việc đụng hàng đợi / SQLite / đĩa
/// chạy ở luồng chặn, không giữ worker tokio hay luồng giao diện (S6).
pub async fn dung_that(
    queue: &Arc<crate::queue::QueueManager>,
    history: &Arc<crate::history_store::HistoryStore>,
    dc: &Arc<DayChuyen>,
    s: &Settings,
    huy_job: bool,
) -> Result<String, String> {
    let (msg, dong_ngay) = dc.yeu_cau_dung(huy_job, chrono::Local::now().fixed_offset())?;
    if dong_ngay {
        let (dc, queue, history, dir) = (dc.clone(), queue.clone(), history.clone(), thu_muc_phieu(s));
        let _ = tauri::async_runtime::spawn_blocking(move || {
            huy_va_dong(&queue, &history, &dc, huy_job);
            dc.ghi_xuong_dia(&dir);
        })
        .await;
    }
    Ok(msg)
}

/// Đóng phiếu `huy`; `huy_job` → huỷ job đợt CHƯA BẮT ĐẦU — kiểm "còn Chờ" và
/// huỷ trong CÙNG 1 khoá hàng đợi (`cancel_if_queued`, S8): job vừa bắt đầu
/// tải không bị huỷ nhầm. Gọi ở luồng chặn.
fn huy_va_dong(
    queue: &Arc<crate::queue::QueueManager>,
    history: &crate::history_store::HistoryStore,
    dc: &DayChuyen,
    huy_job: bool,
) {
    let qs = |job: &str| quan_sat_job(queue.get(job).as_ref(), history.get(job).ok().flatten().as_ref());
    let size = |p: &Path| std::fs::metadata(p).ok().map(|m| m.len());
    let huy = |job: &str| queue.cancel_if_queued(job);
    let (n, so_job) =
        dc.dong_huy_voi(huy_job.then_some(&huy as &dyn Fn(&str) -> bool), &qs, &size, chrono::Local::now().fixed_offset());
    eprintln!("[daychuyen] Dừng: đóng {n} phiếu «huy», huỷ {so_job} job chưa bắt đầu");
}

/// Dấu "thoát êm" cho bot: có dấu = anh tự thoát (không phải chết bất thường,
/// bot không tự mở lại). App mở lên thì xoá dấu.
pub const TEN_DAU_THOAT: &str = "thoat_em.json";

pub fn ghi_dau_thoat(config_dir: &Path, now: DateTime<FixedOffset>) {
    let v = serde_json::json!({ "luc": iso(now), "pid": std::process::id() });
    let _ = ghi_nguyen_tu(&config_dir.join(TEN_DAU_THOAT), &serde_json::to_vec(&v).unwrap_or_default());
}

pub fn xoa_dau_thoat(config_dir: &Path) {
    let _ = std::fs::remove_file(config_dir.join(TEN_DAU_THOAT));
}

/// Dấu "đang tự cập nhật" (có HẠN): trình cài kết thúc app KHÔNG qua đường
/// thoát êm — bot thấy dấu này còn hạn thì KHÔNG mở lại bản cũ giữa lúc cài;
/// quá hạn mà app chưa lên thì chỉ báo. App mở lên thì xoá.
pub const TEN_DAU_CAP_NHAT: &str = "dang_cap_nhat.json";
/// 30 phút + làm mới lúc tải xong (trước khi cài): tải chậm không bị báo nhầm.
pub const CAP_NHAT_HAN_PHUT: i64 = 30;

pub fn ghi_dau_cap_nhat(config_dir: &Path, now: DateTime<FixedOffset>) -> std::io::Result<()> {
    let han = now.checked_add_signed(chrono::Duration::minutes(CAP_NHAT_HAN_PHUT)).unwrap_or(now);
    let v = serde_json::json!({ "luc": iso(now), "het_han": iso(han), "pid": std::process::id() });
    ghi_nguyen_tu(&config_dir.join(TEN_DAU_CAP_NHAT), &serde_json::to_vec(&v).unwrap_or_default())
}

pub fn xoa_dau_cap_nhat(config_dir: &Path) {
    let _ = std::fs::remove_file(config_dir.join(TEN_DAU_CAP_NHAT));
}

fn ghi_ban_thoat_vao(dir: &Path, now: DateTime<FixedOffset>, phien_ban: &str) -> bool {
    let mut v = trang_thai_ban(&[], false, Some("app đã thoát".into()), now, phien_ban);
    if let Some(o) = v.as_object_mut() {
        o.insert("ban".into(), serde_json::Value::Bool(false));
    }
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
        .filter(|c| c.enabled && nhom.map_or(true, |g| c.group.as_deref().unwrap_or("") == g))
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
        let mut lan_ghi_ban: Option<Instant> = None;
        let phien_ban = app.package_info().version.to_string();
        loop {
            let s = settings.get();
            let now = chrono::Local::now().fixed_offset();
            *dc.nhip_cuoi.lock().unwrap_or_else(|e| e.into_inner()) = Some(iso(now));
            if vao_ngay(&mut dc.lock(), now) {
                dc.danh_dau_doi();
            }
            let don = ngay_don != Some(now.date_naive());
            if don {
                ngay_don = Some(now.date_naive());
            }
            let ban = if lan_ghi_ban.map_or(true, |t| t.elapsed() >= Duration::from_secs(60)) {
                lan_ghi_ban = Some(Instant::now());
                Some((s.lich_tai_enabled, phien_ban.clone()))
            } else {
                None
            };
            nhip_nen(&dc, &queue, &history, thu_muc_phieu(&s), don, ban).await;
            // Danh sách nhóm trống → không mở lượt (bot báo "không chạy được").
            if s.lich_tai_enabled && !s.lich_tai_nhom.is_empty() {
                let cho = dc
                    .hoan_den
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .is_some_and(|t| Instant::now() < t);
                let chay = !cho && !dc.dang_quet() && den_gio_chay(now, &s.lich_tai_gio, &dc.lock());
                if chay && dc.chiem_luot() {
                    khoi_chay(&app, &store, &queue, &settings, &history, &dc, false);
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

type Fut<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>>;

/// Mọi thứ 1 lượt Lịch cần từ bên ngoài. Bản thật (`MoiTruongThat`) gọi
/// watcher / hàng đợi / đĩa; test dùng bản giả để chạy `chay_luot` ĐẦU-CUỐI
/// (Dừng giữa vòng, nhịp cũ chen, sang ngày…).
pub trait MoiTruong: Send + Sync {
    fn cai_dat(&self) -> Settings;
    fn bay_gio(&self) -> DateTime<FixedOffset>;
    fn nhom_co(&self, s: &Settings) -> Vec<String>;
    fn thu_muc_nhom(&self, s: &Settings, nhom: &str) -> Vec<String>;
    fn o_trong(&self, p: &Path) -> Option<u64>;
    /// Nhãn "đang chạy đợt X" cho nút ▶ / bot (None = hết lượt).
    fn dat_nhan(&self, nhan: Option<String>);
    fn cho_quet_khac(&self) -> Fut<'_>;
    /// Quét 1 nhóm (đúng `watcher::check_all_with`), job đẩy vào `hop`.
    fn quet_nhom(&self, nhom: String, hop: JobSink) -> Fut<'_>;
    /// Rút hộp job + đối chiếu hàng đợi + ghi phiếu/sổ (luồng chặn).
    fn nhip(&self, dir: PathBuf) -> Fut<'_>;
    fn huy_va_dong(&self, huy_job: bool) -> Fut<'_>;
    fn cho_chut(&self) -> Fut<'_>;
}

pub struct MoiTruongThat {
    app: tauri::AppHandle,
    store: Arc<crate::watchlist_store::WatchlistStore>,
    queue: Arc<crate::queue::QueueManager>,
    settings: Arc<crate::settings_store::SettingsStore>,
    history: Arc<crate::history_store::HistoryStore>,
    dc: Arc<DayChuyen>,
}

impl MoiTruong for MoiTruongThat {
    fn cai_dat(&self) -> Settings {
        self.settings.get()
    }
    fn bay_gio(&self) -> DateTime<FixedOffset> {
        chrono::Local::now().fixed_offset()
    }
    fn nhom_co(&self, s: &Settings) -> Vec<String> {
        nhom_dang_co(&self.store, s)
    }
    fn thu_muc_nhom(&self, s: &Settings, nhom: &str) -> Vec<String> {
        thu_muc_cua_nhom(&self.store, s, Some(nhom)).into_iter().map(|(f, _)| f).collect()
    }
    fn o_trong(&self, p: &Path) -> Option<u64> {
        dung_luong_trong(p)
    }
    fn dat_nhan(&self, nhan: Option<String>) {
        dat_dot_dang_chay(nhan);
    }
    fn cho_quet_khac(&self) -> Fut<'_> {
        Box::pin(cho_luot_quet_khac())
    }
    fn quet_nhom(&self, nhom: String, hop: JobSink) -> Fut<'_> {
        Box::pin(async move {
            let _ = crate::watcher::check_all_with(
                &self.app, &self.store, &self.queue, &self.settings, &self.history, Some(nhom), Some(hop),
            )
            .await;
        })
    }
    fn nhip(&self, dir: PathBuf) -> Fut<'_> {
        Box::pin(nhip_nen(&self.dc, &self.queue, &self.history, dir, false, None))
    }
    fn huy_va_dong(&self, huy_job: bool) -> Fut<'_> {
        let (q, h, dc) = (self.queue.clone(), self.history.clone(), self.dc.clone());
        Box::pin(async move {
            let _ = tauri::async_runtime::spawn_blocking(move || huy_va_dong(&q, &h, &dc, huy_job)).await;
        })
    }
    fn cho_chut(&self) -> Fut<'_> {
        Box::pin(tokio::time::sleep(Duration::from_secs(2)))
    }
}

/// Nhãn "đang chạy" gỡ khi rơi (kể cả return sớm) — như `CoChay`, qua môi trường.
struct NhanChay<'a, E: MoiTruong>(&'a E);

impl<E: MoiTruong> NhanChay<'_, E> {
    fn doi(&self, nhan: String) {
        self.0.dat_nhan(Some(nhan));
    }
}

impl<E: MoiTruong> Drop for NhanChay<'_, E> {
    fn drop(&mut self) {
        self.0.dat_nhan(None);
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

/// 1 lượt Lịch tải: quét LẦN LƯỢT từng nhóm (mỗi nhóm 1 phiếu), KIỂM Ổ trước
/// MỖI nhóm. Nhóm trước xếp hết vào hàng là sang nhóm sau — không chờ tải
/// xong. Không giữ khoá `CHECKING` xuyên nhóm (mỗi nhóm 1 lần `check_all`),
/// không gọi `reconcile_all` (check_channel tự gọi như mọi lượt quét).
/// Cài đặt đọc lại MỖI nhóm: đổi thứ tự / hạn chót giữa chừng có hiệu lực từ
/// nhóm kế. Sang ngày khác giữa chừng (ngủ đông qua đêm) → dừng lượt cũ, lượt
/// của ngày mới chạy lại từ đầu, không chồng lên nhau.
pub async fn chay_luot<E: MoiTruong>(env: &E, dc: &Arc<DayChuyen>, tay: bool) {
    // Cờ chặn quét tay bật NGAY từ bước kiểm ổ, giữ liền tới hết nhóm cuối.
    let co = NhanChay(env);
    co.doi("chuẩn bị (kiểm tra ổ đĩa)".into());
    let bat_dau = env.bay_gio();
    let ngay_chay = ngay_cua(bat_dau);
    vao_ngay(&mut dc.lock(), bat_dau);
    dc.danh_dau_doi();
    let mut dir: PathBuf;
    let mut da_lam: Vec<String> = Vec::new();
    let mut ly_do: Option<LyDoDung> = None;
    loop {
        let s = env.cai_dat();
        dir = thu_muc_phieu(&s);
        let co_nhom = env.nhom_co(&s);
        let con_lai: Vec<String> = nhom_can_quet(&s.lich_tai_nhom, &dc.lock().nhom_da_quet)
            .into_iter()
            .filter(|g| !da_lam.contains(g))
            .collect();
        let mat = nhom_mat(&con_lai, &co_nhom);
        let Some(nhom) = con_lai.into_iter().find(|g| !mat.contains(g)) else { break };
        let now = env.bay_gio();
        // #20 + 1b-D5: ổ trống trước MỖI nhóm (ổ của nhóm đó + ổ C).
        let thu_muc = env.thu_muc_nhom(&s, &nhom);
        let folders: Vec<PathBuf> = thu_muc.iter().map(PathBuf::from).collect();
        let o = kiem_tra_o_dia(&folders, &|p| env.o_trong(p));
        if let Some(l) = truoc_nhom(dc.co_luot().dung, tay || s.lich_tai_enabled, &ngay_chay, now, o) {
            ly_do = Some(l);
            break;
        }
        if dc.lock().ly_do_chan.take().is_some() {
            dc.danh_dau_doi();
        }
        co.doi(format!("nhóm {} (chờ lượt quét tay xong)", ten_nhom_hien_thi(&nhom)));
        let dot_id = mo_dot_sau_khi_cho(
            env.cho_quet_khac(),
            || env.bay_gio(),
            |now| mo_hoac_noi_dot(&mut dc.lock(), &nhom, now, s.lich_tai_han_chot_gio, &dir),
        )
        .await;
        dc.danh_dau_doi();
        co.doi(format!("{dot_id} ({})", ten_nhom_hien_thi(&nhom)));
        env.nhip(dir.clone()).await;
        let sink: JobSink = Arc::default();
        *dc.sink.lock().unwrap_or_else(|e| e.into_inner()) = Some((dot_id.clone(), sink.clone()));
        let mut fut = env.quet_nhom(nhom.clone(), sink.clone());
        // Trong lúc quét: job vừa xếp hàng là ghi phiếu NGAY (hộp `bao`), và
        // 2 giây/lần đối chiếu hàng đợi — app bị giết giữa chừng thì job đã
        // xếp hàng vẫn có trong sổ (D16, 1b-D7).
        loop {
            tokio::select! {
                _ = &mut fut => break,
                _ = sink.bao.notified() => env.nhip(dir.clone()).await,
                _ = env.cho_chut() => env.nhip(dir.clone()).await,
            }
        }
        env.nhip(dir.clone()).await;
        *dc.sink.lock().unwrap_or_else(|e| e.into_inner()) = None;
        {
            let mut st = dc.lock();
            if let Some(d) = st.dot.iter_mut().find(|d| d.phieu.dot_id == dot_id) {
                chot_quet(d, &thu_muc);
            }
            // Đã sang ngày khác (sổ đã qua `vao_ngay` của ngày mới) thì KHÔNG
            // ghi nhóm này vào "đã quét" của ngày mới.
            if st.ngay.as_deref() == Some(ngay_chay.as_str()) && !st.nhom_da_quet.contains(&nhom) {
                st.nhom_da_quet.push(nhom.clone());
            }
        }
        dc.danh_dau_doi();
        env.nhip(dir.clone()).await;
        da_lam.push(nhom);
    }
    let now = env.bay_gio();
    let hm = now.format("%H:%M");
    let s = env.cai_dat();
    {
        let mat = nhom_mat(&s.lich_tai_nhom, &env.nhom_co(&s));
        let mut st = dc.lock();
        match &ly_do {
            None => {
                if st.ngay.as_deref() == Some(ngay_chay.as_str()) && (tay || s.lich_tai_enabled) {
                    st.xong_ngay = true;
                }
                if !mat.is_empty() {
                    st.ly_do_hoan = Some(format!("{hm} — bỏ qua nhóm không còn: {}", mat.join(", ")));
                }
            }
            Some(LyDoDung::NguoiDung) => {} // lý do đã ghi lúc bấm nút
            Some(LyDoDung::TatLich) => {
                st.ly_do_hoan = Some(format!("{hm} — Lịch tải bị tắt giữa chừng, dừng sau nhóm đang chạy"));
            }
            Some(LyDoDung::SangNgay) => {
                st.ly_do_hoan = Some(format!(
                    "{hm} — đã sang ngày mới giữa lượt (máy ngủ qua đêm?): dừng lượt ngày {ngay_chay}, lượt hôm nay chạy lại từ đầu"
                ));
            }
            Some(LyDoDung::ODay(l)) => {
                if st.ly_do_chan.as_deref() != Some(l.as_str()) {
                    log_mot_lan(&format!("o-dia-{}", now.format("%Y%m%d")), &format!("[daychuyen] {l}"));
                }
                st.ly_do_chan = Some(l.clone());
                st.ly_do_hoan = Some(format!("{hm} — {l}"));
                if !da_lam.is_empty() {
                    DayChuyen::them_canh_bao_loai(
                        &mut st,
                        "o_day_giua",
                        format!("o-day-giua-{}", now.format("%Y%m%d%H%M")),
                        format!("Ổ đầy giữa chừng — đã dừng, không quét các nhóm sau. {l}"),
                        now,
                    );
                }
                *dc.hoan_den.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now().checked_add(Duration::from_secs(600));
            }
        }
    }
    dc.danh_dau_doi();
    // CHỐT cờ trong khoá: Dừng bấm SAU dòng này không gắn vào lượt này nữa mà
    // tự đóng phiếu ngay (`yeu_cau_dung` → dong_ngay) — không bị mất (S3).
    let cuoi = {
        let mut c = dc.co_lock();
        c.chot = true;
        *c
    };
    if cuoi.dung && cuoi.dong_huy {
        env.huy_va_dong(cuoi.huy_job).await;
    }
    env.nhip(dir).await;
    drop(co);
}

// ───────────────────────── test ─────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]
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

    // ───────────── Đợt 1b: nút điều khiển + mọi lối chạy ─────────────

    fn dc_co_dot(ten: &str, jobs: &[(&str, &str)]) -> (PathBuf, Arc<DayChuyen>) {
        let (d, dc) = dc_tam(ten);
        {
            let mut st = dc.lock();
            vao_ngay(&mut st, t(MO));
            let mut dot = mo_dot("20261104-01".into(), "A", t(MO), 3);
            for (id, f) in jobs {
                them_job(&mut dot, &job(id, f));
            }
            st.dot.push(dot);
        }
        (d, dc)
    }

    fn st_lich(nhom: &[&str]) -> Settings {
        Settings {
            lich_tai_enabled: true,
            lich_tai_nhom: nhom.iter().map(|s| s.to_string()).collect(),
            ..Settings::default()
        }
    }

    // ── Dừng: bấm 2 lần chỉ 1 lần; dừng không đụng file đang tải; phiếu ghi huy ──
    #[test]
    fn b1_dung_bam_2_lan_chi_1_lan_va_phieu_ghi_huy() {
        let (d, dc) = dc_co_dot("b1", &[("a", r"D:\v\k1"), ("b", r"D:\v\k1"), ("c", r"D:\v\k2")]);
        assert!(dc.chiem_luot(), "đang có lượt quét");
        let (m1, ngay) = dc.yeu_cau_dung(true, t("2026-11-04T06:30:00+07:00")).unwrap();
        assert!(!ngay && m1.contains("nhóm đang quét xong"), "{m1}");
        let (m2, ngay2) = dc.yeu_cau_dung(true, t("2026-11-04T06:30:05+07:00")).unwrap();
        assert!(!ngay2 && m2.contains("từ trước"), "bấm lần 2 không làm gì thêm: {m2}");
        let c = dc.co_luot();
        assert!(c.dung && c.dong_huy && c.huy_job);
        let st = dc.snapshot();
        assert!(st.xong_ngay, "dừng = hôm nay không tự chạy lại");
        assert!(!den_gio_chay(t("2026-11-04T06:31:00+07:00"), "06:00", &st));
        // Lượt kết thúc: a đang tải (KHÔNG đụng), b chờ (huỷ được), c đã xong.
        let huy_ds = Mutex::new(Vec::<String>::new());
        let qs = |j: &str| {
            if huy_ds.lock().unwrap().iter().any(|x| x == j) {
                return QuanSat::Loi { msg: "đã huỷ".into(), cuoi: true };
            }
            match j {
                "ja" => QuanSat::DangTai,
                "jb" => QuanSat::Cho,
                _ => QuanSat::Xong { path: Some(r"D:\v\k2\c.mp4".into()), luc: None },
            }
        };
        let huy = |j: &str| {
            let ok = matches!(qs(j), QuanSat::Cho);
            if ok {
                huy_ds.lock().unwrap().push(j.to_string());
            }
            ok
        };
        let (n, so_job) = dc.dong_huy_voi(Some(&huy), &qs, &khong_size, t("2026-11-04T06:40:00+07:00"));
        assert_eq!((n, so_job), (1, 1), "chỉ huỷ job CHƯA BẮT ĐẦU");
        let p = dc.snapshot().dot[0].phieu.clone();
        assert_eq!((p.trang_thai.as_str(), p.ly_do_dong.as_deref()), ("da_dong", Some("huy")));
        let tt: Vec<&str> = p.kenh.iter().flat_map(|k| &k.video).map(|v| v.trang_thai.as_str()).collect();
        assert_eq!(tt, vec!["tre", "loi", "xong"], "đang tải → tre (vẫn tải tiếp), chờ → huỷ, xong giữ xong");
        // Bấm Dừng lần nữa sau khi đã đóng hết và hết lượt → báo rõ, không làm gì.
        drop(GiuQuet(dc.clone()));
        assert!(dc.yeu_cau_dung(false, t("2026-11-04T06:41:00+07:00")).is_err());
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn b2_dung_khi_khong_quet_thi_dong_ngay_khong_huy_job() {
        let (d, dc) = dc_co_dot("b2", &[("a", r"D:\v\k1")]);
        let (_, dong_ngay) = dc.yeu_cau_dung(false, t("2026-11-04T07:00:00+07:00")).unwrap();
        assert!(dong_ngay, "không có lượt quét → người gọi đóng ngay");
        let (n, so_job) = dc.dong_huy_voi(None, &|_| QuanSat::Cho, &khong_size, t("2026-11-04T07:00:00+07:00"));
        assert_eq!((n, so_job), (1, 0), "không chọn huỷ job thì không huỷ gì");
        assert_eq!(dc.snapshot().dot[0].phieu.kenh[0].video[0].trang_thai, "tre");
        assert!(dc.snapshot().dot[0].can_ghi, "phiếu huy được ghi ra đĩa");
        std::fs::remove_dir_all(d).ok();
    }

    // ── Chạy ngay: bấm nhiều lần chỉ 1 lượt; danh sách rỗng / đã xong báo rõ ──
    #[test]
    fn b3_chay_ngay_bam_2_lan_chi_chay_1_luot() {
        let (d, dc) = dc_tam("b3");
        let now = t("2026-11-04T05:00:00+07:00");
        let s = st_lich(&["A", "B"]);
        let co = vec!["A".to_string(), "B".to_string()];
        let m = dc.chuan_bi_chay_tay(&s, &co, now).unwrap();
        assert!(m.contains("A → B"), "{m}");
        assert!(dc.dang_quet());
        let e = dc.chuan_bi_chay_tay(&s, &co, now).unwrap_err();
        assert!(e.contains("đang chạy rồi"), "{e}");
        assert!(!dc.chiem_luot(), "bộ lịch cũng không chiếm được lượt thứ 2");
        drop(GiuQuet(dc.clone()));
        assert!(!dc.dang_quet());
        // Danh sách rỗng.
        assert!(dc.chuan_bi_chay_tay(&st_lich(&[]), &co, now).unwrap_err().contains("Chưa chọn nhóm"));
        // Đã quét hết hôm nay.
        dc.lock().nhom_da_quet = co.clone();
        assert!(dc.chuan_bi_chay_tay(&s, &co, now).unwrap_err().contains("đã quét xong"));
        assert!(!dc.dang_quet(), "báo lỗi thì không giữ cờ");
        // Nhóm còn lại đã bị xoá → cũng không có gì để chạy.
        dc.lock().nhom_da_quet = vec!["A".into()];
        assert!(dc.chuan_bi_chay_tay(&s, &["A".to_string()], now).is_err());
        std::fs::remove_dir_all(d).ok();
    }

    // ── Bỏ qua hôm nay: không chạy nữa hôm nay, mai chạy đúng giờ; Chạy ngay gỡ bỏ qua ──
    #[test]
    fn b4_bo_qua_hom_nay_va_chay_ngay_go_bo_qua() {
        let (d, dc) = dc_tam("b4");
        let now = t("2026-11-04T05:30:00+07:00");
        let m = dc.bo_qua_hom_nay(now);
        assert!(m.contains("mai chạy lại"), "{m}");
        assert!(dc.bo_qua_hom_nay(now).contains("từ trước"), "bấm 2 lần không đổi gì");
        let st = dc.snapshot();
        assert!(!den_gio_chay(t("2026-11-04T06:00:00+07:00"), "06:00", &st));
        assert_eq!(lan_chay_toi(now, "06:00", &st, true).map(iso).as_deref(), Some("2026-11-05T06:00:00+07:00"));
        assert!(den_gio_chay(t("2026-11-05T06:00:00+07:00"), "06:00", &st), "mai chạy lại");
        // Đang chạy mà bấm Bỏ qua → dừng sau nhóm đang quét, KHÔNG đóng phiếu huy.
        assert!(dc.chiem_luot());
        dc.lock().bo_qua_ngay = None;
        dc.bo_qua_hom_nay(now);
        assert!(dc.co_luot().dung && !dc.co_luot().dong_huy);
        drop(GiuQuet(dc.clone()));
        // Chạy ngay hôm nay vẫn được (gỡ Bỏ qua).
        dc.chuan_bi_chay_tay(&st_lich(&["A"]), &["A".to_string()], now).unwrap();
        assert_eq!(dc.snapshot().bo_qua_ngay, None);
        drop(GiuQuet(dc.clone()));
        std::fs::remove_dir_all(d).ok();
    }

    // ── 1b-D4: panic trong lượt chạy → tự nhả cờ, báo bot, hoãn 10 phút ──
    #[tokio::test]
    async fn b5_d4_panic_trong_luot_chay_tu_nha_co_va_bao_bot() {
        let (d, dc) = dc_tam("b5");
        assert!(dc.chiem_luot());
        chay_co_bao_ve(dc.clone(), async {
            let id = String::from("abc");
            let _ = &id[..8]; // đúng loại panic nghi ở `dot_id[..8]`
        })
        .await;
        assert!(!dc.dang_quet(), "cờ đang quét được nhả");
        assert!(dc.bat_dau_quet.lock().unwrap().is_none());
        let st = dc.snapshot();
        assert!(st.canh_bao.iter().any(|c| c.loai == "ket"), "báo bot loại «ket»");
        assert!(st.ly_do_hoan.unwrap().contains("lỗi bất ngờ"));
        assert!(dc.hoan_den.lock().unwrap().is_some(), "hoãn 10 phút, không lặp panic mỗi 15 giây");
        assert!(dc.chiem_luot(), "lượt sau chiếm được — không kẹt");
        drop(GiuQuet(dc.clone()));
        // Sổ có dot_id hỏng (ngắn) không còn làm panic `vao_ngay`.
        let mut st = TrangThaiLich::default();
        let mut hong = mo_dot("abc".into(), "A", t(MO), 3);
        dong(&mut hong, "xong", t(MO));
        st.dot.push(hong);
        vao_ngay(&mut st, t("2026-11-10T06:00:00+07:00"));
        std::fs::remove_dir_all(d).ok();
    }

    // ── 1b-D5 + D6: kiểm trước MỖI nhóm — ổ đầy giữa chừng, sang ngày, tắt Lịch ──
    #[test]
    fn b6_truoc_moi_nhom_o_day_sang_ngay_tat_lich() {
        let ngay = "2026-11-04";
        let now = t("2026-11-04T06:30:00+07:00");
        assert_eq!(truoc_nhom(false, true, ngay, now, None), None);
        assert_eq!(truoc_nhom(false, true, ngay, now, Some("ổ D".into())), Some(LyDoDung::ODay("ổ D".into())));
        assert_eq!(truoc_nhom(false, false, ngay, now, None), Some(LyDoDung::TatLich));
        assert_eq!(truoc_nhom(true, false, ngay, now, Some("x".into())), Some(LyDoDung::NguoiDung), "Dừng ưu tiên nhất");
        // Qua nửa đêm giữa lượt (bắt đầu 23:50, nhóm sau lúc 00:05).
        assert_eq!(truoc_nhom(false, true, ngay, t("2026-11-05T00:05:00+07:00"), None), Some(LyDoDung::SangNgay));
        // Ổ đầy giữa chừng: hàm kiểm ổ chỉ nhìn ổ của NHÓM ĐÓ (+ ổ C).
        let e_day = |p: &Path| Some(if p.to_string_lossy().starts_with('E') { GB } else { 500 * GB });
        assert!(kiem_tra_o_dia(&[PathBuf::from(r"D:\v\k1")], &e_day).is_none(), "nhóm 1 ở ổ D: chạy");
        assert!(kiem_tra_o_dia(&[PathBuf::from(r"E:\v\k9")], &e_day).is_some(), "nhóm 2 ở ổ E đầy: dừng");
    }

    #[test]
    fn b7_d6_ngu_dong_qua_ngay_khong_quet_lai_ca_luot_chong_len() {
        // Lượt ngày 04 quét xong A rồi máy ngủ tới 08:00 ngày 05.
        let mut st = TrangThaiLich::default();
        vao_ngay(&mut st, t("2026-11-04T06:00:00+07:00"));
        st.nhom_da_quet.push("A".into());
        let thuc = t("2026-11-05T08:00:00+07:00");
        // Vòng lịch chạy `vao_ngay` mỗi nhịp → sổ sang ngày 05.
        assert!(vao_ngay(&mut st, thuc));
        assert!(st.nhom_da_quet.is_empty() && !st.xong_ngay);
        // Lượt cũ: nhóm B KHÔNG được quét tiếp vào ngày 05 (dừng), và nhóm đang
        // dở không được ghi vào "đã quét" của ngày 05 (sổ không còn là ngày 04).
        assert_eq!(truoc_nhom(false, true, "2026-11-04", thuc, None), Some(LyDoDung::SangNgay));
        assert_ne!(st.ngay.as_deref(), Some("2026-11-04"));
        // Lượt ngày 05 chạy MỘT lần từ đầu (A, B), xong thì thôi.
        assert!(den_gio_chay(thuc, "06:00", &st));
        assert_eq!(nhom_can_quet(&["A".into(), "B".into()], &st.nhom_da_quet), vec!["A".to_string(), "B".into()]);
        st.nhom_da_quet = vec!["A".into(), "B".into()];
        st.xong_ngay = true;
        assert!(!den_gio_chay(t("2026-11-05T09:00:00+07:00"), "06:00", &st));
        // Đợt ngày 04 còn mở: tự đóng het_han khi thức dậy (đã quá hạn chót).
        let mut dot = mo_dot("20261104-01".into(), "A", t("2026-11-04T06:00:00+07:00"), 3);
        them_job(&mut dot, &job("a", r"D:\v\k1"));
        cap_nhat_dot(&mut dot, &|_| QuanSat::DangTai, &khong_size, thuc);
        assert_eq!(dot.phieu.ly_do_dong.as_deref(), Some("het_han"));
    }

    #[test]
    fn b8_doi_gio_may_lui_khong_quet_lai() {
        let mut st = TrangThaiLich::default();
        vao_ngay(&mut st, t("2026-11-05T06:00:00+07:00"));
        st.nhom_da_quet = vec!["A".into()];
        st.xong_ngay = true;
        // Đồng hồ bị chỉnh lùi 1 ngày: KHÔNG coi là ngày mới (không quét lại cả lượt).
        let lui = t("2026-11-04T07:00:00+07:00");
        assert!(!den_gio_chay(lui, "06:00", &st), "chưa qua vao_ngay cũng không chạy");
        vao_ngay(&mut st, lui);
        assert_eq!(st.ngay.as_deref(), Some("2026-11-04"));
        assert_eq!(st.nhom_da_quet, vec!["A".to_string()], "giữ tiến độ");
        assert!(!den_gio_chay(lui, "06:00", &st));
        // Lùi trong cùng ngày (07:00 → 05:00 rồi tới 06:00): đã xong thì không chạy lại.
        assert!(!den_gio_chay(t("2026-11-04T06:00:30+07:00"), "06:00", &st));
        // Ngày thật kế tiếp vẫn chạy bình thường.
        let mai = t("2026-11-05T06:00:00+07:00");
        vao_ngay(&mut st, mai);
        assert!(den_gio_chay(mai, "06:00", &st));
    }

    // ── Đổi giờ giữa chừng: có hiệu lực ở nhịp kế, không chạy lại khi đã xong ──
    #[test]
    fn b9_doi_gio_giua_chung() {
        let mut st = TrangThaiLich::default();
        let now = t("2026-11-04T07:00:00+07:00");
        vao_ngay(&mut st, now);
        // Chưa chạy hôm nay, dời 06:00 → 08:00: 07:00 chưa chạy, 08:00 chạy.
        assert!(!den_gio_chay(now, "08:00", &st));
        assert_eq!(lan_chay_toi(now, "08:00", &st, true).map(iso).as_deref(), Some("2026-11-04T08:00:00+07:00"));
        assert!(den_gio_chay(t("2026-11-04T08:00:00+07:00"), "08:00", &st));
        // Đã đến giờ mà chưa chạy → "lần tới" = ngay bây giờ.
        assert_eq!(lan_chay_toi(now, "06:00", &st, true), Some(now));
        // Đã chạy xong 06:00 rồi mới dời sang 08:00 → hôm nay KHÔNG chạy lại.
        st.xong_ngay = true;
        assert!(!den_gio_chay(t("2026-11-04T08:01:00+07:00"), "08:00", &st));
        assert_eq!(lan_chay_toi(now, "08:00", &st, true).map(iso).as_deref(), Some("2026-11-05T08:00:00+07:00"));
        assert_eq!(lan_chay_toi(now, "08:00", &st, false), None, "Lịch tắt");
        assert_eq!(lan_chay_toi(now, "8h", &st, true), None);
    }

    // ── Lỗi theo loại cho bot + "đã ổn lại" (loại biến mất) ──
    #[test]
    fn b10_van_de_theo_loai_cho_bot() {
        let mut d = DauVaoVanDe { lich_bat: true, nhom_rong: true, ..Default::default() };
        assert!(tinh_van_de(&d)["khong_chay"].contains("trống"));
        d.nhom_rong = false;
        d.ly_do_chan = Some("Không mở đợt tải: ổ D: chỉ còn 2 GB".into());
        assert!(tinh_van_de(&d)["khong_chay"].contains("ổ D"));
        d.nhom_mat = vec!["MỸ".into()];
        assert!(tinh_van_de(&d)["nhom_mat"].contains("«MỸ»"));
        d.ghi_loi = Some((Duration::from_secs(60), "x".into()));
        assert!(!tinh_van_de(&d).contains_key("ghi_loi"), "lỗi ghi thoáng qua không báo");
        d.ghi_loi = Some((GHI_LOI_KEO_DAI, "access denied".into()));
        assert!(tinh_van_de(&d)["ghi_loi"].contains("access denied"));
        d.quet_lau = Some(QUET_KET);
        assert!(tinh_van_de(&d).contains_key("ket"));
        d.loi_tat = Some("powershell lỗi".into());
        assert!(tinh_van_de(&d)["loi_tat"].contains("powershell"));
        // Lịch tắt: không báo lỗi của Lịch (lối tắt vẫn báo).
        d.lich_bat = false;
        let m = tinh_van_de(&d);
        assert!(!m.contains_key("khong_chay") && !m.contains_key("nhom_mat") && m.contains_key("loi_tat"));
        // Hết lỗi → map rỗng → bot báo "đã ổn lại" cho loại đã báo.
        assert!(tinh_van_de(&DauVaoVanDe { lich_bat: true, ..Default::default() }).is_empty());
        assert_eq!(nhom_mat(&["".into(), "A".into(), "B".into()], &["A".into()]), vec!["B".to_string()]);
    }

    #[test]
    fn b11_ghi_loi_keo_dai_giu_moc_dau_tien_het_loi_thi_xoa() {
        let (d, dc) = dc_co_dot("b11", &[("a", r"D:\v\k1")]);
        let hong = d.join("la-file");
        std::fs::write(&hong, b"x").unwrap();
        dc.ghi_xuong_dia(&hong);
        let t1 = dc.ghi_loi.lock().unwrap().as_ref().map(|(t, _)| *t).expect("có lỗi");
        dc.ghi_xuong_dia(&hong);
        assert_eq!(dc.ghi_loi.lock().unwrap().as_ref().map(|(t, _)| *t), Some(t1), "giữ mốc lỗi ĐẦU TIÊN");
        std::fs::remove_file(&hong).unwrap();
        dc.ghi_xuong_dia(&hong);
        assert!(dc.ghi_loi.lock().unwrap().is_none(), "ghi lại được → hết lỗi");
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn b12_dong_het_han_con_tre_thi_bao_bot_kem_so() {
        let (d, dc) = dc_co_dot("b12", &[("a", r"D:\v\k1"), ("b", r"D:\v\k1")]);
        dc.cap_nhat_voi(&|_| QuanSat::DangTai, &khong_size, t("2026-11-04T09:00:12+07:00"));
        let c = dc.snapshot().canh_bao;
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].loai, "tre");
        assert!(c[0].text.contains("2 video trễ"), "{}", c[0].text);
        dc.cap_nhat_voi(&|_| QuanSat::DangTai, &khong_size, t("2026-11-04T09:30:00+07:00"));
        assert_eq!(dc.snapshot().canh_bao.len(), 1, "chỉ báo 1 lần");
        std::fs::remove_dir_all(d).ok();
    }

    // ── 1b-D7: job xếp hàng trong 2 giây cuối không rơi khỏi phiếu khi thoát ──
    #[tokio::test]
    async fn b13_d7_job_cuoi_vao_phieu_khi_thoat_em() {
        let (d, dc) = dc_co_dot("b13", &[]);
        let hop: JobSink = Arc::default();
        *dc.sink.lock().unwrap() = Some(("20261104-01".into(), hop.clone()));
        hop.day(job("z", r"D:\v\k1"));
        // Đánh thức vòng ghi NGAY (không đợi 2 giây).
        tokio::time::timeout(Duration::from_millis(100), hop.bao.notified()).await.expect("được đánh thức ngay");
        hop.day(job("y", r"D:\v\k1"));
        hop.bo_sot("kênh 7".into());
        // Thoát êm = rút hộp + ghi đĩa (như `thoat_em`, đồng hồ thật).
        dc.cap_nhat_voi(&|_| QuanSat::Cho, &khong_size, t("2026-11-04T06:05:00+07:00"));
        let dir = d.join("dot");
        assert_eq!(dc.ghi_xuong_dia(&dir), 0);
        let p: PhieuTai = serde_json::from_slice(&std::fs::read(dir.join("20261104-01.json")).unwrap()).unwrap();
        assert_eq!(p.kenh[0].video.len(), 2, "cả 2 job cuối đều vào phiếu");
        let c = dc.snapshot().canh_bao;
        assert!(c.iter().any(|c| c.loai == "bo_sot" && c.text.contains("kênh 7")), "kênh bị sót được báo");
        // Sổ cũng có: mở lại app vẫn biết 2 job này.
        assert_eq!(DayChuyen::load(&d).snapshot().dot[0].jobs.len(), 2);
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn b14_trang_thai_json_du_truong_cho_tin_tu_sua() {
        let (d, dc) = dc_co_dot("b14", &[("a", r"D:\v\k1")]);
        let now = t("2026-11-04T06:10:00+07:00");
        let mut s = st_lich(&["A", "MẤT"]);
        s.lich_tai_gio = "06:00".into();
        let v = dc.trang_thai_json(&s, &["A".to_string()], now);
        for k in ["dangChay", "lanToi", "lyDoHoan", "dotMo", "tre", "vanDe", "canhBao", "nhomCo", "hanChotGio", "dangDung", "nhipCuoi"] {
            assert!(v.get(k).is_some(), "thiếu {k}");
        }
        assert_eq!(v["dotMo"], 1);
        assert!(v["vanDe"].as_array().unwrap().iter().any(|x| x["loai"] == "nhom_mat"));
        // Sổ cũ (trước Đợt 1b, không có trường mới) vẫn đọc được — không bị coi là hỏng.
        let cu = r#"{"ngay":"2026-11-04","nhom_da_quet":[],"xong_ngay":false,"dot":[],"canh_bao":[{"id":"x","luc":"2026-11-04T06:00:00+07:00","text":"t"}],"ly_do_chan":null}"#;
        let st: TrangThaiLich = serde_json::from_str(cu).unwrap();
        assert_eq!(st.canh_bao[0].loai, "");
        std::fs::remove_dir_all(d).ok();
    }

    // ───────────── Vòng duyệt cdb2d8e: chạy `chay_luot` ĐẦU-CUỐI bằng môi trường giả ─────────────

    type KhiQuet = Box<dyn Fn(&EnvGia, &str, &JobSink) + Send + Sync>;

    struct EnvGia {
        s: Mutex<Settings>,
        now: DateTime<FixedOffset>,
        dc: Arc<DayChuyen>,
        tt: Mutex<std::collections::HashMap<String, QuanSat>>,
        da_quet: Mutex<Vec<String>>,
        nhan: Mutex<Option<String>>,
        lich_chay_luc_quet: Mutex<Vec<bool>>,
        dung_sau_chot: Mutex<Option<Result<(String, bool), String>>>,
        thu_dung_sau_chot: bool,
        khi_quet: KhiQuet,
    }

    impl EnvGia {
        fn qs(&self) -> impl Fn(&str) -> QuanSat + '_ {
            |j: &str| self.tt.lock().unwrap().get(j).cloned().unwrap_or(QuanSat::Cho)
        }
    }

    impl MoiTruong for EnvGia {
        fn cai_dat(&self) -> Settings {
            self.s.lock().unwrap().clone()
        }
        fn bay_gio(&self) -> DateTime<FixedOffset> {
            self.now
        }
        fn nhom_co(&self, s: &Settings) -> Vec<String> {
            s.lich_tai_nhom.clone()
        }
        fn thu_muc_nhom(&self, _s: &Settings, nhom: &str) -> Vec<String> {
            vec![format!(r"D:\v\{nhom}")]
        }
        fn o_trong(&self, _p: &Path) -> Option<u64> {
            Some(500 * GB)
        }
        fn dat_nhan(&self, nhan: Option<String>) {
            *self.nhan.lock().unwrap() = nhan;
        }
        fn cho_quet_khac(&self) -> Fut<'_> {
            Box::pin(async {})
        }
        fn quet_nhom(&self, nhom: String, hop: JobSink) -> Fut<'_> {
            Box::pin(async move {
                self.lich_chay_luc_quet.lock().unwrap().push(lich_dang_chay());
                (self.khi_quet)(self, &nhom, &hop);
                self.da_quet.lock().unwrap().push(nhom);
            })
        }
        fn nhip(&self, dir: PathBuf) -> Fut<'_> {
            Box::pin(async move {
                self.dc.cap_nhat_voi(&self.qs(), &khong_size, self.now);
                self.dc.ghi_xuong_dia(&dir);
                // Bấm Dừng ĐÚNG SAU bước kiểm tra cuối (nhịp ghi cuối lượt).
                if self.thu_dung_sau_chot && self.dc.co_luot().chot && self.dung_sau_chot.lock().unwrap().is_none() {
                    *self.dung_sau_chot.lock().unwrap() = Some(self.dc.yeu_cau_dung(false, self.now));
                }
            })
        }
        fn huy_va_dong(&self, huy_job: bool) -> Fut<'_> {
            Box::pin(async move {
                let huy = |j: &str| {
                    let mut tt = self.tt.lock().unwrap();
                    let cho = matches!(tt.get(j), None | Some(QuanSat::Cho));
                    if cho {
                        tt.insert(j.into(), QuanSat::Loi { msg: "đã huỷ".into(), cuoi: true });
                    }
                    cho
                };
                self.dc.dong_huy_voi(huy_job.then_some(&huy as &dyn Fn(&str) -> bool), &self.qs(), &khong_size, self.now);
            })
        }
        fn cho_chut(&self) -> Fut<'_> {
            Box::pin(std::future::pending())
        }
    }

    fn env_gia(ten: &str, s: Settings, thu_dung_sau_chot: bool, khi_quet: KhiQuet) -> (PathBuf, Arc<EnvGia>) {
        let (d, dc) = dc_tam(ten);
        let mut s = s;
        s.daychuyen_dir = Some(d.to_string_lossy().into_owned());
        let env = Arc::new(EnvGia {
            s: Mutex::new(s),
            now: t(MO),
            dc,
            tt: Mutex::default(),
            da_quet: Mutex::default(),
            nhan: Mutex::default(),
            lich_chay_luc_quet: Mutex::default(),
            dung_sau_chot: Mutex::default(),
            thu_dung_sau_chot,
            khi_quet,
        });
        (d, env)
    }

    async fn chay_gia(env: &Arc<EnvGia>, tay: bool) {
        assert!(env.dc.chiem_luot());
        let e = env.clone();
        chay_co_bao_ve(env.dc.clone(), async move {
            let dc = e.dc.clone();
            chay_luot(&*e, &dc, tay).await;
        })
        .await;
    }

    // Dừng (huỷ job chưa tải) giữa vòng, Chạy ngay khi công tắc Lịch TẮT mà
    // nhịp quét cũ đang BẬT: nhịp cũ phải nhường suốt lượt (S2).
    #[tokio::test]
    async fn e1_dau_cuoi_dung_giua_vong_va_nhip_cu_nhuong() {
        let s = Settings {
            lich_tai_enabled: false,
            watch_auto_enabled: true,
            lich_tai_nhom: vec!["A".into(), "B".into(), "C".into()],
            ..Settings::default()
        };
        let (d, env) = env_gia("e1", s.clone(), false, Box::new(|env: &EnvGia, nhom: &str, hop: &JobSink| {
            if nhom == "A" {
                hop.day(job("a", r"D:\v\A"));
                hop.day(job("b", r"D:\v\A"));
                env.tt.lock().unwrap().insert("ja".into(), QuanSat::DangTai);
                // Nhịp cũ thử chen vào đúng giữa nhóm → phải nhường.
                assert!(!nhip_cu_chay_ngay(&env.cai_dat(), lich_dang_chay()));
                let (m, ngay) = env.dc.yeu_cau_dung(true, env.now).unwrap();
                assert!(!ngay, "{m}");
            }
        }));
        chay_gia(&env, true).await;
        assert_eq!(*env.da_quet.lock().unwrap(), vec!["A".to_string()], "B, C không được quét sau Dừng");
        assert_eq!(*env.lich_chay_luc_quet.lock().unwrap(), vec![true]);
        assert!(nhip_cu_duoc_chay(&s), "giá trị nhịp cũ của anh giữ nguyên");
        let st = env.dc.snapshot();
        let p = &st.dot[0].phieu;
        assert_eq!((p.trang_thai.as_str(), p.ly_do_dong.as_deref()), ("da_dong", Some("huy")));
        let tt: Vec<&str> = p.kenh.iter().flat_map(|k| &k.video).map(|v| v.trang_thai.as_str()).collect();
        assert_eq!(tt, vec!["tre", "loi"], "đang tải giữ nguyên (tre), chưa tải bị huỷ");
        assert!(st.xong_ngay, "Dừng = hôm nay không tự chạy lại");
        assert!(!env.dc.dang_quet() && env.dc.co_luot() == CoLuot::default(), "nhả cờ, xoá cờ");
        assert_eq!(*env.nhan.lock().unwrap(), None, "gỡ nhãn đang chạy");
        // Ra đĩa đúng: tool cắt đọc thấy `huy`.
        let dia: PhieuTai = serde_json::from_slice(&std::fs::read(d.join("dot").join("20261104-01.json")).unwrap()).unwrap();
        assert_eq!(dia.ly_do_dong.as_deref(), Some("huy"));
        std::fs::remove_dir_all(d).ok();
    }

    // Lượt chạy hết 2 nhóm bình thường: phiếu từng nhóm, xong ngày, nhóm sau
    // đọc cài đặt mới (đổi hạn chót giữa chừng có hiệu lực từ nhóm kế).
    #[tokio::test]
    async fn e2_dau_cuoi_chay_het_nhom_doi_han_chot_giua_chung() {
        let s = Settings { lich_tai_enabled: true, lich_tai_nhom: vec!["A".into(), "B".into()], ..Settings::default() };
        let (d, env) = env_gia("e2", s, false, Box::new(|env: &EnvGia, nhom: &str, hop: &JobSink| {
            hop.day(job(&format!("{nhom}1"), &format!(r"D:\v\{nhom}")));
            if nhom == "A" {
                env.s.lock().unwrap().lich_tai_han_chot_gio = 5;
            }
        }));
        chay_gia(&env, false).await;
        let st = env.dc.snapshot();
        assert_eq!(st.nhom_da_quet, vec!["A".to_string(), "B".into()]);
        assert!(st.xong_ngay);
        assert_eq!(st.dot.len(), 2);
        assert_eq!(st.dot[0].phieu.han_chot, "2026-11-04T09:00:12+07:00");
        assert_eq!(st.dot[1].phieu.han_chot, "2026-11-04T11:00:12+07:00", "nhóm kế dùng hạn chót mới");
        assert!(st.dot.iter().all(|x| x.quet_xong && x.jobs.len() == 1));
        std::fs::remove_dir_all(d).ok();
    }

    // Dừng bấm SAU bước kiểm tra cuối: không bị mất — tự đóng phiếu ngay (S3).
    #[tokio::test]
    async fn e3_dung_sau_buoc_chot_khong_bi_mat() {
        let s = Settings { lich_tai_enabled: true, lich_tai_nhom: vec!["A".into()], ..Settings::default() };
        let (d, env) = env_gia("e3", s, true, Box::new(|env: &EnvGia, _n: &str, hop: &JobSink| {
            hop.day(job("a", r"D:\v\A"));
            env.tt.lock().unwrap().insert("ja".into(), QuanSat::DangTai);
        }));
        chay_gia(&env, false).await;
        let kq = env.dung_sau_chot.lock().unwrap().clone().expect("đã thử Dừng sau chốt");
        let (m, dong_ngay) = kq.unwrap();
        assert!(dong_ngay, "lượt đã chốt → người gọi đóng phiếu NGAY: {m}");
        assert!(m.contains("KHÔNG bị ảnh hưởng"), "{m}");
        let (n, _) = env.dc.dong_huy_voi(None, &env.qs(), &khong_size, env.now);
        assert_eq!(n, 1);
        assert_eq!(env.dc.snapshot().dot[0].phieu.ly_do_dong.as_deref(), Some("huy"));
        std::fs::remove_dir_all(d).ok();
    }

    // Dừng trong khe giữa `chiem_luot` và lúc lượt bắt đầu chạy: không bị xoá (S3).
    #[test]
    fn e4_dung_ngay_sau_khi_chiem_luot_khong_bi_xoa() {
        let (d, dc) = dc_tam("e4");
        assert!(dc.chiem_luot());
        dc.yeu_cau_dung(true, t(MO)).unwrap();
        let g = GiuQuet::new(dc.clone()); // lượt bắt đầu chạy
        assert!(dc.co_luot().dung && dc.co_luot().huy_job, "cờ Dừng còn nguyên");
        drop(g);
        assert!(dc.chiem_luot());
        assert_eq!(dc.co_luot(), CoLuot::default(), "lượt MỚI không mang cờ Dừng cũ");
        drop(GiuQuet(dc.clone()));
        std::fs::remove_dir_all(d).ok();
    }

    // Dừng 2 lần khác lựa chọn → báo rõ lựa chọn đang dùng.
    #[test]
    fn e5_dung_2_lan_khac_lua_chon_bao_ro() {
        let (d, dc) = dc_tam("e5");
        assert!(dc.chiem_luot());
        dc.yeu_cau_dung(false, t(MO)).unwrap();
        let (m, _) = dc.yeu_cau_dung(true, t(MO)).unwrap();
        assert!(m.contains("lựa chọn đang dùng: giữ job đã xếp"), "{m}");
        assert!(!dc.co_luot().huy_job);
        drop(GiuQuet(dc.clone()));
        std::fs::remove_dir_all(d).ok();
    }

    // 01:00 bấm Dừng để đóng đợt đêm qua: KHÔNG được làm mất lượt 06:00 (S7).
    #[test]
    fn e6_dung_khi_khong_chay_khong_bo_luot_hom_nay() {
        let (d, dc) = dc_co_dot("e6", &[("a", r"D:\v\k1")]);
        let mot_gio = t("2026-11-05T01:00:00+07:00");
        let (m, dong_ngay) = dc.yeu_cau_dung(false, mot_gio).unwrap();
        assert!(dong_ngay && m.contains("KHÔNG bị ảnh hưởng"), "{m}");
        let st = dc.snapshot();
        assert!(!st.xong_ngay);
        assert!(den_gio_chay(t("2026-11-05T06:00:00+07:00"), "06:00", &st), "06:00 vẫn chạy");
        std::fs::remove_dir_all(d).ok();
    }

    // Thoát 2 lần (quit_gracefully rồi WindowEvent::Destroyed SAU queue.shutdown):
    // lần 2 không được ghi job bị ngắt thành `loi` (S5).
    #[test]
    fn e7_thoat_2_lan_lan_sau_khong_ghi_de() {
        let (d, dc) = dc_co_dot("e7", &[("a", r"D:\v\k1")]);
        let dir = d.join("dot");
        assert!(dc.thoat_em_voi(&|_| QuanSat::DangTai, &khong_size, t(MO), &dir));
        let huy = |_: &str| QuanSat::Loi { msg: "đã huỷ".into(), cuoi: true };
        assert!(!dc.thoat_em_voi(&huy, &khong_size, t(MO), &dir), "lần 2 bỏ qua");
        let p: PhieuTai = serde_json::from_slice(&std::fs::read(dir.join("20261104-01.json")).unwrap()).unwrap();
        assert_eq!(p.kenh[0].video[0].trang_thai, "dang_tai");
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn e9_dau_cap_nhat_co_han_30_phut() {
        let (d, _dc) = dc_tam("e9");
        ghi_dau_cap_nhat(&d, t(MO)).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(d.join(TEN_DAU_CAP_NHAT)).unwrap()).unwrap();
        assert_eq!(v["het_han"], "2026-11-04T06:30:12+07:00");
        xoa_dau_cap_nhat(&d);
        assert!(!d.join(TEN_DAU_CAP_NHAT).exists());
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn e8_dau_thoat_em_cho_bot() {
        let (d, _dc) = dc_tam("e8");
        ghi_dau_thoat(&d, t(MO));
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(d.join(TEN_DAU_THOAT)).unwrap()).unwrap();
        assert_eq!(v["luc"], MO);
        xoa_dau_thoat(&d);
        assert!(!d.join(TEN_DAU_THOAT).exists());
        std::fs::remove_dir_all(d).ok();
    }

    #[cfg(windows)]
    #[test]
    fn dung_luong_o_that_doc_duoc() {
        assert!(dung_luong_trong(Path::new("C:\\")).unwrap_or(0) > 0);
        assert_eq!(goc_o(Path::new(r"D:\a\b")), Some(PathBuf::from("D:\\")));
    }
}
