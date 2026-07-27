import { describe, expect, it } from "vitest";

import { chuanUrl, loiTrung, nhomMacDinh, timTrungUrl } from "./watch-group";

const K = (name: string, group: string, ...urls: string[]) => ({
  name,
  group,
  keys: urls.map((url) => ({ url })),
});

describe("chuanUrl — phải GIỐNG HỆT Rust add_watched_channel", () => {
  it("bỏ dấu / cuối và hạ chữ thường", () => {
    expect(chuanUrl("https://YouTube.com/@Abc/")).toBe("https://youtube.com/@abc");
  });
  it("bỏ nhiều dấu / cuối", () => {
    expect(chuanUrl("https://x.com/@a///")).toBe("https://x.com/@a");
  });
  it("cắt khoảng trắng hai đầu", () => {
    expect(chuanUrl("  https://x.com/@a  ")).toBe("https://x.com/@a");
  });
  it("rỗng vẫn ra rỗng, không nổ", () => {
    expect(chuanUrl("")).toBe("");
    expect(chuanUrl(undefined as unknown as string)).toBe("");
  });
  it("hai cách viết cùng 1 kênh phải BẰNG NHAU", () => {
    expect(chuanUrl("https://www.youtube.com/@Kenh1/")).toBe(
      chuanUrl("https://www.youtube.com/@kenh1"),
    );
  });
});

describe("timTrungUrl — chặn thêm trùng TRƯỚC khi ghi đè", () => {
  const all = [
    K("Kênh A", "Mỹ", "https://youtube.com/@a"),
    K("Kênh B", "Mỹ mới", "https://youtube.com/@b", "https://youtube.com/@b2"),
    K("", "", "https://youtube.com/@c"),
  ];

  it("tìm đúng kênh khi trùng key ĐẦU", () => {
    expect(timTrungUrl(all, "https://youtube.com/@a")?.name).toBe("Kênh A");
  });
  it("tìm đúng kênh khi trùng key THỨ HAI (kênh nhiều key nguồn)", () => {
    expect(timTrungUrl(all, "https://youtube.com/@b2")?.name).toBe("Kênh B");
  });
  it("KHÁC hoa/thường và dấu / cuối vẫn coi là TRÙNG", () => {
    expect(timTrungUrl(all, "https://YouTube.com/@A/")?.name).toBe("Kênh A");
  });
  it("link mới thì trả null", () => {
    expect(timTrungUrl(all, "https://youtube.com/@moi")).toBeNull();
  });
  it("link rỗng trả null, không nổ", () => {
    expect(timTrungUrl(all, "")).toBeNull();
    expect(timTrungUrl(all, "   ")).toBeNull();
  });
  it("kênh không có key nào cũng không làm nổ", () => {
    const l = [{ name: "X", group: "", keys: [] }];
    expect(timTrungUrl(l, "https://youtube.com/@a")).toBeNull();
  });
});

describe("loiTrung — câu báo phải nói RÕ kênh nào, nhóm nào", () => {
  it("có tên kênh và tên nhóm", () => {
    const s = loiTrung(K("Kênh A", "Mỹ", "u"));
    expect(s).toContain("Kênh A");
    expect(s).toContain("Mỹ");
    expect(s).toContain("ĐÃ CÓ");
  });
  it("kênh chưa đặt tên / chưa có nhóm vẫn báo dễ hiểu", () => {
    const s = loiTrung(K("", "", "u"));
    expect(s).toContain("(chưa đặt tên)");
    expect(s).toContain("Chưa phân nhóm");
  });
});

describe("nhomMacDinh — mở ➕ Thêm kênh phải theo NHÓM ĐANG XEM", () => {
  it("đang xem nhóm Mỹ -> mặc định Mỹ (đây chính là lỗi đã sửa)", () => {
    expect(nhomMacDinh("Mỹ")).toBe("Mỹ");
  });
  it("đang xem tất cả nhóm (null) -> để trống", () => {
    expect(nhomMacDinh(null)).toBe("");
  });
  it("đang xem 'Chưa phân nhóm' ('') -> để trống", () => {
    expect(nhomMacDinh("")).toBe("");
  });
  it("cắt khoảng trắng", () => {
    expect(nhomMacDinh("  Nhật  ")).toBe("Nhật");
  });
});
