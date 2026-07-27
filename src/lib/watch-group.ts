/**
 * Nhóm & chống trùng cho trang "Theo dõi kênh" — hàm THUẦN để test được.
 *
 * BA LỖI THẬT anh Hùng báo 27/07/2026, sửa ở đây:
 *
 * 1. Đang xem nhóm "Mỹ", bấm ➕ Thêm kênh thì kênh lại vào nhóm "Mỹ mới".
 *    Vì `addGrp` chỉ đổi khi user TỰ GÕ, và sau khi thêm xong KHÔNG được
 *    reset (WatchPage reset addName/addUrl/addDir mà thiếu addGrp) → ô nhóm
 *    còn nguyên chữ gõ lần trước. `nhomMacDinh` lo việc này.
 *
 * 2. Thêm TRÙNG link mà không báo gì, lại "tự thay đổi": Rust
 *    add_watched_channel thấy trùng URL thì trả về kênh CŨ (idempotent), rồi
 *    UI chạy tiếp setWatchedTarget → setWatchedGroup → setWatchedSourceMode →
 *    setWatchedDestDir, tức GHI ĐÈ tên + nhóm + chế độ + thư mục lưu của kênh
 *    đang có. `timTrungUrl` chặn TRƯỚC khi gọi, không ghi đè gì.
 *
 * 3. Không đổi được nhóm sau khi thêm: ô <select> nhóm nằm trong khối
 *    `{kOpen && …}` nên chỉ hiện khi bung thẻ kênh — thẻ mặc định thu gọn.
 *    Sửa ở WatchPage (đưa ô nhóm ra dòng thu gọn), không cần hàm ở đây.
 */

/** Chuẩn hoá URL kênh để so trùng — GIỐNG HỆT Rust `add_watched_channel`:
 *  `url.trim_end_matches('/').to_lowercase()`. Lệch cách chuẩn hoá là UI báo
 *  "chưa trùng" nhưng Rust vẫn coi là trùng → lại rơi vào lỗi ghi đè. */
export function chuanUrl(u: string): string {
  return (u ?? "").trim().replace(/\/+$/, "").toLowerCase();
}

export type KenhToiThieu = {
  name?: string;
  group?: string;
  keys: { url: string }[];
};

/** Kênh nào đang chứa URL này? (so trên MỌI key nguồn của mọi kênh) */
export function timTrungUrl<T extends KenhToiThieu>(
  kenhAll: T[],
  url: string,
): T | null {
  const u = chuanUrl(url);
  if (!u) return null;
  for (const k of kenhAll) {
    for (const key of k.keys ?? []) {
      if (chuanUrl(key.url) === u) return k;
    }
  }
  return null;
}

/** Câu báo trùng — nói RÕ link đã nằm ở kênh nào, nhóm nào, và làm gì tiếp. */
export function loiTrung(k: KenhToiThieu): string {
  const ten = (k.name || "").trim() || "(chưa đặt tên)";
  const nhom = (k.group || "").trim() || "Chưa phân nhóm";
  return (
    `Link này ĐÃ CÓ trong kênh "${ten}" (nhóm "${nhom}") — không thêm nữa để ` +
    `khỏi ghi đè tên/nhóm/thư mục của kênh đó. Muốn thêm 1 key nguồn nữa cho ` +
    `kênh đó thì mở thẻ kênh rồi dán key vào ô "thêm key".`
  );
}

/** Nhóm mặc định khi mở hộp ➕ Thêm kênh: LẤY NHÓM ĐANG XEM.
 *  `null` = đang xem tất cả nhóm → để trống cho user tự chọn.
 *  `""`   = đang xem "Chưa phân nhóm" → cũng để trống. */
export function nhomMacDinh(groupFilter: string | null): string {
  return (groupFilter ?? "").trim();
}
