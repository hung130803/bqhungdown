// @vitest-environment jsdom
/**
 * CỔNG CHỖ NỐI RUST ↔ GIAO DIỆN — ô cookie riêng từng trang.
 *
 * ═══ VÌ SAO PHẢI CÓ FILE NÀY ═══
 * Lớp bệnh "giao diện tự chế dữ liệu" đã cắn app này HAI lần: 0.3.0 làm RƠI
 * trường (ngày đăng + lượt tim bị vứt ngay tại `.map()` của giao diện), 0.3.1
 * BỊA trường (tên kênh ghép cứng, ảnh đại diện lấy nhầm ảnh bìa video). Cả hai
 * lần bộ kiểm đều XANH, vì mọi cổng đều tự dựng dữ liệu giả đã khớp sẵn với
 * giao diện rồi nhét thẳng vào store — chưa cổng nào đi qua đoạn dây thật.
 *
 * Cookie nhiều trang có đúng hình dạng để dính lại cái bẫy đó: một bảng
 * `siteCookies` lồng nhau, khoá do Rust đặt (`youtube`/`tiktok`/`douyin`…),
 * mỗi ô lại có `file`/`browser`. Gõ sai một tên khoá là ô hiện trống, anh Hùng
 * tưởng chưa nạp cookie và nạp lại vô ích — TypeScript không kêu một tiếng vì
 * interface chỉ tồn tại lúc biên dịch.
 *
 * ═══ CỔNG NÀY KHÁC Ở ĐÂU ═══
 * 1. Dữ liệu nạp vào là GÓI JSON THẬT do CHÍNH RUST sinh ra
 *    (`src-tauri/tests/fixtures/settings_cookies_seam.json`), không phải chữ
 *    gõ tay ở đây. Test Rust `khoa_goi_settings_that_gui_len_giao_dien` khẳng
 *    định file đó đúng bằng thứ `get_settings` serialize ra lúc này.
 * 2. KHÔNG giả lập `@/ipc/commands` lẫn `useSettingsStore`. `getSettings` và
 *    `updateSettings` chạy THẬT, chỉ chặn ở tầng `invoke` — đúng chỗ nối cần canh.
 * 3. Bấm qua GIAO DIỆN thật (nút ✕ của đúng dòng trang đó), không gọi tắt vào store.
 *
 * ═══ ĐÃ TỰ KIỂM CỔNG (đổi về sai thì phải ĐỎ) ═══ — xem báo cáo, mã thoát 1.
 * - Đổi `settings.siteCookies?.[s.key]` thành `undefined` ở SettingsPage
 *   (= giao diện đánh rơi bảng Rust gửi lên, đúng kiểu 0.3.0) → ĐỎ.
 * - Đổi khoá trong fixture `siteCookies` → `site_cookies` (lệch camelCase) → ĐỎ.
 */
import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, act, fireEvent, waitFor } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { SettingsPage } from "./SettingsPage";
import { useSettingsStore } from "@/stores/useSettingsStore";

// ── Gói JSON THẬT Rust gửi lên ───────────────────────────────────────────────
// Đọc từ đĩa, KHÔNG gõ lại ở đây.
const THƯ_MỤC_NÀY = path.dirname(fileURLToPath(import.meta.url));
const ĐƯỜNG_FIXTURE = path.resolve(
  THƯ_MỤC_NÀY,
  "../../src-tauri/tests/fixtures/settings_cookies_seam.json",
);
const GÓI_RUST_TRẢ_VỀ = JSON.parse(readFileSync(ĐƯỜNG_FIXTURE, "utf8")) as Record<
  string,
  unknown
>;

const { invokeGiả } = vi.hoisted(() => ({ invokeGiả: vi.fn() }));

// Chặn ở TẦNG DƯỚI CÙNG THẬT SỰ: `window.__TAURI_INTERNALS__.invoke` — đúng
// cái cầu mà `@tauri-apps/api` gọi xuống. Mọi lớp TS phía trên (commands.ts,
// store, component) chạy NGUYÊN BẢN.
//
// ĐO ĐƯỢC, KHÔNG ĐOÁN: chỉ `vi.mock("@tauri-apps/api/core")` là KHÔNG đủ —
// `get_settings` bị chặn nhưng `update_settings` gọi từ trong component vẫn
// rơi vào core.js thật (thấy `window.__TAURI_INTERNALS__` undefined trong vết
// lỗi). Chặn ở cầu thì cả hai chiều đều đi qua đây.
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeGiả }));
(globalThis as unknown as { window: Record<string, unknown> }).window
  .__TAURI_INTERNALS__ = { invoke: invokeGiả };

// i18n: trả lại chính khoá để cổng không phụ thuộc bản dịch.
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (k: string) => k }),
}));

/** Bản `Settings` mà Rust đang giữ — `update_settings` áp patch lên nó. */
let bảnRustĐangGiữ: Record<string, unknown>;
/** Mọi patch giao diện gửi xuống Rust — để soi giao diện gửi ĐÚNG hình dạng gì. */
let cácPatchĐãGửi: Record<string, unknown>[];

beforeEach(async () => {
  bảnRustĐangGiữ = JSON.parse(JSON.stringify(GÓI_RUST_TRẢ_VỀ));
  cácPatchĐãGửi = [];
  invokeGiả.mockReset();
  invokeGiả.mockImplementation(async (lệnh: string, đối?: Record<string, unknown>) => {
    if (lệnh === "get_settings") return bảnRustĐangGiữ;
    if (lệnh === "update_settings") {
      const patch = (đối?.patch ?? {}) as Record<string, unknown>;
      cácPatchĐãGửi.push(patch);
      bảnRustĐangGiữ = { ...bảnRustĐangGiữ, ...patch };
      return bảnRustĐangGiữ;
    }
    return null;
  });
  // Nạp cài đặt qua ĐƯỜNG THẬT: store → commands.getSettings → invoke.
  await act(async () => {
    await useSettingsStore.getState().refresh();
  });
});
afterEach(cleanup);

describe("Chỗ nối Rust → giao diện: ô cookie từng trang phải hiện ĐÚNG file Rust gửi", () => {
  it("file cổng phải là gói Rust thật, không rỗng", () => {
    expect(
      typeof GÓI_RUST_TRẢ_VỀ.siteCookies === "object" && GÓI_RUST_TRẢ_VỀ.siteCookies !== null,
      `Thiếu/hỏng ${ĐƯỜNG_FIXTURE}. Sinh lại bằng: ` +
        "BQD_UPDATE_FIXTURE=1 cargo test khoa_goi_settings",
    ).toBe(true);
  });

  it("mỗi trang hiện ĐÚNG file cookie của trang đó — không lẫn, không trống", () => {
    render(<SettingsPage />);
    // Đọc thẳng từ gói Rust, không gõ lại đường dẫn ở đây.
    const ô = GÓI_RUST_TRẢ_VỀ.siteCookies as Record<string, { file?: string | null }>;
    const fileYouTube = ô.youtube?.file as string;
    const fileDouyin = ô.douyin?.file as string;
    expect(fileYouTube, "fixture phải có ô youtube").toBeTruthy();
    expect(fileDouyin, "fixture phải có ô douyin").toBeTruthy();

    // Giao diện phải hiện ĐÚNG hai đường dẫn khác nhau đó.
    expect(
      screen.getByDisplayValue(fileYouTube),
      "ô YouTube không hiện file Rust gửi lên",
    ).toBeInTheDocument();
    expect(
      screen.getByDisplayValue(fileDouyin),
      "ô Douyin không hiện file Rust gửi lên",
    ).toBeInTheDocument();
    // Ô CHUNG vẫn còn nguyên, không bị ô riêng nuốt mất.
    expect(
      screen.getByDisplayValue(GÓI_RUST_TRẢ_VỀ.cookiesFile as string),
    ).toBeInTheDocument();
  });

  it("ô đặt trình duyệt hiện đúng tên trình duyệt Rust gửi", () => {
    render(<SettingsPage />);
    const ô = GÓI_RUST_TRẢ_VỀ.siteCookies as Record<string, { browser?: string | null }>;
    const trìnhDuyệt = ô.tiktok?.browser as string; // "firefox"
    expect(trìnhDuyệt, "fixture phải có ô tiktok dùng trình duyệt").toBeTruthy();
    expect(
      screen.getByDisplayValue(trìnhDuyệt),
      "ô TikTok không hiện trình duyệt Rust gửi lên",
    ).toBeInTheDocument();
  });

  it("trang CHƯA có ô riêng thì để trống + nói rõ là dùng ô chung", () => {
    render(<SettingsPage />);
    const ô = GÓI_RUST_TRẢ_VỀ.siteCookies as Record<string, unknown>;
    expect(ô.facebook, "fixture cố ý KHÔNG có ô facebook").toBeUndefined();
    // Có ít nhất một dòng để trống mang nhãn "dùng ô chung".
    expect(screen.getAllByPlaceholderText("dùng ô chung").length).toBeGreaterThan(0);
  });

  it("xoá ô một trang thì gửi xuống Rust bảng THIẾU ĐÚNG trang đó, giữ nguyên trang khác", async () => {
    render(<SettingsPage />);
    const ô = GÓI_RUST_TRẢ_VỀ.siteCookies as Record<string, { file?: string | null }>;
    const fileYouTube = ô.youtube?.file as string;

    // Bấm ✕ ở ĐÚNG dòng YouTube (nút nằm cùng hàng với ô hiện file YouTube).
    const dòng = screen.getByDisplayValue(fileYouTube).closest("div") as HTMLElement;
    const nútXoá = Array.from(dòng.querySelectorAll("button")).find(
      b => b.textContent?.trim() === "✕",
    ) as HTMLButtonElement;
    expect(nútXoá, "không tìm thấy nút xoá của dòng YouTube").toBeTruthy();
    await act(async () => {
      fireEvent.click(nútXoá);
    });
    // `update` là fire-and-forget + có `await import()` bên trong, nên phải CHỜ
    // patch thật sự tới Rust rồi mới soi — khẳng định sớm là đọc mảng rỗng.
    await waitFor(() => expect(cácPatchĐãGửi.length).toBeGreaterThan(0));

    // Patch gửi xuống Rust phải là CẢ BẢNG, thiếu đúng youtube, còn nguyên douyin.
    const patch = cácPatchĐãGửi.at(-1) as { siteCookies?: Record<string, unknown> };
    expect(patch?.siteCookies, "giao diện phải gửi khoá `siteCookies`").toBeTruthy();
    expect(
      patch.siteCookies!.youtube,
      "xoá ô YouTube mà bảng gửi xuống vẫn còn youtube",
    ).toBeUndefined();
    expect(
      patch.siteCookies!.douyin,
      "xoá ô YouTube mà lại làm mất luôn ô Douyin",
    ).toBeTruthy();
  });

  it("chọn trình duyệt cho một trang thì gửi xuống đúng khoá `browser`", async () => {
    render(<SettingsPage />);
    const dòngTrống = screen.getAllByPlaceholderText("dùng ô chung")[0];
    const dòng = dòngTrống.closest("div") as HTMLElement;
    const select = dòng.querySelector("select") as HTMLSelectElement;
    await act(async () => {
      fireEvent.change(select, { target: { value: "chrome" } });
    });
    await waitFor(() => expect(cácPatchĐãGửi.length).toBeGreaterThan(0));
    const patch = cácPatchĐãGửi.at(-1) as { siteCookies?: Record<string, { browser?: string }> };
    const đãĐặt = Object.values(patch?.siteCookies ?? {}).some(v => v?.browser === "chrome");
    expect(đãĐặt, "chọn trình duyệt mà patch không mang khoá `browser`").toBe(true);
  });
});
