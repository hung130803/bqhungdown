// @vitest-environment jsdom
/**
 * KÊNH DOUYIN — test TÍCH HỢP đúng cảnh anh Hùng gặp ở v0.2.0.
 *
 * Ảnh anh gửi: "Tất cả bài đăng · 250", ô tích xanh rõ ràng, mà nút dưới cùng
 * vẫn ghi "Chưa chọn video nào". Nguyên nhân: nhãn nút cộng cứng
 * `longList + shortList` — hai list CHỈ dùng cho YouTube, với Douyin luôn
 * RỖNG (allList mới là list thật). Lỗi có từ v0.1.5, chỉ là hồi đó Douyin lấy
 * được 21 bài nên ít ai để ý.
 *
 * Test mount CHÍNH component thật + store thật, không giả lập nửa vời.
 */
import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, act, within } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import type { ChannelVideo, ChannelInfo } from "@/types/models";
import { ChannelInput } from "./ChannelInput";
import { useChannelStore } from "@/stores/useChannelStore";

const đãGửi: string[][] = [];

vi.mock("@/ipc/commands", () => ({
  fetchChannelVideos: vi.fn(async () => ({ info: null, videos: [] })),
  cancelChannelFetch: vi.fn(async () => undefined),
  restoreDownloaded: vi.fn(async () => undefined),
  confirmDialog: vi.fn(async () => true),
}));
vi.mock("@/ipc/events", () => ({
  onDouyinScraperProgress: vi.fn(async () => () => undefined),
  onDouyinScraperNote: vi.fn(async () => () => undefined),
}));
vi.mock("@/stores/useSettingsStore", () => ({
  useSettingsStore: (sel: (s: unknown) => unknown) =>
    sel({ settings: { cookiesFile: null }, update: vi.fn() }),
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => "data:image/jpeg;base64,AAAA"),
}));

/** 2 bài THẬT anh Hùng đã tích trong ảnh — số liệu đo 16/08/2026 bằng
 *  `src-tauri/tests/douyin_probe.rs` với cookie đăng nhập thật. */
const BÀI_THẬT: ChannelVideo[] = [
  {
    url: "https://www.douyin.com/video/7655989781075217704",
    title: "2026下半年按这份片单来！10部重磅新片，总有",
    durationSec: null,
    viewCount: null, // Douyin KHÔNG cho lượt xem
    likeCount: 35316, // digg_count — số thật
    uploadDate: "20260728",
    thumbnail: "https://p3.douyinpic.com/a.jpeg",
    isShort: false,
    isPhoto: false,
    hashtags: [],
    downloaded: false,
  },
  {
    url: "https://www.douyin.com/video/7655989781075217705",
    title: "用算盘和粉笔，把一艘飞船从引力场里拽回来",
    durationSec: null,
    viewCount: null,
    likeCount: 12733,
    uploadDate: "20260719",
    thumbnail: "https://p3.douyinpic.com/b.jpeg",
    isShort: false,
    isPhoto: false,
    hashtags: [],
    downloaded: false,
  },
];

function bàiPhụ(n: number): ChannelVideo[] {
  return Array.from({ length: n }, (_, i) => ({
    url: `https://www.douyin.com/video/9${String(i).padStart(18, "0")}`,
    title: `Bài phụ ${i}`,
    durationSec: null,
    viewCount: null,
    likeCount: 1000 + i,
    uploadDate: `2026010${(i % 9) + 1}`,
    thumbnail: null,
    isShort: false,
    isPhoto: false,
    hashtags: [],
    downloaded: false,
  }));
}

const infoDouyin: ChannelInfo = {
  url: "https://www.douyin.com/user/MS4wLjABAAAA-che",
  title: "Kênh Douyin — 250 video",
  thumbnail: null,
  videoCount: 250,
  extractor: "douyin",
};

const infoYoutube: ChannelInfo = {
  url: "https://www.youtube.com/@ai-do",
  title: "Kênh YouTube",
  thumbnail: null,
  videoCount: 2,
  extractor: "youtube",
};

/** Nạp kết quả kênh. LƯU Ý: `setResult` cho MỌI bài vào diện bị loại
 *  (mặc định KHÔNG tích gì) — user phải tự tích. */
function nạp(info: ChannelInfo, videos: ChannelVideo[]) {
  act(() => {
    useChannelStore.getState().setResult(info, videos);
  });
}

/** Tích đúng những bài này (giống user bấm vào ô vuông), bỏ tích phần còn lại. */
function tích(tấtCả: ChannelVideo[], chọn: ChannelVideo[]) {
  const giữ = new Set(chọn.map((v) => v.url));
  act(() => {
    useChannelStore
      .getState()
      .setExcluded(new Set(tấtCả.filter((v) => !giữ.has(v.url)).map((v) => v.url)));
  });
}

function nútTải(): HTMLElement {
  // Nút cuối cùng trong form là nút "Tải … vào hàng đợi".
  const nút = screen.getAllByRole("button");
  return nút[nút.length - 1];
}

beforeEach(() => {
  đãGửi.length = 0;
  act(() => {
    useChannelStore.getState().resetResult();
    useChannelStore.getState().setExcluded(new Set());
  });
});
afterEach(cleanup);

describe("Kênh Douyin — nút tải phải đếm ĐÚNG số bài đang tích", () => {
  it("ĐÚNG ẢNH ANH HÙNG GỬI: 250 bài, tích 2 -> nút phải ghi 'Tải 2 video'", () => {
    const videos = [...BÀI_THẬT, ...bàiPhụ(248)];
    nạp(infoDouyin, videos);
    tích(videos, BÀI_THẬT); // đúng 2 bài anh Hùng tích trong ảnh
    render(<ChannelInput onSubmit={async (u) => void đãGửi.push(u)} />);

    expect(screen.getByText(/Tất cả bài đăng/)).toBeInTheDocument();
    const nút = nútTải();
    expect(
      nút,
      "ĐÂY LÀ LỖI ANH HÙNG BÁO: đã tích 2 bài mà nút vẫn bảo chưa chọn gì",
    ).not.toHaveTextContent("Chưa chọn video nào");
    expect(nút).toHaveTextContent("Tải 2 video vào hàng đợi");
  });

  it("tích HẾT 250 bài -> nút ghi 'Tải 250 video vào hàng đợi'", () => {
    const videos = [...BÀI_THẬT, ...bàiPhụ(248)];
    nạp(infoDouyin, videos);
    tích(videos, videos);
    render(<ChannelInput onSubmit={async (u) => void đãGửi.push(u)} />);
    expect(nútTải()).toHaveTextContent("Tải 250 video vào hàng đợi");
  });

  it("chưa tích gì -> lúc đó MỚI được ghi 'Chưa chọn video nào'", () => {
    const videos = [...BÀI_THẬT, ...bàiPhụ(8)];
    nạp(infoDouyin, videos); // setResult = loại hết, chưa tích gì
    render(<ChannelInput onSubmit={async (u) => void đãGửi.push(u)} />);
    expect(nútTải()).toHaveTextContent("Chưa chọn video nào");
  });

  it("nhãn nút khớp ĐÚNG số url thật sự được gửi đi khi bấm", async () => {
    const videos = [...BÀI_THẬT, ...bàiPhụ(8)];
    nạp(infoDouyin, videos);
    tích(videos, videos.slice(3)); // tích 7 bài
    render(<ChannelInput onSubmit={async (u) => void đãGửi.push(u)} />);

    const nút = nútTải();
    expect(nút).toHaveTextContent("Tải 7 video vào hàng đợi");
    await act(async () => {
      nút.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    expect(đãGửi[0], "số trên nhãn phải đúng bằng số url gửi đi").toHaveLength(7);
  });

  it("YouTube vẫn cộng cả 2 tab (video dài + shorts) như cũ", () => {
    const videos: ChannelVideo[] = [
      { ...BÀI_THẬT[0], url: "https://youtu.be/aaa", durationSec: 600, isShort: false },
      { ...BÀI_THẬT[1], url: "https://youtu.be/bbb", durationSec: 30, isShort: true },
    ];
    nạp(infoYoutube, videos);
    tích(videos, videos);
    render(<ChannelInput onSubmit={async (u) => void đãGửi.push(u)} />);
    // Đang ở tab "Video dài" (1 bài) nhưng nút phải đếm CẢ 2 tab = 2.
    expect(nútTải()).toHaveTextContent("Tải 2 video vào hàng đợi");
  });
});

describe("Kênh Douyin — hiện ngày đăng + lượt tim, KHÔNG bịa lượt xem", () => {
  it("mỗi dòng hiện lượt tim và ngày đăng", () => {
    nạp(infoDouyin, BÀI_THẬT);
    render(<ChannelInput onSubmit={async () => undefined} />);

    // "lượt tim" chứ không phải "tim" trơ trọi — 17/08/2026 anh Hùng nhìn chữ
    // "tim" mà không hiểu đó là gì ("sao tool tải nó hiện lượt tim là sao").
    expect(screen.getByText(/35,316 lượt tim/)).toBeInTheDocument();
    expect(screen.getByText(/12,733 lượt tim/)).toBeInTheDocument();
    // uploadDate 20260728 -> hiện theo kiểu Việt Nam.
    expect(screen.getByText("28/7/2026")).toBeInTheDocument();
    expect(screen.getByText("19/7/2026")).toBeInTheDocument();
  });

  it("KHÔNG hiện '0 view' — Douyin không cho lượt xem thì phải nói thẳng", () => {
    nạp(infoDouyin, BÀI_THẬT);
    render(<ChannelInput onSubmit={async () => undefined} />);
    expect(screen.queryByText(/0 view/)).not.toBeInTheDocument();
    expect(
      screen.getAllByText(/Douyin không cho lấy/).length,
      "phải nói rõ vì sao thiếu lượt xem, đừng im lặng để trống",
    ).toBeGreaterThan(0);
  });

  it("'Mới nhất' sắp theo NGÀY THẬT, không tin thứ tự Douyin trả (bài ghim lên đầu)", () => {
    // Giống hệt thứ tự thật đo được: bài ghim cũ nằm trên bài mới.
    const ghimCũ: ChannelVideo = {
      ...BÀI_THẬT[0],
      url: "https://www.douyin.com/video/111",
      title: "Bài GHIM cũ",
      uploadDate: "20260118",
    };
    nạp(infoDouyin, [ghimCũ, BÀI_THẬT[0], BÀI_THẬT[1]]);
    render(<ChannelInput onSubmit={async () => undefined} />);

    const dòng = document.querySelectorAll("[data-vid-url]");
    expect(dòng[0].getAttribute("data-vid-url")).toBe(BÀI_THẬT[0].url); // 28/07
    expect(dòng[1].getAttribute("data-vid-url")).toBe(BÀI_THẬT[1].url); // 19/07
    expect(dòng[2].getAttribute("data-vid-url")).toBe(ghimCũ.url); // 18/01
  });

  it("ô lọc đổi nhãn sang 'Lượt tim từ/đến' khi nền tảng không có lượt xem", () => {
    nạp(infoDouyin, BÀI_THẬT);
    render(<ChannelInput onSubmit={async () => undefined} />);
    expect(screen.getByPlaceholderText("Lượt tim từ")).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Lượt tim đến")).toBeInTheDocument();
    // Ô sắp xếp là combobox đầu tiên trong thanh lọc.
    const sắp = screen.getAllByRole("combobox")[0];
    expect(within(sắp).getByText("Nhiều lượt tim nhất")).toBeInTheDocument();
    expect(within(sắp).queryByText("Nhiều lượt xem nhất")).not.toBeInTheDocument();
  });
});
