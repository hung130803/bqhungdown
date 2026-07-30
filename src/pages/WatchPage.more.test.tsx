// @vitest-environment jsdom
/**
 * ➕ THÊM 1 LƯỢT CHO CẢ NHÓM — test TÍCH HỢP: mount trang Theo dõi thật,
 * bấm đúng cái nút thật, kiểm xem lệnh tải được gọi cho ĐÚNG những kênh nào.
 *
 * Mục đích: bắt lỗi ở phần NỐI DÂY (nút → hộp xác nhận → vòng lặp → báo cáo),
 * chỗ mà unit-test của `bulk-more.ts` không phủ được.
 */
import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor, cleanup } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import type { WatchedChannel } from "@/types/models";

// ── Mock TOÀN BỘ lớp IPC: không có Tauri backend trong test ──
vi.mock("@/ipc/commands", () => ({
  listWatchedChannels: vi.fn(async () => mockChannels),
  listQueue: vi.fn(async () => mockQueue),
  listHistory: vi.fn(async () => []),
  reconcileWatched: vi.fn(async () => undefined),
  archivedVideoIds: vi.fn(async () => []),
  downloadedTitleKeys: vi.fn(async () => []),
  confirmDialog: vi.fn(async () => confirmAnswer),
  downloadMoreToday: vi.fn(async (id: string) => {
    moreCalls.push(id);
    if (id === "throw") throw new Error("kênh này lỗi mạng");
    return id === "empty" ? 0 : 1;
  }),
  checkWatchedNow: vi.fn(async () => mockChannels),
  checkWatchedOne: vi.fn(async () => undefined),
  cancelAllDownloads: vi.fn(async () => 0),
  cancelDownload: vi.fn(async () => undefined),
  fetchChannelVideos: vi.fn(async () => []),
  openInFolder: vi.fn(async () => undefined),
  pickFolder: vi.fn(async () => null),
  addWatchedChannel: vi.fn(), removeWatchedChannel: vi.fn(),
  replaceWatchedUrl: vi.fn(), setVideosSkipped: vi.fn(),
  setWatchedDailyLimit: vi.fn(), setWatchedDestDir: vi.fn(),
  setWatchedEnabled: vi.fn(), setWatchedGroup: vi.fn(),
  setWatchedMaxHeight: vi.fn(), setWatchedPicked: vi.fn(),
  setWatchedSourceMode: vi.fn(), setWatchedTab: vi.fn(),
  setWatchedTarget: vi.fn(), dismissPending: vi.fn(),
  downloadPending: vi.fn(),
}));
// Trang lắng nghe sự kiện Tauri (tiến trình tải) — không có runtime Tauri
// trong test, phải giả lập kẻo `listen` nổ khi mount.
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => undefined),
}));
vi.mock("@/stores/useSettingsStore", () => ({
  useSettingsStore: (sel: (s: unknown) => unknown) =>
    sel({ settings: { watchGroups: ["Mỹ", "Hàn"] }, update: vi.fn() }),
}));
vi.mock("@/stores/useQueueStore", () => ({
  useQueueStore: (sel: (s: unknown) => unknown) =>
    sel({ items: mockQueue, refresh: vi.fn() }),
}));

let mockChannels: WatchedChannel[] = [];
let mockQueue: unknown[] = [];
let moreCalls: string[] = [];
let confirmAnswer = true;

const TODAY = (() => {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
})();

/** Một key nguồn tối thiểu nhưng ĐỦ trường để trang render như thật. */
const mkKey = (o: Partial<WatchedChannel> & { id: string }): WatchedChannel =>
  ({
    id: o.id,
    url: `https://youtube.com/@${o.id}`,
    tab: "videos",
    enabled: o.enabled ?? true,
    targetName: o.targetName ?? o.id,
    group: o.group ?? "Mỹ",
    destDir: o.destDir ?? `D:/kho/${o.targetName ?? o.id}`,
    dailyLimit: o.dailyLimit ?? 1,
    sourceMode: o.sourceMode ?? "auto",
    sourceEmpty: o.sourceEmpty ?? false,
    dripDate: o.dripDate ?? null,
    dripCount: o.dripCount ?? 0,
    picked: o.picked ?? [], doneIds: [], seenIds: [], skippedIds: [],
    dlPending: [],
    maxHeight: null, lastCheck: null,
  } as unknown as WatchedChannel);

async function mountPage() {
  const { WatchPage } = await import("./WatchPage");
  render(<WatchPage />);
  // Nút ➕ chỉ BỎ mờ khi danh sách kênh đã nạp xong (disabled khi 0 kênh)
  // → chờ nó bật là mốc "trang sẵn sàng" chính xác nhất.
  await waitFor(() => expect(btnMore()).not.toBeDisabled(), { timeout: 4000 });
}

const btnMore = () => screen.getByTitle(/TẢI THÊM 1 VIDEO cho MỖI kênh/);

beforeEach(() => {
  // Trang NHỚ nhóm đang lọc trong localStorage ("watch.groupFilter") — không
  // xoá thì ca test sau vẫn dính nhóm của ca trước.
  localStorage.clear();
  moreCalls = [];
  confirmAnswer = true;
  mockQueue = [];
  vi.clearAllMocks();
});
afterEach(() => cleanup());

describe("Nút ➕ Thêm 1 lượt (trang Theo dõi)", () => {
  it("gọi tải thêm cho MỌI kênh đang tích — kể cả kênh 'Chờ lượt'", async () => {
    mockChannels = [
      mkKey({ id: "daTai", dripDate: TODAY, dripCount: 1 }),
      mkKey({ id: "choLuot" }),                       // chưa tải hôm nay
    ];
    await mountPage();
    btnMore().click();
    await waitFor(() => expect(moreCalls.length).toBe(2), { timeout: 3000 });
    expect(moreCalls.sort()).toEqual(["choLuot", "daTai"]);
  });

  it("bấm HUỶ ở hộp xác nhận → KHÔNG tải gì", async () => {
    confirmAnswer = false;
    mockChannels = [mkKey({ id: "a" }), mkKey({ id: "b" })];
    await mountPage();
    btnMore().click();
    await new Promise((r) => setTimeout(r, 120));
    expect(moreCalls).toEqual([]);
  });

  it("hộp xác nhận nói ĐÚNG số kênh + nêu tên nhóm", async () => {
    mockChannels = [mkKey({ id: "a" }), mkKey({ id: "b" }), mkKey({ id: "c" })];
    await mountPage();
    btnMore().click();
    const cmdMod = await import("@/ipc/commands");
    await waitFor(() =>
      expect(vi.mocked(cmdMod.confirmDialog)).toHaveBeenCalled());
    const msg = vi.mocked(cmdMod.confirmDialog).mock.calls[0][0];
    expect(msg).toContain("3 kênh");
    expect(msg).toContain("mọi nhóm");        // chưa lọc nhóm nào
    expect(msg).toContain("ĐÚNG 1");
  });

  it("BỎ QUA kênh chưa tích ✓ và kênh hết kho", async () => {
    mockChannels = [
      mkKey({ id: "ok" }),
      mkKey({ id: "tat", enabled: false }),
      mkKey({ id: "hetKho", sourceEmpty: true }),
    ];
    await mountPage();
    btnMore().click();
    await waitFor(() => expect(moreCalls.length).toBe(1), { timeout: 3000 });
    expect(moreCalls).toEqual(["ok"]);
  });

  it("kênh đỏ HẾT KHO nhưng CÒN HÀNG CHỜ 🎯 vẫn được chạy — bug 28/07", async () => {
    // Anh Hùng: kênh báo đỏ hết video, đã tích 1 video vào hàng chờ, bấm chạy
    // hàng loạt → không chạy gì. Kênh đỏ có hàng chờ PHẢI được gọi tải.
    mockChannels = [
      mkKey({ id: "doConHang", sourceEmpty: true,
              picked: [{ id: "v1", url: "u", title: "t" }] as never }),
      mkKey({ id: "doTrong", sourceEmpty: true }),
    ];
    await mountPage();
    btnMore().click();
    await waitFor(() => expect(moreCalls.length).toBe(1), { timeout: 3000 });
    expect(moreCalls).toEqual(["doConHang"]);   // kênh đỏ hàng chờ trống vẫn bị bỏ qua
  });

  it("BỎ QUA kênh đang tải (khỏi 2 video song song 1 kênh)", async () => {
    mockChannels = [mkKey({ id: "ok" }), mkKey({ id: "dangTai" })];
    mockQueue = [{
      shortId: "s1", state: "downloading", bytesDownloaded: 1, bytesTotal: 10,
      request: { saveFolder: "D:/kho/dangTai", url: "u" }, title: "t",
    }];
    await mountPage();
    btnMore().click();
    await waitFor(() => expect(moreCalls.length).toBe(1), { timeout: 3000 });
    expect(moreCalls).toEqual(["ok"]);
  });

  it("1 kênh LỖI không làm đứt cả loạt — các kênh sau vẫn chạy", async () => {
    mockChannels = [
      mkKey({ id: "a" }), mkKey({ id: "throw" }), mkKey({ id: "z" }),
    ];
    await mountPage();
    btnMore().click();
    await waitFor(() => expect(moreCalls.length).toBe(3), { timeout: 4000 });
    expect(moreCalls).toContain("z");     // chạy TIẾP sau kênh lỗi
  });

  it("kênh hết video chưa làm (trả 0) được đếm riêng, không báo là đã thêm", async () => {
    mockChannels = [mkKey({ id: "a" }), mkKey({ id: "empty" })];
    await mountPage();
    btnMore().click();
    await waitFor(() => expect(moreCalls.length).toBe(2), { timeout: 3000 });
    await waitFor(() =>
      expect(screen.getByText(/Đã thêm 1 video/)).toBeTruthy(),
      { timeout: 3000 });
    expect(screen.getByText(/1 kênh hết video chưa làm/)).toBeTruthy();
  });

  it("không có kênh nào hợp lệ → báo lỗi rõ ràng, KHÔNG hỏi xác nhận", async () => {
    mockChannels = [mkKey({ id: "tat", enabled: false })];
    await mountPage();
    btnMore().click();
    const cmdMod = await import("@/ipc/commands");
    await waitFor(() =>
      expect(screen.getByText(/Không có kênh nào để thêm lượt/)).toBeTruthy(),
      { timeout: 3000 });
    expect(vi.mocked(cmdMod.confirmDialog)).not.toHaveBeenCalled();
    expect(moreCalls).toEqual([]);
  });

  it("bấm ⏹ Dừng giữa lượt → NGỪNG, không chạy hết danh sách", async () => {
    mockChannels = Array.from({ length: 8 }, (_, i) =>
      mkKey({ id: `c${i}` }));
    const cmdMod = await import("@/ipc/commands");
    // Mỗi kênh mất 60ms -> kịp bấm Dừng khi mới xong vài kênh.
    vi.mocked(cmdMod.downloadMoreToday).mockImplementation(async (id: string) => {
      moreCalls.push(id);
      await new Promise((r) => setTimeout(r, 60));
      return 1;
    });
    await mountPage();
    btnMore().click();
    await waitFor(() => expect(moreCalls.length).toBeGreaterThanOrEqual(1),
                  { timeout: 3000 });
    // Nút đã hoá thành ⏹ Dừng -> bấm lại chính nó
    await waitFor(() => expect(btnMore().textContent).toMatch(/Dừng/),
                  { timeout: 2000 });
    btnMore().click();
    await waitFor(() => expect(btnMore().textContent).toMatch(/Thêm 1 lượt/),
                  { timeout: 4000 });
    expect(moreCalls.length).toBeLessThan(8);
    expect(screen.getByText(/Đã dừng giữa lượt/)).toBeTruthy();
  });

  it("đang chạy thì nút ▶ Chạy nhóm bị KHOÁ (khỏi chạy chồng 2 việc)", async () => {
    mockChannels = Array.from({ length: 5 }, (_, i) => mkKey({ id: `d${i}` }));
    const cmdMod = await import("@/ipc/commands");
    vi.mocked(cmdMod.downloadMoreToday).mockImplementation(async (id: string) => {
      moreCalls.push(id);
      await new Promise((r) => setTimeout(r, 50));
      return 1;
    });
    await mountPage();
    btnMore().click();
    await waitFor(() => expect(btnMore().textContent).toMatch(/Dừng/),
                  { timeout: 3000 });
    expect(screen.getByText(/^▶ Chạy tất cả$/)).toBeDisabled();
    btnMore().click();   // dừng cho gọn
    await waitFor(() => expect(btnMore().textContent).toMatch(/Thêm 1 lượt/),
                  { timeout: 4000 });
  });

  it("đang lọc 1 NHÓM → chỉ kênh nhóm đó chạy, KHÔNG đụng nhóm khác", async () => {
    mockChannels = [
      mkKey({ id: "my1", group: "Mỹ" }),
      mkKey({ id: "han1", group: "Hàn" }),
      mkKey({ id: "han2", group: "Hàn" }),
    ];
    await mountPage();
    // Bấm chip lọc nhóm "Hàn". Chữ bị tách (tên + ô đếm) nên tìm theo NÚT có
    // nội dung chứa "Hàn" và ngắn (chip), không phải nút "Chạy nhóm…".
    const chip = screen.getAllByRole("button").find(
      (b) => /Hàn/.test(b.textContent ?? "")
        && (b.textContent ?? "").length < 12
        && !/Chạy/.test(b.textContent ?? ""),
    );
    expect(chip).toBeTruthy();
    chip!.click();
    await waitFor(() =>
      expect(btnMore().getAttribute("title")).toContain('nhóm "Hàn"'),
      { timeout: 2000 });
    btnMore().click();
    await waitFor(() => expect(moreCalls.length).toBe(2), { timeout: 3000 });
    expect(moreCalls.sort()).toEqual(["han1", "han2"]);
    expect(moreCalls).not.toContain("my1");
  });

  it("cộng vào ĐÚNG key đã tải hôm nay khi kênh có nhiều key", async () => {
    mockChannels = [
      mkKey({ id: "key_moi", targetName: "K", group: "Mỹ" }),
      mkKey({ id: "key_da_tai", targetName: "K", group: "Mỹ",
              dripDate: TODAY, dripCount: 2 }),
    ];
    await mountPage();
    btnMore().click();
    // 2 key = 1 KÊNH -> đúng 1 lệnh, và phải là key đã tải hôm nay
    await waitFor(() => expect(moreCalls.length).toBe(1), { timeout: 3000 });
    expect(moreCalls).toEqual(["key_da_tai"]);
  });
});

describe("Nút ▶ Chạy nhóm — hộp thoại phải nói ĐÚNG số kênh sẽ tải", () => {
  const btnRun = () => screen.getByText(/^▶ Chạy (tất cả|nhóm)/);

  it("8 kênh đã đủ 1/ngày + 3 kênh chờ lượt → báo 3 kênh / tối đa 3 video", async () => {
    // Dựng đúng ảnh anh Hùng gửi (nhóm "Mỹ mới": 11 kênh, 8 đã tải 1 hôm nay).
    mockChannels = [
      ...Array.from({ length: 8 }, (_, i) =>
        mkKey({ id: `da${i}`, dripDate: TODAY, dripCount: 1 })),
      ...Array.from({ length: 3 }, (_, i) => mkKey({ id: `cho${i}` })),
    ];
    await mountPage();
    btnRun().click();
    const cmdMod = await import("@/ipc/commands");
    await waitFor(() =>
      expect(vi.mocked(cmdMod.confirmDialog)).toHaveBeenCalled(),
      { timeout: 3000 });
    const msg = vi.mocked(cmdMod.confirmDialog).mock.calls[0][0];
    // Dòng đầu phải nêu TỔNG (11 kênh) rồi mới tới số chạy (3) — trả lời đúng
    // thắc mắc "nhóm 48 kênh mà sao báo 43".
    expect(msg).toContain("có 11 kênh");
    expect(msg).toContain("chạy 3 kênh còn suất");  // KHÔNG phải 11
    expect(msg).toContain("TỐI ĐA 3 video");
    expect(msg).toContain("8 kênh hôm nay ĐÃ TẢI ĐỦ hạn mức");
    expect(msg).toContain("ĐỨNG YÊN");
  });

  it("MỌI kênh đã đủ suất → KHÔNG hỏi, báo luôn + chỉ cách xử lý", async () => {
    mockChannels = Array.from({ length: 5 }, (_, i) =>
      mkKey({ id: `da${i}`, dripDate: TODAY, dripCount: 1 }));
    await mountPage();
    btnRun().click();
    const cmdMod = await import("@/ipc/commands");
    await waitFor(() =>
      expect(screen.getByText(/ĐÃ ĐỦ suất hôm nay/)).toBeTruthy(),
      { timeout: 3000 });
    expect(vi.mocked(cmdMod.confirmDialog)).not.toHaveBeenCalled();
    expect(vi.mocked(cmdMod.checkWatchedNow)).not.toHaveBeenCalled();
    // Câu báo phải CHỈ CÁCH khác (➕ Thêm 1 lượt) chứ không bỏ user lơ lửng.
    expect(screen.getByText(/ĐÃ ĐỦ suất hôm nay/).textContent)
      .toMatch(/Thêm 1 lượt/);
  });

  it("hạn mức 3/ngày đã tải 1 → còn 2 suất, vẫn nằm trong danh sách chạy", async () => {
    mockChannels = [
      mkKey({ id: "a", dailyLimit: 3, dripDate: TODAY, dripCount: 1 }),
    ];
    await mountPage();
    btnRun().click();
    const cmdMod = await import("@/ipc/commands");
    await waitFor(() =>
      expect(vi.mocked(cmdMod.confirmDialog)).toHaveBeenCalled(),
      { timeout: 3000 });
    const msg = vi.mocked(cmdMod.confirmDialog).mock.calls[0][0];
    expect(msg).toContain("chạy 1 kênh còn suất");
    expect(msg).toContain("TỐI ĐA 2 video");
  });

  it("bấm Huỷ ở hộp xác nhận → KHÔNG gọi lệnh chạy", async () => {
    confirmAnswer = false;
    mockChannels = [mkKey({ id: "a" }), mkKey({ id: "b" })];
    await mountPage();
    btnRun().click();
    const cmdMod = await import("@/ipc/commands");
    await waitFor(() =>
      expect(vi.mocked(cmdMod.confirmDialog)).toHaveBeenCalled(),
      { timeout: 3000 });
    await new Promise((r) => setTimeout(r, 120));
    expect(vi.mocked(cmdMod.checkWatchedNow)).not.toHaveBeenCalled();
  });

  it("đồng ý → gọi lệnh chạy với ĐÚNG nhóm đang lọc", async () => {
    mockChannels = [mkKey({ id: "a" }), mkKey({ id: "b" })];
    await mountPage();
    btnRun().click();
    const cmdMod = await import("@/ipc/commands");
    await waitFor(() =>
      expect(vi.mocked(cmdMod.checkWatchedNow)).toHaveBeenCalledWith(null),
      { timeout: 3000 });
  });

  it("bỏ hết tích ✓ → báo 'chưa có kênh nào đang tích', không hỏi", async () => {
    mockChannels = [mkKey({ id: "a", enabled: false })];
    await mountPage();
    btnRun().click();
    const cmdMod = await import("@/ipc/commands");
    await waitFor(() =>
      expect(screen.getByText(/Chưa có kênh nào đang tích/)).toBeTruthy(),
      { timeout: 3000 });
    expect(vi.mocked(cmdMod.confirmDialog)).not.toHaveBeenCalled();
  });
});
