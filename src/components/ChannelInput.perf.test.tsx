// @vitest-environment jsdom
/**
 * ĐO TỐC ĐỘ danh sách kênh — 21 bài (đời cũ) so với 250 / 1000 / 3600 bài.
 *
 * VÌ SAO CÓ FILE NÀY: trước bản vá cookie, kênh Douyin chỉ lấy được ~21 bài
 * nên không ai biết danh sách có chịu nổi số lớn không. Nay 250, kênh to có
 * thể 3600. "Đơ hay không đơ" phải trả lời bằng SỐ, không phán cảm tính.
 *
 * Đo 2 thứ user thật sự cảm nhận:
 *   1. dựng danh sách lần đầu (mount)
 *   2. TÍCH/BỎ TÍCH một ô  ← cái này quan trọng nhất, xảy ra liên tục
 *
 * Mốc đặt rất rộng (xem NGƯỠNG_*) vì máy chạy CI/sandbox chậm thất thường —
 * cổng này để bắt lỗi ĐỘ PHỨC TẠP (bậc 2, ví dụ mỗi lần tích lại dựng lại cả
 * 3600 dòng), không phải để so từng mili-giây.
 */
import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, act } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import type { ChannelVideo, ChannelInfo } from "@/types/models";
import { ChannelInput } from "./ChannelInput";
import { useChannelStore } from "@/stores/useChannelStore";

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
// Đếm số lượt gọi backend lấy ảnh nhỏ. Ảnh Douyin (`douyinpic.com`) bắt buộc
// đi qua backend vì CDN chặn hotlink, nên MỖI DÒNG = 1 lượt IPC + 1 lượt tải
// ảnh từ CDN Douyin. Đây mới là chỗ tốn thật, không phải JS.
const lượtLấyẢnh: string[] = [];
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (tên: string, args: { url?: string }) => {
    if (tên === "fetch_thumbnail_data_url") {
      lượtLấyẢnh.push(args?.url ?? "");
      return "data:image/jpeg;base64,AAAA";
    }
    return undefined;
  }),
}));

/** Dựng N bài Douyin GIỐNG THẬT: có ngày đăng + lượt tim, KHÔNG có view.
 *
 *  `mẻ` làm URL ảnh khác nhau giữa các test. BẮT BUỘC: `Thumbnail` có cache
 *  ảnh TOÀN CỤC theo URL, dùng lại URL cũ là test sau đo được 0 lượt tải và
 *  tưởng nhầm "không tốn gì". */
function duLieuDouyin(n: number, mẻ = "a"): ChannelVideo[] {
  const out: ChannelVideo[] = [];
  for (let i = 0; i < n; i++) {
    const ngay = new Date(2026, 0, 1 + (i % 900));
    const ymd =
      `${ngay.getFullYear()}` +
      `${String(ngay.getMonth() + 1).padStart(2, "0")}` +
      `${String(ngay.getDate()).padStart(2, "0")}`;
    out.push({
      url: `https://www.douyin.com/video/76559897810752${String(i).padStart(5, "0")}`,
      title: `Bài số ${i} — 2026下半年按这份片单来，10部重磅新片`,
      durationSec: null,
      viewCount: null,
      likeCount: 35316 - (i % 30000),
      uploadDate: ymd,
      thumbnail: `https://p3.douyinpic.com/${mẻ}-${i}.jpeg`,
      isShort: false,
      isPhoto: false,
      hashtags: [],
      downloaded: false,
    });
  }
  return out;
}

const info: ChannelInfo = {
  url: "https://www.douyin.com/user/MS4wLjABAAAA-che",
  title: "Kênh Douyin — 250 video",
  thumbnail: null,
  videoCount: 250,
  extractor: "douyin",
};

function napVaoStore(videos: ChannelVideo[]) {
  act(() => {
    useChannelStore.getState().setResult(info, videos);
  });
}

/** Giả lập TẦM NHÌN: jsdom không có IntersectionObserver thật (không layout).
 *  Stub này coi `SỐ_DÒNG_NHÌN_THẤY` phần tử đầu tiên là đang hiện trên màn —
 *  đúng như khung danh sách cao 520px chỉ chứa được hơn chục dòng. */
const SỐ_DÒNG_NHÌN_THẤY = 15;
function gắnStubTầmNhìn() {
  let đãQuanSát = 0;
  class IOStub {
    constructor(private cb: IntersectionObserverCallback) {}
    observe(el: Element) {
      const hiện = đãQuanSát < SỐ_DÒNG_NHÌN_THẤY;
      đãQuanSát++;
      if (hiện) {
        this.cb(
          [{ isIntersecting: true, target: el } as IntersectionObserverEntry],
          this as unknown as IntersectionObserver,
        );
      }
    }
    unobserve() {}
    disconnect() {}
    takeRecords() {
      return [];
    }
  }
  vi.stubGlobal("IntersectionObserver", IOStub);
  return () => {
    đãQuanSát = 0;
  };
}

beforeEach(() => {
  gắnStubTầmNhìn();
  act(() => {
    useChannelStore.getState().resetResult();
    useChannelStore.getState().setExcluded(new Set());
  });
});
afterEach(cleanup);

/** Dựng danh sách N bài, trả (ms) của lần mount. */
function doMount(n: number): number {
  const videos = duLieuDouyin(n);
  napVaoStore(videos);
  const t0 = performance.now();
  render(<ChannelInput onSubmit={async () => undefined} />);
  return performance.now() - t0;
}

describe("Danh sách kênh — đo tốc độ khi số bài tăng 21 -> 3600", () => {
  // Rộng tay: cốt bắt lỗi bậc 2, không phải so từng mili-giây.
  const NGƯỠNG_MOUNT_MS = 12000;
  const NGƯỠNG_TÍCH_MS = 2500;

  it("dựng danh sách: 21 / 250 / 1000 bài — in số đo thật", () => {
    const bảng: Record<string, string> = {};
    for (const n of [21, 250, 1000]) {
      const ms = doMount(n);
      bảng[`${n} bài`] = `${ms.toFixed(0)} ms`;
      cleanup();
      expect(
        ms,
        `dựng ${n} bài mất ${ms.toFixed(0)} ms — quá ngưỡng ${NGƯỠNG_MOUNT_MS} ms`,
      ).toBeLessThan(NGƯỠNG_MOUNT_MS);
    }
    console.log("== DỰNG DANH SÁCH ==", bảng);
  });

  it("TÍCH 1 ô trong danh sách 250 bài phải nhanh (không dựng lại cả list)", () => {
    napVaoStore(duLieuDouyin(250));
    render(<ChannelInput onSubmit={async () => undefined} />);
    // Kênh Douyin không có ô "Lấy thêm số view…" (vô tác dụng) nên mọi
    // checkbox ở đây đều là một bài.
    const ô = screen.getAllByRole("checkbox");
    expect(ô.length).toBeGreaterThanOrEqual(250);

    const đo: number[] = [];
    for (let i = 0; i < 5; i++) {
      const t0 = performance.now();
      act(() => {
        ô[i].dispatchEvent(new MouseEvent("click", { bubbles: true }));
      });
      đo.push(performance.now() - t0);
    }
    const tb = đo.reduce((a, b) => a + b, 0) / đo.length;
    console.log(
      "== TÍCH 1 Ô / 250 bài ==",
      đo.map((x) => `${x.toFixed(0)} ms`).join(" · "),
      `| trung bình ${tb.toFixed(0)} ms`,
    );
    expect(tb, `tích 1 ô mất trung bình ${tb.toFixed(0)} ms`).toBeLessThan(NGƯỠNG_TÍCH_MS);
  });

  it("CHỖ TỐN THẬT: mở 250 bài bắn bao nhiêu lượt tải ảnh về CDN Douyin", () => {
    lượtLấyẢnh.length = 0;
    napVaoStore(duLieuDouyin(250, "đếm-ảnh"));
    render(<ChannelInput onSubmit={async () => undefined} />);
    console.log(
      "== LƯỢT TẢI ẢNH khi mở 250 bài ==",
      `${lượtLấyẢnh.length} lượt (mỗi lượt = 1 IPC + 1 request tới douyinpic.com)`,
    );
    // Douyin chặn theo tần suất — bắn cả trăm request một lúc là tự xin 403.
    // Chỉ được tải ảnh của những dòng user NHÌN THẤY.
    expect(
      lượtLấyẢnh.length,
      `mở 250 bài mà bắn ${lượtLấyẢnh.length} lượt tải ảnh cùng lúc — phải tải theo tầm nhìn`,
    ).toBeLessThanOrEqual(SỐ_DÒNG_NHÌN_THẤY);
  });

  it("3600 bài (kênh to nhất) vẫn dựng được, không treo", () => {
    const ms = doMount(3600);
    console.log("== DỰNG 3600 bài ==", `${ms.toFixed(0)} ms`);
    expect(screen.getByText(/Tất cả bài đăng/)).toBeInTheDocument();
    expect(ms, `dựng 3600 bài mất ${ms.toFixed(0)} ms`).toBeLessThan(NGƯỠNG_MOUNT_MS * 3);
  });
});
