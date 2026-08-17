// @vitest-environment jsdom
/**
 * CỔNG CHỖ NỐI RUST ↔ GIAO DIỆN — nửa còn thiếu của bộ kiểm.
 *
 * ═══ VÌ SAO PHẢI CÓ FILE NÀY ═══
 * Bản 0.3.0 phát hành với: Rust ĐÚNG (`ChannelVideo` có `rename_all =
 * "camelCase"`, điền đủ `like_count` + `upload_date`), 194 test Rust XANH,
 * 101 test TS XANH — mà anh Hùng cài xong vẫn KHÔNG thấy ngày đăng lẫn lượt
 * tim.
 *
 * Lý do: lệnh `scrape_douyin_channel` trả `DouyinPost` thô, rồi
 * `ChannelInput.tsx` TỰ VIẾT LẠI một vòng `.map()` đổi post → ChannelVideo và
 * vòng đó chỉ chép 4 trường (url/title/thumbnail/isPhoto). `createTime` và
 * `likeCount` Rust gửi lên bị VỨT BỎ ngay tại giao diện. `interface DouyinPost`
 * bên TS lại khai THIẾU đúng hai trường đó, nên TypeScript không kêu một tiếng.
 *
 * Và mọi cổng cũ đều mù chỗ này, vì chúng TỰ DỰNG dữ liệu giả bằng tên khoá
 * đã khớp sẵn với giao diện rồi nhét thẳng vào store (xem
 * `ChannelInput.douyin.test.tsx`: hằng `BÀI_THẬT` + `nạp()` gọi
 * `useChannelStore.setResult`). Chưa cổng nào đi qua đoạn dây thật
 * Rust → `invoke` → `commands.ts` → component. Cổng khớp sẵn với giao diện là
 * cổng VÔ DỤNG — nó chỉ kiểm giao diện với chính nó.
 *
 * ═══ CỔNG NÀY KHÁC Ở ĐÂU ═══
 * 1. Dữ liệu nạp vào là GÓI JSON THẬT do CHÍNH RUST sinh ra
 *    (`src-tauri/tests/fixtures/douyin_ui_seam.json`), không phải chữ gõ tay ở
 *    đây. Test Rust `khoa_goi_json_that_gui_len_giao_dien` khẳng định file đó
 *    đúng bằng thứ `scrape_douyin_channel` serialize ra lúc này — Rust đổi tên
 *    khoá hay bỏ trường là file đổi theo / cổng Rust đỏ, rồi cổng này đỏ theo.
 * 2. KHÔNG giả lập `@/ipc/commands`. `scrapeDouyinChannel` chạy THẬT, chỉ chặn
 *    ở tầng `invoke` — tức đúng chỗ nối cần canh.
 * 3. Bấm qua GIAO DIỆN thật (gõ URL → bấm "Lấy danh sách"), không gọi tắt
 *    vào store.
 *
 * ═══ ĐÃ TỰ KIỂM CỔNG (đổi về sai thì phải ĐỎ) ═══
 * - Trả lại vòng `.map()` 4 trường của 0.3.0 vào `ChannelInput.tsx`
 *   → ĐỎ: không thấy "35,316 tim" lẫn "28/7/2026".
 * - Đổi khoá trong fixture `likeCount` → `like_count`, `uploadDate` →
 *   `upload_date` (đúng kiểu lệch mà đề bài nghi)
 *   → ĐỎ: mất cả cột tim lẫn ngày.
 * - Bỏ `rename_all` ở `ChannelVideo` bên Rust → cổng Rust đỏ trước.
 */
import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { ChannelInput } from "./ChannelInput";
import { useChannelStore } from "@/stores/useChannelStore";

// ── Gói JSON THẬT Rust gửi lên ───────────────────────────────────────────────
// Đọc từ đĩa, KHÔNG gõ lại ở đây. Gõ lại = lại rơi vào cái bẫy "dữ liệu giả
// khớp sẵn với giao diện" đã làm 0.3.0 xanh oan.
const THƯ_MỤC_NÀY = path.dirname(fileURLToPath(import.meta.url));
const ĐƯỜNG_FIXTURE = path.resolve(
  THƯ_MỤC_NÀY,
  "../../src-tauri/tests/fixtures/douyin_ui_seam.json",
);
const GÓI_RUST_TRẢ_VỀ = JSON.parse(readFileSync(ĐƯỜNG_FIXTURE, "utf8")) as {
  info: { title: string; thumbnail: string | null; videoCount: number | null };
  videos: unknown[];
};

const { invokeGiả } = vi.hoisted(() => ({ invokeGiả: vi.fn() }));

// Chỉ chặn ở TẦNG DƯỚI CÙNG (cầu Tauri). Mọi lớp TS phía trên chạy thật.
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeGiả }));

// Giả lập TỐI THIỂU: chỉ những lệnh KHÔNG thuộc chỗ nối đang kiểm.
// `scrapeDouyinChannel` giữ NGUYÊN BẢN THẬT — đó là thứ cần canh.
vi.mock("@/ipc/commands", async (importOriginal) => {
  const thật = await importOriginal<typeof import("@/ipc/commands")>();
  return {
    ...thật,
    fetchChannelVideos: vi.fn(async () => ({ info: null, videos: [] })),
    cancelChannelFetch: vi.fn(async () => undefined),
    restoreDownloaded: vi.fn(async () => undefined),
    confirmDialog: vi.fn(async () => true),
  };
});
vi.mock("@/ipc/events", () => ({
  onDouyinScraperProgress: vi.fn(async () => () => undefined),
  onDouyinScraperNote: vi.fn(async () => () => undefined),
}));
vi.mock("@/stores/useSettingsStore", () => ({
  useSettingsStore: (sel: (s: unknown) => unknown) =>
    sel({ settings: { cookiesFile: null }, update: vi.fn() }),
}));

const URL_KÊNH = "https://www.douyin.com/user/MS4wLjABAAAA-che";

/** Gõ URL kênh rồi bấm "Lấy danh sách" — đúng thao tác anh Hùng làm. */
async function bấmLấyDanhSách() {
  render(<ChannelInput onSubmit={async () => undefined} />);
  fireEvent.change(screen.getByPlaceholderText(/MrBeast/), {
    target: { value: URL_KÊNH },
  });
  await act(async () => {
    fireEvent.click(screen.getByText("Lấy danh sách"));
  });
}

beforeEach(() => {
  invokeGiả.mockReset();
  invokeGiả.mockImplementation(async (lệnh: string) => {
    // Đây là chỗ Rust trả kết quả về giao diện.
    if (lệnh === "scrape_douyin_channel") return GÓI_RUST_TRẢ_VỀ;
    // Ảnh thu nhỏ đi qua invoke riêng — không liên quan cổng này.
    return "data:image/jpeg;base64,AAAA";
  });
  act(() => {
    useChannelStore.getState().resetResult();
    useChannelStore.getState().setExcluded(new Set());
  });
});
afterEach(cleanup);

describe("Chỗ nối Rust → giao diện: JSON THẬT phải ra dòng CÓ ngày + CÓ tim", () => {
  it("file cổng phải là gói Rust thật, không rỗng", () => {
    expect(
      Array.isArray(GÓI_RUST_TRẢ_VỀ.videos) && GÓI_RUST_TRẢ_VỀ.videos.length >= 2,
      `Thiếu/hỏng ${ĐƯỜNG_FIXTURE}. Sinh lại bằng: ` +
        "BQD_UPDATE_FIXTURE=1 cargo test khoa_goi_json",
    ).toBe(true);
    expect(
      typeof GÓI_RUST_TRẢ_VỀ.info?.title === "string",
      "gói Rust PHẢI kèm khối `info` (tên kênh + ảnh đại diện)",
    ).toBe(true);
  });

  it("LỖI ANH HÙNG BÁO: cài 0.3.0 mà không thấy ngày đăng / lượt tim", async () => {
    await bấmLấyDanhSách();

    // Đúng lệnh Rust, đúng tham số — nếu đổi tên lệnh thì cổng này đỏ luôn.
    expect(invokeGiả).toHaveBeenCalledWith("scrape_douyin_channel", { url: URL_KÊNH });

    // ── Hai thứ 0.3.0 đánh rơi ────────────────────────────────────────────
    expect(
      screen.getByText(/35,316 lượt tim/),
      "LƯỢT TIM không ra tới giao diện — đúng lỗi 0.3.0: gói Rust có `likeCount` " +
        "nhưng đường TS làm rơi mất",
    ).toBeInTheDocument();
    expect(screen.getByText(/12,733 lượt tim/)).toBeInTheDocument();

    expect(
      screen.getByText("28/7/2026"),
      "NGÀY ĐĂNG không ra tới giao diện — gói Rust có `uploadDate` nhưng đường " +
        "TS làm rơi mất",
    ).toBeInTheDocument();
    expect(screen.getByText("19/7/2026")).toBeInTheDocument();
  });

  it("mỗi bài trong gói Rust ra đúng MỘT dòng, không rơi bài nào", async () => {
    await bấmLấyDanhSách();
    expect(document.querySelectorAll("[data-vid-url]")).toHaveLength(
      GÓI_RUST_TRẢ_VỀ.videos.length,
    );
  });

  // ══ LỖI ANH HÙNG BÁO 17/08/2026 ═══════════════════════════════════════
  // "tên kênh ảnh avatar kênh không chuẩn".
  // 0.3.1 ghép TÊN + ẢNH ngay tại `ChannelInput.tsx`:
  //   title: `Kênh Douyin — ${videos.length} video`  → tên thật không bao giờ hiện
  //   thumbnail: videos[0]?.thumbnail                → ẢNH BÌA BÀI làm avatar kênh
  // Cổng này đi ĐÚNG đường đó: gói Rust thật → invoke → commands.ts → component.
  it("TÊN KÊNH phải là tên THẬT Rust gửi lên, không phải chuỗi ghép ở giao diện", async () => {
    await bấmLấyDanhSách();

    expect(
      screen.getByText(GÓI_RUST_TRẢ_VỀ.info.title),
      `Tên kênh thật ("${GÓI_RUST_TRẢ_VỀ.info.title}") không ra tới giao diện — ` +
        "đúng lỗi 0.3.1: gói Rust có `info.title` nhưng giao diện tự ghép tên khác",
    ).toBeInTheDocument();

    expect(
      screen.queryByText(/Kênh Douyin — \d+ video/),
      "vẫn còn tên GHÉP CỨNG ở giao diện — nghĩa là `info.title` Rust gửi lên bị bỏ qua",
    ).not.toBeInTheDocument();
  });

  it("ẢNH ĐẠI DIỆN phải là avatar KÊNH, không phải ảnh bìa bài", async () => {
    await bấmLấyDanhSách();

    const avatar = GÓI_RUST_TRẢ_VỀ.info.thumbnail ?? "";
    expect(
      avatar,
      "gói Rust phải kèm ảnh đại diện kênh (đường dẫn có `aweme-avatar`)",
    ).toContain("aweme-avatar");

    // Ảnh Douyin đi qua backend tải hộ (đổi thành data URL) nên KHÔNG canh
    // `<img src>`. Canh ô avatar đang trỏ vào URL nào — đó là thứ giao diện
    // thật sự chọn để hiện.
    const ôAvatar = document.querySelector("[data-channel-avatar]");
    expect(ôAvatar, "phải có ô ảnh đại diện kênh").toBeTruthy();
    expect(
      ôAvatar?.getAttribute("data-channel-avatar"),
      "ô ảnh đại diện đang trỏ vào ảnh KHÁC với avatar Rust gửi lên",
    ).toBe(avatar);

    // Chốt chặn cho ĐÚNG lỗi 0.3.1: avatar không được trùng ảnh bìa của BẤT KỲ
    // bài nào. Trả lại `thumbnail: videos[0]?.thumbnail` là dòng này đỏ.
    const ảnhBìaCácBài = (GÓI_RUST_TRẢ_VỀ.videos as { thumbnail: string | null }[]).map(
      (v) => v.thumbnail,
    );
    expect(
      ảnhBìaCácBài,
      "ảnh đại diện kênh đang là ẢNH BÌA MỘT BÀI — đúng lỗi anh Hùng báo",
    ).not.toContain(ôAvatar?.getAttribute("data-channel-avatar"));
  });

  it("KHÔNG bịa '0 view' — Douyin không cho lượt xem thì phải nói thẳng", async () => {
    await bấmLấyDanhSách();
    // `viewCount` trong gói Rust là null. Hiện "0 view" là BỊA số.
    expect(screen.queryByText(/0 view/)).not.toBeInTheDocument();
    expect(
      screen.getAllByText(/Douyin không cho lấy/).length,
      "thiếu lượt xem thì phải nói rõ vì sao, đừng im lặng để trống",
    ).toBeGreaterThan(0);
  });

  it("lọc/sắp xếp đổi sang thước đo TIM vì gói Rust không có view", async () => {
    await bấmLấyDanhSách();
    // GHI CHÚ THẲNG: đây KHÔNG phải chốt chặn cho lỗi rơi `likeCount` — đã đo,
    // khi rơi mất tim thì `hasViews` cũng false nên nhãn vẫn ra "Tim từ" và
    // test này vẫn xanh. Nó chỉ canh riêng chuyện đừng bao giờ hiện "View từ"
    // cho Douyin. Chốt chặn thật cho tim/ngày là test "LỖI ANH HÙNG BÁO" ở trên.
    expect(screen.getByPlaceholderText("Lượt tim từ")).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Lượt tim đến")).toBeInTheDocument();
  });

  // 17/08/2026 — anh Hùng: "sao tool tải nó hiện lượt tim là sao".
  // Nhãn phải TỰ NÓI RA nó là gì, và lời giải thích phải nằm trong nội dung
  // trang (đọc được), KHÔNG giấu trong thuộc tính `title` của tooltip.
  it("nhãn nói rõ 'lượt tim' và giải thích vì sao không có lượt xem", async () => {
    await bấmLấyDanhSách();

    expect(
      screen.getByText(/35,316 lượt tim/),
      "nhãn phải ghi rõ 'lượt tim', chữ 'tim' trơ trọi gây hiểu nhầm",
    ).toBeInTheDocument();

    const lờiGiải = document.querySelector("[data-metric-note]");
    expect(lờiGiải, "thiếu lời giải thích ngay trên danh sách").toBeTruthy();
    const chữ = lờiGiải?.textContent ?? "";
    expect(chữ, "phải nói rõ con số là LƯỢT TIM").toMatch(/LƯỢT TIM/);
    expect(chữ, "phải nói thẳng Douyin không cho lấy lượt xem").toMatch(
      /không cho lấy lượt xem/,
    );

    // Không được là tooltip: chữ phải nằm trong nội dung đọc được.
    expect(
      lờiGiải?.getAttribute("title"),
      "lời giải thích không được nhét vào tooltip",
    ).toBeNull();
  });

  it("sắp 'Mới nhất' theo NGÀY THẬT trong gói Rust (bài mới lên trên)", async () => {
    await bấmLấyDanhSách();
    const dòng = document.querySelectorAll("[data-vid-url]");
    // Fixture: bài …704 đăng 28/07, bài …705 đăng 19/07.
    expect(dòng[0].getAttribute("data-vid-url")).toContain("7655989781075217704");
    expect(dòng[1].getAttribute("data-vid-url")).toContain("7655989781075217705");
  });
});
