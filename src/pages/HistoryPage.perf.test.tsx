// @vitest-environment jsdom
/**
 * Trang Lịch sử với lịch sử THẬT cỡ máy anh Hùng (16/11/2026: 14 953 mục).
 *
 * Trước đây trang vẽ hết 1 lượt → webview 2,4 GB, bấm tab đơ. Nay chỉ vẽ dần;
 * test này giữ 2 bất biến: (1) lần đầu chỉ vẽ một trang nhỏ, (2) các con số
 * "Chọn tất cả" / "Xoá tất cả" vẫn tính trên ĐỦ danh sách.
 */
import { describe, expect, it, vi, afterEach } from "vitest";
import { render, cleanup, act, fireEvent } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import type { HistoryEntry } from "@/types/models";

const N = 15000;
function lichSu(n: number): HistoryEntry[] {
  const base = Date.UTC(2026, 10, 16);
  return Array.from({ length: n }, (_, i) => ({
    shortId: `h${i}`,
    url: `https://www.youtube.com/watch?v=v${i}`,
    title: `Video ${i}`,
    extractor: "youtube",
    formatId: null,
    mode: "video",
    saveFolder: "D:\\video",
    outputPath: `D:\\video\\Video ${i}.mp4`,
    status: "completed",
    error: null,
    finishedAt: new Date(base - i * 60_000).toISOString(),
    channel: `Kênh ${i % 50}`,
    thumbnail: null,
    edited: false,
  })) as HistoryEntry[];
}
const DATA = lichSu(N);

vi.mock("@/ipc/commands", () => ({
  listHistory: vi.fn(async () => DATA),
  findOutputFile: vi.fn(async () => null),
  updateHistoryOutputPath: vi.fn(async () => undefined),
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (k: string) => k }) }));

import { HistoryPage } from "./HistoryPage";

afterEach(cleanup);

describe("Lịch sử 15 000 mục", () => {
  it("lần đầu chỉ vẽ 1 trang nhỏ nhưng đếm đủ", async () => {
    let view: ReturnType<typeof render> | undefined;
    await act(async () => {
      view = render(<HistoryPage />);
    });
    const rows = view!.container.querySelectorAll("[data-history-id]");
    expect(rows.length).toBeGreaterThan(0);
    expect(rows.length).toBeLessThanOrEqual(200);
    expect(view!.getByText(`Xoá tất cả (${N})`)).toBeInTheDocument();
    expect(view!.getByText(`Chọn tất cả (${N})`)).toBeInTheDocument();
    expect(view!.getByText(new RegExp(`/ ${N} — cuộn xuống`))).toBeInTheDocument();
  });

  it("bấm Xem thêm vẽ thêm đúng 1 trang", async () => {
    let view: ReturnType<typeof render> | undefined;
    await act(async () => {
      view = render(<HistoryPage />);
    });
    const truoc = view!.container.querySelectorAll("[data-history-id]").length;
    await act(async () => {
      fireEvent.click(view!.getByText("Xem thêm"));
    });
    const sau = view!.container.querySelectorAll("[data-history-id]").length;
    expect(sau).toBe(truoc + 100);
  });
});
