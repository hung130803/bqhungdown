// @vitest-environment jsdom
/**
 * Mục "Dây chuyền tự động" trong Cài đặt (04/11/2026). Dữ liệu nạp từ GÓI
 * JSON THẬT Rust sinh ra (fixture `settings_cookies_seam.json`) — nếu Rust
 * đổi tên khoá `lichTai*`/`moCungWindows` mà giao diện không theo thì đỏ ở đây.
 */
import { describe, expect, it, vi, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import type { Settings } from "@/types/models";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (k: string) => k }) }));

import { DayChuyenSection } from "./SettingsPage";

const GÓI_RUST = JSON.parse(
  readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../src-tauri/tests/fixtures/settings_cookies_seam.json"),
    "utf8",
  ),
) as Settings;

afterEach(cleanup);

describe("Dây chuyền tự động — Cài đặt", () => {
  it("gói Rust có đủ khoá mới và mọi công tắc mặc định TẮT", () => {
    expect(GÓI_RUST.lichTaiEnabled).toBe(false);
    expect(GÓI_RUST.moCungWindows).toBe(false);
    expect(GÓI_RUST.lichTaiGio).toBe("06:00");
    expect(GÓI_RUST.lichTaiHanChotGio).toBe(3);
    expect(GÓI_RUST.lichTaiNhom).toEqual([]);
    expect("daychuyenDir" in GÓI_RUST).toBe(true);
    const onPatch = vi.fn();
    render(<DayChuyenSection settings={GÓI_RUST} onPatch={onPatch} />);
    const lich = screen.getByLabelText(/Bật Lịch tải/) as HTMLInputElement;
    const win = screen.getByLabelText(/Mở cùng Windows/) as HTMLInputElement;
    expect(lich.checked).toBe(false);
    expect(win.checked).toBe(false);
    expect(onPatch).not.toHaveBeenCalled();
    fireEvent.click(lich);
    expect(onPatch).toHaveBeenCalledWith({ lichTaiEnabled: true });
  });

  it("thêm nhóm và đổi thứ tự gửi đúng danh sách theo thứ tự", () => {
    const onPatch = vi.fn();
    const s = { ...GÓI_RUST, watchGroups: ["MỸ", "HÀN"], lichTaiNhom: ["HÀN", "MỸ"] };
    render(<DayChuyenSection settings={s} onPatch={onPatch} />);
    fireEvent.click(screen.getByLabelText("Đưa MỸ lên"));
    expect(onPatch).toHaveBeenLastCalledWith({ lichTaiNhom: ["MỸ", "HÀN"] });
    fireEvent.click(screen.getByText("+ Chưa phân nhóm"));
    expect(onPatch).toHaveBeenLastCalledWith({ lichTaiNhom: ["HÀN", "MỸ", ""] });
    fireEvent.click(screen.getByLabelText("Bỏ HÀN"));
    expect(onPatch).toHaveBeenLastCalledWith({ lichTaiNhom: ["MỸ"] });
  });

  it("Lịch tải + nhịp cũ cùng bật thì báo nhịp cũ tạm nghỉ (D4)", () => {
    render(<DayChuyenSection settings={{ ...GÓI_RUST, lichTaiEnabled: true, watchAutoEnabled: true }} onPatch={vi.fn()} />);
    expect(screen.getByRole("note")).toHaveTextContent("TẠM NGHỈ");
  });

  it("nhịp cũ < 30 phút chỉ CẢNH BÁO, không tự đổi giá trị (#25)", () => {
    const onPatch = vi.fn();
    render(<DayChuyenSection settings={{ ...GÓI_RUST, watchAutoEnabled: true, watchIntervalMin: 1 }} onPatch={onPatch} />);
    expect(screen.getByRole("alert")).toHaveTextContent("1 phút");
    expect(onPatch).not.toHaveBeenCalled();
    cleanup();
    render(<DayChuyenSection settings={{ ...GÓI_RUST, watchAutoEnabled: true, watchIntervalMin: 30 }} onPatch={vi.fn()} />);
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
