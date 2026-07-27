// @vitest-environment jsdom
/**
 * BA LỖI NHÓM/KÊNH anh Hùng báo 27/07/2026 — test TÍCH HỢP: mount trang thật,
 * bấm nút thật, kiểm đúng lệnh nào được gọi (và lệnh nào KHÔNG được gọi).
 *
 * 1. Đang xem nhóm "Mỹ" mà thêm kênh thì nó vào nhóm "Mỹ mới" (nhóm tạo sau).
 * 2. Thêm rồi KHÔNG đổi được nhóm (ô đổi nhóm bị ẩn trong thẻ thu gọn).
 * 3. Thêm TRÙNG link: không báo gì, lại âm thầm GHI ĐÈ cấu hình kênh đang có.
 */
import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import type { WatchedChannel } from "@/types/models";

vi.mock("@/ipc/commands", () => ({
  listWatchedChannels: vi.fn(async () => mockChannels),
  listQueue: vi.fn(async () => []),
  listHistory: vi.fn(async () => []),
  reconcileWatched: vi.fn(async () => undefined),
  archivedVideoIds: vi.fn(async () => []),
  downloadedTitleKeys: vi.fn(async () => []),
  confirmDialog: vi.fn(async () => true),
  downloadMoreToday: vi.fn(async () => 1),
  checkWatchedNow: vi.fn(async () => mockChannels),
  checkWatchedOne: vi.fn(async () => undefined),
  cancelAllDownloads: vi.fn(async () => 0),
  cancelDownload: vi.fn(async () => undefined),
  fetchChannelVideos: vi.fn(async () => []),
  openInFolder: vi.fn(async () => undefined),
  pickFolder: vi.fn(async () => "D:/kho/thu-muc-moi"),
  addWatchedChannel: vi.fn(async (url: string) => {
    goiAdd.push(url);
    return mkKey({ id: "moi", url });
  }),
  removeWatchedChannel: vi.fn(),
  replaceWatchedUrl: vi.fn(),
  setVideosSkipped: vi.fn(),
  setWatchedDailyLimit: vi.fn(),
  setWatchedDestDir: vi.fn(),
  setWatchedEnabled: vi.fn(),
  setWatchedGroup: vi.fn(async (id: string, g: string | null) => {
    goiNhom.push([id, g]);
  }),
  setWatchedMaxHeight: vi.fn(),
  setWatchedPicked: vi.fn(),
  setWatchedSourceMode: vi.fn(),
  setWatchedTab: vi.fn(),
  setWatchedTarget: vi.fn(async (id: string, n: string) => {
    goiTen.push([id, n]);
  }),
  dismissPending: vi.fn(),
  downloadPending: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => undefined),
}));
vi.mock("@/stores/useSettingsStore", () => ({
  useSettingsStore: (sel: (s: unknown) => unknown) =>
    sel({ settings: { watchGroups: ["Mỹ", "Mỹ mới", "Hàn"] }, update: vi.fn() }),
}));
vi.mock("@/stores/useQueueStore", () => ({
  useQueueStore: (sel: (s: unknown) => unknown) => sel({ items: [], refresh: vi.fn() }),
}));

let mockChannels: WatchedChannel[] = [];
let goiAdd: string[] = [];
let goiNhom: [string, string | null][] = [];
let goiTen: [string, string][] = [];

const mkKey = (o: Partial<WatchedChannel> & { id: string }): WatchedChannel =>
  ({
    id: o.id,
    url: o.url ?? `https://youtube.com/@${o.id}`,
    tab: "videos",
    enabled: true,
    targetName: o.targetName ?? o.id,
    group: o.group ?? "Mỹ",
    destDir: o.destDir ?? `D:/kho/${o.targetName ?? o.id}`,
    dailyLimit: 1,
    sourceMode: "auto",
    sourceEmpty: false,
    dripDate: null,
    dripCount: 0,
    picked: [], doneIds: [], seenIds: [], skippedIds: [], dlPending: [],
    maxHeight: null, lastCheck: null,
  } as unknown as WatchedChannel);

const btnThem = () => screen.getByRole("button", { name: /➕ Thêm kênh/ });

async function mountPage() {
  const { WatchPage } = await import("./WatchPage");
  render(<WatchPage />);
  await waitFor(() => expect(btnThem()).toBeInTheDocument(), { timeout: 4000 });
  // KHÔNG chờ theo tên kênh: kênh có thể bị BỘ LỌC NHÓM ẩn đi -> chờ vô vọng.
  // Chờ mốc chắc chắn có: chip nhóm đã dựng xong (listWatchedChannels đã về).
  await waitFor(
    () => expect(screen.getAllByRole("combobox").length).toBeGreaterThan(0),
    { timeout: 4000 },
  );
}

/** Ô <select> nhóm trong HỘP ➕ Thêm kênh (có mục "— chọn nhóm —"). */
const selectNhomTrongHop = () =>
  screen.getAllByRole("combobox").find((s) =>
    Array.from((s as HTMLSelectElement).options).some(
      (o) => o.textContent === "— chọn nhóm —",
    ),
  ) as HTMLSelectElement | undefined;

/** Các ô <select> ĐỔI NHÓM trên thẻ kênh (có mục "— nhóm —"). */
const selectDoiNhom = () =>
  screen.getAllByRole("combobox").filter((s) =>
    Array.from((s as HTMLSelectElement).options).some(
      (o) => o.textContent === "— nhóm —",
    ),
  ) as HTMLSelectElement[];

beforeEach(() => {
  localStorage.clear();
  goiAdd = [];
  goiNhom = [];
  goiTen = [];
  vi.clearAllMocks();
});
afterEach(() => cleanup());

describe("LỖI 1 — thêm kênh phải vào ĐÚNG nhóm đang xem", () => {
  it("đang lọc nhóm 'Mỹ mới' → hộp Thêm kênh mặc định 'Mỹ mới'", async () => {
    mockChannels = [mkKey({ id: "a", group: "Mỹ" }), mkKey({ id: "b", group: "Mỹ mới" })];
    localStorage.setItem("watch.groupFilter", "Mỹ mới");
    await mountPage();
    btnThem().click();
    await waitFor(() => expect(selectNhomTrongHop()).toBeTruthy());
    expect(selectNhomTrongHop()!.value).toBe("Mỹ mới");
  });

  it("đang lọc nhóm 'Mỹ' → mặc định 'Mỹ' (KHÔNG dính nhóm khác)", async () => {
    mockChannels = [mkKey({ id: "a", group: "Mỹ" })];
    localStorage.setItem("watch.groupFilter", "Mỹ");
    await mountPage();
    btnThem().click();
    await waitFor(() => expect(selectNhomTrongHop()).toBeTruthy());
    expect(selectNhomTrongHop()!.value).toBe("Mỹ");
  });

  it("xem TẤT CẢ nhóm → để trống, buộc user chọn", async () => {
    mockChannels = [mkKey({ id: "a", group: "Mỹ" })];
    localStorage.setItem("watch.groupFilter", "*ALL*");
    await mountPage();
    btnThem().click();
    await waitFor(() => expect(selectNhomTrongHop()).toBeTruthy());
    expect(selectNhomTrongHop()!.value).toBe("");
  });
});

describe("LỖI 2 — đổi nhóm được NGAY trên thẻ thu gọn", () => {
  it("thẻ kênh THU GỌN vẫn có ô đổi nhóm (không phải bung ra mới thấy)", async () => {
    mockChannels = [mkKey({ id: "a", targetName: "Kênh A", group: "Mỹ" })];
    await mountPage();
    const oNhom = selectDoiNhom();
    expect(oNhom.length).toBeGreaterThanOrEqual(1);
    expect(oNhom[0].value).toBe("Mỹ");
  });

  it("chọn nhóm khác → gọi setWatchedGroup đúng kênh, đúng nhóm", async () => {
    mockChannels = [mkKey({ id: "a", targetName: "Kênh A", group: "Mỹ" })];
    await mountPage();
    const o = selectDoiNhom()[0];
    o.value = "Hàn";
    o.dispatchEvent(new Event("change", { bubbles: true }));
    await waitFor(() => expect(goiNhom.length).toBe(1));
    expect(goiNhom[0]).toEqual(["a", "Hàn"]);
  });

  it("chọn '— nhóm —' (rỗng) → bỏ kênh ra khỏi nhóm (null)", async () => {
    mockChannels = [mkKey({ id: "a", targetName: "Kênh A", group: "Mỹ" })];
    await mountPage();
    const o = selectDoiNhom()[0];
    o.value = "";
    o.dispatchEvent(new Event("change", { bubbles: true }));
    await waitFor(() => expect(goiNhom.length).toBe(1));
    expect(goiNhom[0]).toEqual(["a", null]);
  });
});

describe("LỖI 3 — thêm TRÙNG link phải BÁO, và KHÔNG ghi đè gì", () => {
  /** Điền hộp ➕ Thêm kênh rồi bấm gửi.
   *  PHẢI dùng fireEvent.change: input của React là controlled, gán .value rồi
   *  bắn Event thô KHÔNG cập nhật state (bài học: 4 ca đầu FAIL oan vì việc này).
   *  PHẢI chọn thư mục lưu: nút gửi disabled khi thiếu addDir. */
  const themKenh = async (ten: string, url: string) => {
    btnThem().click();
    await waitFor(() => expect(selectNhomTrongHop()).toBeTruthy());
    fireEvent.change(screen.getByPlaceholderText(/vd: Kênh Mỹ 1/), {
      target: { value: ten },
    });
    const oUrl = screen
      .getAllByRole("textbox")
      .find((i) => /youtube/i.test((i as HTMLInputElement).placeholder)) as HTMLInputElement;
    fireEvent.change(oUrl, { target: { value: url } });
    fireEvent.click(screen.getByRole("button", { name: /Chọn thư mục/ }));
    await waitFor(() => expect(screen.getByText(/thu-muc-moi/)).toBeInTheDocument());
    // Nhãn nút gửi là "Tạo kênh" (không phải "Thêm") — đọc từ code, không đoán.
    const nutGui = screen.getByRole("button", { name: /Tạo kênh|Đang tạo/ });
    expect(nutGui).not.toBeDisabled();
    fireEvent.click(nutGui);
  };

  it("link đã có → hiện lỗi nói RÕ kênh nào, và KHÔNG gọi lệnh nào", async () => {
    mockChannels = [
      mkKey({ id: "a", targetName: "Kênh A", group: "Mỹ",
              url: "https://youtube.com/@trung" }),
    ];
    // App BẮT BUỘC chọn nhóm mới cho tạo kênh -> đặt nhóm đang xem để nút bật.
    localStorage.setItem("watch.groupFilter", "Mỹ");
    await mountPage();
    await themKenh("Kênh Mới", "https://youtube.com/@trung");
    // "Kênh A" có ở CẢ thẻ kênh lẫn câu báo lỗi -> phải kiểm TRONG node lỗi,
    // không dùng getByText toàn trang (sẽ báo "found multiple elements").
    const oLoi = await waitFor(() => screen.getByText(/ĐÃ CÓ trong kênh/));
    expect(oLoi.textContent).toContain("Kênh A");
    expect(oLoi.textContent).toContain("Mỹ");
    // KHÔNG được gọi bất cứ lệnh ghi nào -> kênh cũ nguyên vẹn
    expect(goiAdd).toEqual([]);
    expect(goiTen).toEqual([]);
    expect(goiNhom).toEqual([]);
  });

  it("trùng nhưng KHÁC hoa/thường + có dấu / cuối → vẫn chặn", async () => {
    mockChannels = [
      mkKey({ id: "a", targetName: "Kênh A", group: "Mỹ",
              url: "https://youtube.com/@trung" }),
    ];
    localStorage.setItem("watch.groupFilter", "Mỹ");
    await mountPage();
    await themKenh("Kênh Mới", "https://YouTube.com/@Trung/");
    await waitFor(() => expect(screen.getByText(/ĐÃ CÓ trong kênh/)).toBeInTheDocument());
    expect(goiAdd).toEqual([]);
  });

  it("link MỚI thì vẫn thêm bình thường (không chặn oan)", async () => {
    mockChannels = [
      mkKey({ id: "a", targetName: "Kênh A", group: "Mỹ",
              url: "https://youtube.com/@cu" }),
    ];
    localStorage.setItem("watch.groupFilter", "Mỹ");
    await mountPage();
    await themKenh("Kênh Mới", "https://youtube.com/@hoantoanmoi");
    await waitFor(() => expect(goiAdd.length).toBe(1), { timeout: 3000 });
    expect(goiAdd[0]).toBe("https://youtube.com/@hoantoanmoi");
    // và nhóm gán phải là nhóm ĐANG XEM
    await waitFor(() => expect(goiNhom.length).toBe(1));
    expect(goiNhom[0][1]).toBe("Mỹ");
  });
});
