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

  // ══ 0.4.0 — LƯỢT BÌNH LUẬN + LƯỢT CHIA SẺ ═════════════════════════════
  // Hai số này đã nằm SẴN trong gói app tải về để lấy lượt tim (cùng khối
  // `statistics`), nên bóc thêm KHÔNG tốn một lượt gọi mạng nào. Douyin
  // không cho lượt xem, mà bài nổi thường CHIA SẺ > BÌNH LUẬN — nên chia sẻ
  // là thước "hot" tốt nhất lấy được miễn phí.
  //
  // Vì sao phải canh ở ĐÂY chứ không chỉ ở Rust: lớp bệnh "giao diện đánh
  // rơi / tự chế trường" đã ship HAI lần (0.3.0 rơi, 0.3.1 bịa). Cổng Rust
  // chỉ chứng minh số ra tới mép JSON; đoạn còn lại tới mắt anh Hùng là
  // đường TS, và đúng đoạn đó đã hỏng hai lần.
  it("LƯỢT BÌNH LUẬN + LƯỢT CHIA SẺ phải ra tới giao diện, không rơi ở đường TS", async () => {
    await bấmLấyDanhSách();

    // Số THẬT trong gói Rust: bài …704 → 908 bình luận / 12.272 chia sẻ.
    expect(
      screen.getByText(/908 bình luận/),
      "LƯỢT BÌNH LUẬN không ra tới giao diện — gói Rust có `commentCount` " +
        "nhưng đường TS làm rơi mất (đúng lớp bệnh 0.3.0)",
    ).toBeInTheDocument();
    expect(
      screen.getByText(/12,272 chia sẻ/),
      "LƯỢT CHIA SẺ không ra tới giao diện — gói Rust có `shareCount` nhưng " +
        "đường TS làm rơi mất",
    ).toBeInTheDocument();

    // Bài thứ hai cũng phải có, không phải chỉ dòng đầu ăn may.
    expect(screen.getByText(/325 bình luận/)).toBeInTheDocument();
    expect(screen.getByText(/1,593 chia sẻ/)).toBeInTheDocument();

    // Lượt tim vẫn còn nguyên — thêm hai cột không được đè mất cột cũ.
    expect(screen.getByText(/35,316 lượt tim/)).toBeInTheDocument();

    // Ba số phải TỰ NÓI RA nó là gì. Anh Hùng đã hỏi "sao tool hiện lượt
    // tim là sao" khi con số đứng trơ trọi — đừng lặp lại với ba con số.
    for (const chữ of [/908 bình luận/, /12,272 chia sẻ/, /35,316 lượt tim/]) {
      expect(screen.getByText(chữ).textContent ?? "").toMatch(
        /bình luận|chia sẻ|lượt tim/,
      );
    }
  });

  it("ô chọn thước đo chỉ liệt kê số CÓ THẬT — Douyin không được có 'lượt xem'", async () => {
    await bấmLấyDanhSách();

    const ôChọn = document.querySelector("[data-metric-picker]") as HTMLSelectElement | null;
    expect(
      ôChọn,
      "thiếu ô chọn thước đo — có 3 số thật (tim/bình luận/chia sẻ) thì phải " +
        "cho anh Hùng chọn sắp xếp/lọc theo số nào",
    ).toBeTruthy();

    const cácLựaChọn = Array.from(ôChọn!.options).map((o) => o.textContent ?? "");
    expect(cácLựaChọn.join(" | ")).toMatch(/lượt tim/);
    expect(cácLựaChọn.join(" | ")).toMatch(/lượt bình luận/);
    expect(cácLựaChọn.join(" | ")).toMatch(/lượt chia sẻ/);
    // Gói Rust có `viewCount: null` → tuyệt đối không được mời chọn lượt xem,
    // vì lọc theo nó là lọc trên số RỖNG mà cứ tưởng là thật.
    expect(
      cácLựaChọn.join(" | "),
      "Douyin KHÔNG có lượt xem mà ô chọn vẫn mời chọn — lọc theo nó là lọc " +
        "trên số rỗng",
    ).not.toMatch(/lượt xem/);
  });

  // CỔNG MẠNH NHẤT của nhóm này: chứng minh ô chọn thật sự ĐIỀU KHIỂN bộ lọc,
  // chứ không phải cái nhãn trang trí. Dựa trên khác biệt THẬT trong fixture:
  //   bài …704: tim 35.316 · chia sẻ 12.272
  //   bài …705: tim 12.733 · chia sẻ  1.593
  // Ngưỡng 2.000 → theo TIM giữ cả hai, theo CHIA SẺ chỉ giữ một.
  it("đổi sang 'Theo lượt chia sẻ' thì lọc chạy theo CHIA SẺ, không phải tim", async () => {
    await bấmLấyDanhSách();

    // Mặc định là lượt tim (số sát người xem nhất mà Douyin có).
    fireEvent.change(screen.getByPlaceholderText("Lượt tim từ"), {
      target: { value: "2000" },
    });
    expect(
      document.querySelectorAll("[data-vid-url]"),
      "lọc theo TIM ở ngưỡng 2.000: cả hai bài (35.316 và 12.733) phải còn",
    ).toHaveLength(2);

    // Đổi thước đo — KHÔNG đụng gì tới ô ngưỡng.
    fireEvent.change(document.querySelector("[data-metric-picker]")!, {
      target: { value: "shares" },
    });

    // Nhãn ô ngưỡng phải đổi theo, nếu không anh Hùng sẽ tưởng vẫn đang lọc tim.
    expect(
      screen.getByPlaceholderText("Lượt chia sẻ từ"),
      "đổi thước đo mà nhãn ô lọc không đổi — anh Hùng sẽ lọc nhầm số",
    ).toBeInTheDocument();

    const dòng = document.querySelectorAll("[data-vid-url]");
    expect(
      dòng,
      "lọc theo CHIA SẺ ở ngưỡng 2.000 phải loại bài …705 (1.593 chia sẻ). " +
        "Vẫn còn 2 dòng nghĩa là ô chọn chỉ đổi cái nhãn, bộ lọc vẫn chạy theo tim",
    ).toHaveLength(1);
    expect(dòng[0].getAttribute("data-vid-url")).toContain("7655989781075217704");

    // Mục "Nhiều … nhất" cũng phải nói đúng số đang dùng.
    expect(screen.getByText("Nhiều lượt chia sẻ nhất")).toBeInTheDocument();
  });
});
