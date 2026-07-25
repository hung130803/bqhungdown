/**
 * ➕ THÊM 1 LƯỢT CHO CẢ NHÓM — phần LOGIC THUẦN (không React, không IPC) để
 * unit-test được mọi nhánh.
 *
 * QUY TẮC (chọn cho DỄ ĐOÁN, không bao giờ tải ồ ạt):
 *   MỖI kênh đang tích ✓ được tải THÊM ĐÚNG 1 video — bất kể hôm nay đã tải
 *   hay còn "Chờ lượt". Bấm 2 lần = mỗi kênh 2 video. Không có đường nào ra
 *   4-5 video một phát.
 *
 * BỎ QUA có lý do rõ ràng (để UI báo lại cho user, không im lặng):
 *   - `off`  : không key nào đang tích ✓ → user chủ động tắt kênh đó.
 *   - `busy` : còn video đang tải/chờ → cộng thêm lúc chưa xong sẽ thành 2
 *              video chạy song song cho 1 kênh, dễ thành "tải quá nhiều".
 *   - `dry`  : hết kho (`sourceEmpty`) → chắc chắn trả 0 mà vẫn tốn 1 lượt
 *              quét mạng; 100 kênh là chờ rất lâu vô ích.
 * Mỗi kênh chỉ vào ĐÚNG MỘT nhóm (thứ tự off → busy → dry → run), nên
 * `off + busy + dry + run` luôn bằng số kênh trong phạm vi — không đếm trùng.
 */

/** Một key nguồn của kênh (chỉ những trường liên quan tới quyết định). */
export type MoreKey = {
  id: string;
  enabled: boolean;
  sourceEmpty?: boolean | null;
  /** Ngày local YYYY-MM-DD của lần tự tải gần nhất. */
  dripDate?: string | null;
  dripCount?: number | null;
};

/** Một KÊNH ĐÍCH (gom nhiều key cùng tên). */
export type MoreKenh<K extends MoreKey = MoreKey> = {
  group: string;
  keys: K[];
};

export type MorePlan<T> = {
  /** Sẽ được +1 video. */ run: T[];
  /** Bỏ qua: chưa tích ✓. */ off: T[];
  /** Bỏ qua: đang tải. */ busy: T[];
  /** Bỏ qua: hết kho. */ dry: T[];
};

/**
 * Chia danh sách kênh thành sẽ-chạy / bị-bỏ-qua.
 *
 * @param groupFilter `null` = mọi nhóm; `""` = nhóm "Chưa phân nhóm";
 *                    ngược lại chỉ đúng nhóm đó (KHÔNG đụng nhóm khác).
 * @param isBusy      kênh còn video đang tải/chờ hay không (UI truyền vào).
 */
export function planAddMore<T extends MoreKenh>(
  kenh: readonly T[],
  groupFilter: string | null,
  isBusy: (k: T) => boolean,
): MorePlan<T> {
  const out: MorePlan<T> = { run: [], off: [], busy: [], dry: [] };
  for (const k of kenh) {
    if (groupFilter !== null && (k.group || "") !== groupFilter) continue;
    if (!k.keys.some((c) => c.enabled)) {
      out.off.push(k);
    } else if (isBusy(k)) {
      out.busy.push(k);
    } else if (k.keys.some((c) => c.sourceEmpty)) {
      out.dry.push(k);
    } else {
      out.run.push(k);
    }
  }
  return out;
}

/** Tổng số video kênh này ĐÃ TỰ TẢI hôm nay (cộng mọi key). */
export function takenToday(
  keys: readonly MoreKey[],
  todayStr: string,
): number {
  return keys.reduce(
    (s, c) => s + (c.dripDate === todayStr ? (c.dripCount ?? 0) : 0), 0);
}

/**
 * Chia kênh theo HẠN MỨC NGÀY cho nút "▶ Chạy nhóm" — KHÁC hẳn "➕ Thêm 1
 * lượt": nút Chạy nhóm chỉ tải cho ĐỦ hạn mức, kênh đã đủ suất thì ĐỨNG YÊN.
 *
 * VÌ SAO CẦN (anh Hùng thắc mắc): hộp xác nhận cũ ghi "Chạy 11 kênh đang
 * tích ✓" nhưng thực tế chỉ 3 kênh tải (8 kênh kia đã đủ 1/ngày) → tưởng nó
 * tải lại hết. Nay đếm tách ra để hộp thoại nói đúng sự thật.
 *
 * Khớp ĐÚNG chốt của backend (`watcher::apply`): mọi đường lấy video đều gác
 * bởi `drip_count < limit`, với `limit = daily_limit` kẹp trong 1..=3, và
 * `drip_count` chỉ tính khi `drip_date` là HÔM NAY (sang ngày là về 0).
 *
 * `slots` = tổng số suất còn lại — tức số video TỐI ĐA có thể tải (còn thật
 * sự tải được hay không thì phụ thuộc kho video, nên UI phải nói "tối đa").
 */
/// `limitOf` là THAM SỐ BẮT BUỘC (không phải trường tuỳ chọn trên object):
/// hạn mức nằm ở `k.rep.dailyLimit` chứ không phải trên chính kênh, mà nếu để
/// dạng trường tuỳ chọn thì thiếu nó TypeScript KHÔNG báo — mọi kênh sẽ bị
/// coi là 1/ngày và kênh 3/ngày đã tải 1 bị báo sai "đã đủ suất" (bug thật đã
/// bị test bắt). Bắt buộc truyền vào thì không thể quên.
export function planCheckNow<T extends MoreKenh>(
  kenh: readonly T[],
  groupFilter: string | null,
  todayStr: string,
  limitOf: (k: T) => number | null | undefined,
): { run: T[]; full: T[]; off: T[]; slots: number } {
  const out = { run: [] as T[], full: [] as T[], off: [] as T[], slots: 0 };
  for (const k of kenh) {
    if (groupFilter !== null && (k.group || "") !== groupFilter) continue;
    if (!k.keys.some((c) => c.enabled)) {
      out.off.push(k);
      continue;
    }
    const limit = Math.min(3, Math.max(1, limitOf(k) ?? 1));
    const left = limit - takenToday(k.keys, todayStr);
    if (left > 0) {
      out.run.push(k);
      out.slots += left;
    } else {
      out.full.push(k);
    }
  }
  return out;
}

/**
 * Key sẽ nhận lệnh "+1" của một kênh — GIỐNG HỆT nút ➕ Thêm từng kênh:
 * ưu tiên key ĐÃ TỰ TẢI HÔM NAY (để bộ đếm cộng dồn trên đúng key đó),
 * không có thì key đang tích ✓ đầu tiên, cuối cùng mới tới key đại diện.
 *
 * `dripDate` của ngày KHÁC (hôm qua) KHÔNG tính — nếu không, sang ngày mới
 * bộ đếm sẽ cộng vào key cũ và số "đã tải hôm nay" hiện sai.
 */
export function pickKeyForMore<K extends MoreKey>(
  keys: readonly K[],
  rep: K,
  todayStr: string,
): K {
  return (
    keys.find((c) => c.dripDate === todayStr && (c.dripCount ?? 0) > 0)
    ?? keys.find((c) => c.enabled)
    ?? rep
  );
}
