/** 1b-D1: đổi tên / xoá nhóm ở trang Theo dõi → danh sách nhóm của Lịch tải theo kèm. */
import { describe, expect, it } from "vitest";
import { lichSauDoiNhom } from "./watch-group";

describe("lichSauDoiNhom", () => {
  it("đổi tên giữ đúng thứ tự", () => {
    expect(lichSauDoiNhom(["MỸ", "HÀN", ""], "HÀN", "Hàn Quốc")).toEqual(["MỸ", "Hàn Quốc", ""]);
  });
  it("xoá nhóm thì bỏ khỏi Lịch", () => {
    expect(lichSauDoiNhom(["MỸ", "HÀN"], "MỸ", null)).toEqual(["HÀN"]);
  });
  it("đổi sang tên đã có trong Lịch thì không lặp", () => {
    expect(lichSauDoiNhom(["MỸ", "HÀN"], "HÀN", "MỸ")).toEqual(["MỸ"]);
  });
  it("Lịch không có nhóm đó → không cần lưu", () => {
    expect(lichSauDoiNhom(["MỸ"], "HÀN", "X")).toBeNull();
    expect(lichSauDoiNhom(undefined, "HÀN", null)).toBeNull();
  });
});
