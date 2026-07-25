import { describe, expect, it } from "vitest";
import { planAddMore, pickKeyForMore, type MoreKey } from "./bulk-more";

const TODAY = "2026-07-25";

/** Kênh test: `name` chỉ để đọc kết quả cho dễ. */
type K = {
  name: string;
  group: string;
  keys: MoreKey[];
};
const key = (o: Partial<MoreKey> = {}): MoreKey => ({
  id: o.id ?? "k1",
  enabled: o.enabled ?? true,
  sourceEmpty: o.sourceEmpty ?? false,
  dripDate: o.dripDate ?? null,
  dripCount: o.dripCount ?? 0,
});
const ch = (name: string, group: string, keys: MoreKey[] = [key()]): K =>
  ({ name, group, keys });
const names = (a: K[]) => a.map((x) => x.name);
const nobodyBusy = () => false;

describe("planAddMore — kênh nào được +1", () => {
  it("kênh CHƯA TẢI hôm nay VẪN chạy (đúng câu hỏi của anh Hùng)", () => {
    // "Chờ lượt" = chưa có dripDate hôm nay. Nó PHẢI được +1 như kênh khác,
    // nếu không thì mấy kênh chưa tới lượt sẽ bị kẹt mãi.
    const choLuot = ch("ChoLuot", "Mỹ", [key({ dripDate: null })]);
    const daTai = ch("DaTai", "Mỹ", [key({ dripDate: TODAY, dripCount: 1 })]);
    const p = planAddMore([choLuot, daTai], "Mỹ", nobodyBusy);
    expect(names(p.run)).toEqual(["ChoLuot", "DaTai"]);
  });

  it("chỉ chạy NHÓM đang chọn — không đụng nhóm khác", () => {
    const p = planAddMore(
      [ch("A", "Mỹ"), ch("B", "Hàn"), ch("C", "Mỹ"), ch("D", "Mỹ mới")],
      "Mỹ",
      nobodyBusy,
    );
    expect(names(p.run)).toEqual(["A", "C"]);
  });

  it("groupFilter = null → chạy MỌI nhóm", () => {
    const p = planAddMore([ch("A", "Mỹ"), ch("B", "Hàn")], null, nobodyBusy);
    expect(names(p.run)).toEqual(["A", "B"]);
  });

  it("groupFilter = '' → đúng nhóm 'Chưa phân nhóm', không lấy nhóm có tên", () => {
    const p = planAddMore([ch("KhongNhom", ""), ch("A", "Mỹ")], "", nobodyBusy);
    expect(names(p.run)).toEqual(["KhongNhom"]);
  });

  it("kênh CHƯA TÍCH ✓ không chạy", () => {
    const off = ch("Off", "Mỹ", [key({ enabled: false })]);
    const p = planAddMore([off, ch("On", "Mỹ")], "Mỹ", nobodyBusy);
    expect(names(p.run)).toEqual(["On"]);
    expect(names(p.off)).toEqual(["Off"]);
  });

  it("kênh có ÍT NHẤT 1 key đang tích ✓ thì vẫn chạy", () => {
    const mix = ch("Mix", "Mỹ", [key({ id: "a", enabled: false }),
                                 key({ id: "b", enabled: true })]);
    expect(names(planAddMore([mix], "Mỹ", nobodyBusy).run)).toEqual(["Mix"]);
  });

  it("kênh ĐANG TẢI bị bỏ qua (khỏi thành 2 video song song 1 kênh)", () => {
    const a = ch("Busy", "Mỹ"), b = ch("Ranh", "Mỹ");
    const p = planAddMore([a, b], "Mỹ", (k) => k.name === "Busy");
    expect(names(p.run)).toEqual(["Ranh"]);
    expect(names(p.busy)).toEqual(["Busy"]);
  });

  it("kênh HẾT KHO bị bỏ qua", () => {
    const dry = ch("Dry", "Mỹ", [key({ sourceEmpty: true })]);
    const p = planAddMore([dry, ch("Ok", "Mỹ")], "Mỹ", nobodyBusy);
    expect(names(p.run)).toEqual(["Ok"]);
    expect(names(p.dry)).toEqual(["Dry"]);
  });

  it("MỘT key hết kho là cả kênh bị coi hết kho (an toàn hơn là cố chạy)", () => {
    const half = ch("Half", "Mỹ", [key({ id: "a", sourceEmpty: false }),
                                   key({ id: "b", sourceEmpty: true })]);
    expect(names(planAddMore([half], "Mỹ", nobodyBusy).dry)).toEqual(["Half"]);
  });

  it("KHÔNG đếm trùng: mỗi kênh vào đúng 1 nhóm, tổng luôn khớp", () => {
    // Kênh vừa tắt, vừa đang tải, vừa hết kho -> chỉ vào 'off' (xét trước).
    const all = ch("All", "Mỹ", [key({ enabled: false, sourceEmpty: true })]);
    const busyDry = ch("BusyDry", "Mỹ", [key({ sourceEmpty: true })]);
    const list = [all, busyDry, ch("Ok", "Mỹ"), ch("NgoaiNhom", "Hàn")];
    const p = planAddMore(list, "Mỹ", (k) => k.name === "BusyDry");
    expect(names(p.off)).toEqual(["All"]);
    expect(names(p.busy)).toEqual(["BusyDry"]);   // busy xét trước dry
    expect(p.dry).toEqual([]);
    expect(names(p.run)).toEqual(["Ok"]);
    const inScope = list.filter((k) => k.group === "Mỹ").length;
    expect(p.run.length + p.off.length + p.busy.length + p.dry.length)
      .toBe(inScope);
  });

  it("danh sách rỗng / nhóm không có kênh nào → không chạy gì", () => {
    expect(planAddMore([], "Mỹ", nobodyBusy).run).toEqual([]);
    expect(planAddMore([ch("A", "Hàn")], "Mỹ", nobodyBusy).run).toEqual([]);
  });

  it("nhãn nhóm bị null/undefined vẫn coi là 'Chưa phân nhóm'", () => {
    const weird = { name: "N", group: undefined as unknown as string,
                    keys: [key()] };
    expect(names(planAddMore([weird], "", nobodyBusy).run)).toEqual(["N"]);
  });
});

describe("pickKeyForMore — cộng vào đúng key", () => {
  const rep = key({ id: "rep" });

  it("ưu tiên key ĐÃ TỰ TẢI HÔM NAY", () => {
    const ks = [key({ id: "a" }),
                key({ id: "b", dripDate: TODAY, dripCount: 2 })];
    expect(pickKeyForMore(ks, rep, TODAY).id).toBe("b");
  });

  it("BỎ QUA key tải HÔM QUA (khỏi cộng sai vào ngày cũ)", () => {
    const ks = [key({ id: "a" }),
                key({ id: "hom_qua", dripDate: "2026-07-24", dripCount: 5 })];
    expect(pickKeyForMore(ks, rep, TODAY).id).toBe("a");
  });

  it("dripCount = 0 thì không tính là 'đã tải hôm nay'", () => {
    const ks = [key({ id: "a" }),
                key({ id: "b", dripDate: TODAY, dripCount: 0 })];
    expect(pickKeyForMore(ks, rep, TODAY).id).toBe("a");
  });

  it("chưa tải gì → lấy key đang tích ✓ đầu tiên (bỏ key đã tắt)", () => {
    const ks = [key({ id: "tat", enabled: false }), key({ id: "bat" })];
    expect(pickKeyForMore(ks, rep, TODAY).id).toBe("bat");
  });

  it("không key nào tích ✓ → dùng key đại diện", () => {
    const ks = [key({ id: "x", enabled: false })];
    expect(pickKeyForMore(ks, rep, TODAY).id).toBe("rep");
  });

  it("danh sách key rỗng → dùng key đại diện", () => {
    expect(pickKeyForMore([], rep, TODAY).id).toBe("rep");
  });
});
