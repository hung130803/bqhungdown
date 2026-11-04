// @vitest-environment jsdom
/**
 * Đợt 1b — nút điều khiển Lịch tải trong Cài đặt (cùng lệnh với nút trên bot):
 * Chạy ngay bấm nhiều lần chỉ 1 lần, Dừng phải hỏi xác nhận (+ tuỳ chọn huỷ job
 * chưa tải), tin trạng thái đủ ý, lưu bị app từ chối (giờ sai) thì hiện lý do.
 */
import { describe, expect, it, vi, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor, act } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import type { Settings } from "@/types/models";
import type { DayChuyenStatus } from "@/ipc/commands";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (k: string) => k }) }));

import { DayChuyenSection, type DayChuyenApi } from "./SettingsPage";

afterEach(cleanup);

const S = { lichTaiEnabled: true, lichTaiGio: "06:00", lichTaiNhom: ["MỸ"], lichTaiHanChotGio: 3, watchGroups: ["MỸ"] } as Settings;

const ST: DayChuyenStatus = {
  lichTaiEnabled: true, gio: "06:00", hanChotGio: 3, nhom: ["MỸ"], dangChay: "20261104-01 (MỸ)", dangDung: false,
  lanToi: "2026-11-05T06:00:00+07:00", lyDoHoan: "06:00 — ổ E đầy", boQuaHomNay: false, dotMo: 1, tre: 2,
  dots: [{ dotId: "20261104-01", nhom: "MỸ", trangThai: "dang_tai", video: 5, xong: 3 }],
  vanDe: [{ loai: "nhom_mat", text: "Nhóm trong Lịch tải không còn: «HÀN»" }],
};

function apiGia(over: Partial<DayChuyenApi> = {}): DayChuyenApi {
  return {
    status: vi.fn(async () => ST),
    chayNgay: vi.fn(async () => "Bắt đầu Lịch tải 1 nhóm: MỸ"),
    dung: vi.fn(async () => "Đã nhận lệnh Dừng"),
    boQua: vi.fn(async () => "Đã bỏ qua Lịch tải hôm nay"),
    ...over,
  };
}

describe("Lịch tải — nút điều khiển (Đợt 1b)", () => {
  it("tin trạng thái: đang làm, lần tới, lý do hoãn, đợt mở, số trễ, lỗi", async () => {
    render(<DayChuyenSection settings={S} onPatch={vi.fn()} api={apiGia()} />);
    const ds = await screen.findByLabelText("Tình trạng Lịch tải");
    for (const want of ["20261104-01 (MỸ)", "06:00 05/11", "ổ E đầy", "MỸ: 3/5 xong", "trễ hôm nay: 2", "«HÀN»"]) {
      expect(ds).toHaveTextContent(want);
    }
  });

  it("Chạy ngay bấm 2 lần liền chỉ gọi 1 lần", async () => {
    let xong!: (v: string) => void;
    const api = apiGia({ chayNgay: vi.fn(() => new Promise<string>(r => { xong = r; })) });
    render(<DayChuyenSection settings={S} onPatch={vi.fn()} api={api} />);
    const nut = screen.getByText("▶ Chạy ngay");
    fireEvent.click(nut);
    fireEvent.click(nut);
    expect(api.chayNgay).toHaveBeenCalledTimes(1);
    await act(async () => xong("Bắt đầu Lịch tải 1 nhóm: MỸ"));
    expect(await screen.findByText(/Bắt đầu Lịch tải/)).toBeInTheDocument();
  });

  it("Dừng hỏi xác nhận, có tuỳ chọn huỷ job chưa tải", async () => {
    const api = apiGia();
    render(<DayChuyenSection settings={S} onPatch={vi.fn()} api={api} />);
    fireEvent.click(screen.getByText("⏹ Dừng lượt…"));
    expect(api.dung).not.toHaveBeenCalled();
    fireEvent.click(screen.getByLabelText(/Huỷ luôn job/));
    fireEvent.click(screen.getByText("Xác nhận dừng"));
    await waitFor(() => expect(api.dung).toHaveBeenCalledWith(true));
    expect(await screen.findByText("Đã nhận lệnh Dừng")).toBeInTheDocument();
  });

  it("lỗi của app (đang chạy rồi…) hiện rõ, không nuốt", async () => {
    const api = apiGia({ boQua: vi.fn(async () => { throw new Error("Lịch tải đang chạy rồi"); }) });
    render(<DayChuyenSection settings={S} onPatch={vi.fn()} api={api} />);
    fireEvent.click(screen.getByText("⏭ Bỏ qua hôm nay"));
    expect(await screen.findByText(/Không làm được: .*đang chạy rồi/)).toBeInTheDocument();
  });

  it("lưu bị từ chối (giờ sai / chưa có nhóm) hiện lý do", async () => {
    const onPatch = vi.fn(() => Promise.reject(new Error("Chưa chọn nhóm nào cho Lịch tải")));
    render(<DayChuyenSection settings={{ ...S, lichTaiEnabled: false, lichTaiNhom: [] }} onPatch={onPatch} />);
    expect(screen.getByText(/chưa bật được Lịch tải/)).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText(/Bật Lịch tải/));
    expect(await screen.findByText(/Không lưu được: .*Chưa chọn nhóm/)).toBeInTheDocument();
  });

  it("giờ gõ sai định dạng thì báo, không gửi", () => {
    const onPatch = vi.fn();
    render(<DayChuyenSection settings={S} onPatch={onPatch} />);
    const o = screen.getByLabelText("Giờ bắt đầu Lịch tải");
    fireEvent.change(o, { target: { value: "" } });
    fireEvent.blur(o);
    expect(onPatch).not.toHaveBeenCalled();
    expect(screen.getByText(/sai định dạng/)).toBeInTheDocument();
  });
});
