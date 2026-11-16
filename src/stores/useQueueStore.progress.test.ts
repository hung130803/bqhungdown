/**
 * Gom tiến độ tải: 5 mục × 4 lần/giây từ backend KHÔNG được thành 20 lần vẽ
 * lại mỗi giây (anh Hùng 16/11/2026: chạy dây chuyền đơ máy). Số cuối cùng
 * vẫn phải đúng, mục không đổi giữ nguyên object (để QueueRow memo bỏ qua).
 */
import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import type { DownloadItem, ProgressSnapshot } from "@/types/models";

vi.mock("@/ipc/commands", () => ({}));
import { useQueueStore } from "./useQueueStore";

function muc(id: string): DownloadItem {
  return { shortId: id, state: "downloading", bytesDownloaded: 0 } as unknown as DownloadItem;
}
function tienDo(n: number): ProgressSnapshot {
  return { bytesDownloaded: n, bytesTotal: 1000, speedBps: 1, etaSec: 1 } as ProgressSnapshot;
}

describe("gom tiến độ", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("5 mục × 4 lần/giây trong 5 giây chỉ vẽ ~1 lần/giây, số cuối đúng", () => {
    const ids = ["a", "b", "c", "d", "e"];
    useQueueStore.getState().setAll([...ids.map(muc), muc("khong-tai")]);
    const yen = useQueueStore.getState().items.find((i) => i.shortId === "khong-tai");
    let lanVe = 0;
    const huy = useQueueStore.subscribe(() => lanVe++);
    let n = 0;
    for (let t = 0; t < 20; t++) {
      for (const id of ids) useQueueStore.getState().applyProgress(id, tienDo(++n));
      vi.advanceTimersByTime(250);
    }
    vi.advanceTimersByTime(2000);
    huy();
    expect(lanVe).toBeGreaterThanOrEqual(5);
    expect(lanVe).toBeLessThanOrEqual(7);
    const items = useQueueStore.getState().items;
    expect(items.find((i) => i.shortId === "e")?.bytesDownloaded).toBe(n);
    expect(items.find((i) => i.shortId === "khong-tai")).toBe(yen);
  });
});
