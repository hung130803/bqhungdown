import { describe, expect, it } from "vitest";
import { planAddMore, pickKeyForMore, planCheckNow, takenToday, type MoreKey } from "./bulk-more";

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
/** Hàng chờ 🎯 trống — mặc định cho các test không quan tâm hàng chờ. */
const noQueue = () => false;

describe("planAddMore — kênh nào được +1", () => {
  it("kênh CHƯA TẢI hôm nay VẪN chạy (đúng câu hỏi của anh Hùng)", () => {
    // "Chờ lượt" = chưa có dripDate hôm nay. Nó PHẢI được +1 như kênh khác,
    // nếu không thì mấy kênh chưa tới lượt sẽ bị kẹt mãi.
    const choLuot = ch("ChoLuot", "Mỹ", [key({ dripDate: null })]);
    const daTai = ch("DaTai", "Mỹ", [key({ dripDate: TODAY, dripCount: 1 })]);
    const p = planAddMore([choLuot, daTai], "Mỹ", nobodyBusy, noQueue);
    expect(names(p.run)).toEqual(["ChoLuot", "DaTai"]);
  });

  it("chỉ chạy NHÓM đang chọn — không đụng nhóm khác", () => {
    const p = planAddMore(
      [ch("A", "Mỹ"), ch("B", "Hàn"), ch("C", "Mỹ"), ch("D", "Mỹ mới")],
      "Mỹ",
      nobodyBusy,
      noQueue,
    );
    expect(names(p.run)).toEqual(["A", "C"]);
  });

  it("groupFilter = null → chạy MỌI nhóm", () => {
    const p = planAddMore([ch("A", "Mỹ"), ch("B", "Hàn")], null, nobodyBusy, noQueue);
    expect(names(p.run)).toEqual(["A", "B"]);
  });

  it("groupFilter = '' → đúng nhóm 'Chưa phân nhóm', không lấy nhóm có tên", () => {
    const p = planAddMore([ch("KhongNhom", ""), ch("A", "Mỹ")], "", nobodyBusy, noQueue);
    expect(names(p.run)).toEqual(["KhongNhom"]);
  });

  it("kênh CHƯA TÍCH ✓ không chạy", () => {
    const off = ch("Off", "Mỹ", [key({ enabled: false })]);
    const p = planAddMore([off, ch("On", "Mỹ")], "Mỹ", nobodyBusy, noQueue);
    expect(names(p.run)).toEqual(["On"]);
    expect(names(p.off)).toEqual(["Off"]);
  });

  it("kênh có ÍT NHẤT 1 key đang tích ✓ thì vẫn chạy", () => {
    const mix = ch("Mix", "Mỹ", [key({ id: "a", enabled: false }),
                                 key({ id: "b", enabled: true })]);
    expect(names(planAddMore([mix], "Mỹ", nobodyBusy, noQueue).run)).toEqual(["Mix"]);
  });

  it("kênh ĐANG TẢI bị bỏ qua (khỏi thành 2 video song song 1 kênh)", () => {
    const a = ch("Busy", "Mỹ"), b = ch("Ranh", "Mỹ");
    const p = planAddMore([a, b], "Mỹ", (k) => k.name === "Busy", noQueue);
    expect(names(p.run)).toEqual(["Ranh"]);
    expect(names(p.busy)).toEqual(["Busy"]);
  });

  it("kênh HẾT KHO (và hàng chờ trống) bị bỏ qua", () => {
    const dry = ch("Dry", "Mỹ", [key({ sourceEmpty: true })]);
    const p = planAddMore([dry, ch("Ok", "Mỹ")], "Mỹ", nobodyBusy, noQueue);
    expect(names(p.run)).toEqual(["Ok"]);
    expect(names(p.dry)).toEqual(["Dry"]);
  });

  it("kênh đỏ HẾT KHO nhưng CÒN HÀNG CHỜ 🎯 thì VẪN CHẠY — bug anh Hùng 28/07", () => {
    // Anh tích 1 video vào hàng chờ của kênh đang đỏ "hết kho", bấm chạy hàng
    // loạt → bị bỏ qua im lặng. force_one_more rót hàng chờ TRƯỚC, không cần
    // kho, nên kênh này phải vào 'run'.
    const doConHang = ch("DoConHang", "Mỹ", [key({ sourceEmpty: true })]);
    const doTrong = ch("DoTrong", "Mỹ", [key({ id: "x", sourceEmpty: true })]);
    const p = planAddMore(
      [doConHang, doTrong], "Mỹ", nobodyBusy,
      (k) => k === doConHang,          // chỉ DoConHang còn hàng chờ
    );
    expect(names(p.run)).toEqual(["DoConHang"]);
    expect(names(p.dry)).toEqual(["DoTrong"]);
  });

  it("kênh đỏ còn hàng chờ nhưng ĐANG TẢI → vẫn bỏ qua vì busy (busy xét trước)", () => {
    const k1 = ch("DoBusy", "Mỹ", [key({ sourceEmpty: true })]);
    const p = planAddMore([k1], "Mỹ", () => true, () => true);
    expect(names(p.busy)).toEqual(["DoBusy"]);
    expect(p.run).toEqual([]);
  });

  it("MỘT key hết kho là cả kênh bị coi hết kho (an toàn hơn là cố chạy)", () => {
    const half = ch("Half", "Mỹ", [key({ id: "a", sourceEmpty: false }),
                                   key({ id: "b", sourceEmpty: true })]);
    expect(names(planAddMore([half], "Mỹ", nobodyBusy, noQueue).dry)).toEqual(["Half"]);
  });

  it("KHÔNG đếm trùng: mỗi kênh vào đúng 1 nhóm, tổng luôn khớp", () => {
    // Kênh vừa tắt, vừa đang tải, vừa hết kho -> chỉ vào 'off' (xét trước).
    const all = ch("All", "Mỹ", [key({ enabled: false, sourceEmpty: true })]);
    const busyDry = ch("BusyDry", "Mỹ", [key({ sourceEmpty: true })]);
    const list = [all, busyDry, ch("Ok", "Mỹ"), ch("NgoaiNhom", "Hàn")];
    const p = planAddMore(list, "Mỹ", (k) => k.name === "BusyDry", noQueue);
    expect(names(p.off)).toEqual(["All"]);
    expect(names(p.busy)).toEqual(["BusyDry"]);   // busy xét trước dry
    expect(p.dry).toEqual([]);
    expect(names(p.run)).toEqual(["Ok"]);
    const inScope = list.filter((k) => k.group === "Mỹ").length;
    expect(p.run.length + p.off.length + p.busy.length + p.dry.length)
      .toBe(inScope);
  });

  it("danh sách rỗng / nhóm không có kênh nào → không chạy gì", () => {
    expect(planAddMore([], "Mỹ", nobodyBusy, noQueue).run).toEqual([]);
    expect(planAddMore([ch("A", "Hàn")], "Mỹ", nobodyBusy, noQueue).run).toEqual([]);
  });

  it("nhãn nhóm bị null/undefined vẫn coi là 'Chưa phân nhóm'", () => {
    const weird = { name: "N", group: undefined as unknown as string,
                    keys: [key()] };
    expect(names(planAddMore([weird], "", nobodyBusy, noQueue).run)).toEqual(["N"]);
  });
});

describe("planCheckNow — nút ▶ Chạy nhóm: ai còn suất hôm nay", () => {
  const chL = (name: string, limit: number, keys: MoreKey[]) =>
    ({ name, group: "Mỹ", dailyLimit: limit, keys });
  /** Gọi planCheckNow với accessor hạn mức — giống UI truyền k.rep.dailyLimit. */
  const planCheckNow2 = (
    list: Array<{ name: string; group: string; dailyLimit: number; keys: MoreKey[] }>,
    g: string | null, today: string,
  ) => planCheckNow(list, g, today, (k) => k.dailyLimit);

  it("kênh ĐÃ ĐỦ suất thì ĐỨNG YÊN — đúng thắc mắc của anh Hùng", () => {
    // Giống ảnh nhóm "Mỹ mới": 8 kênh đã tải 1/1, 3 kênh chưa tải.
    const daTai = Array.from({ length: 8 }, (_, i) =>
      chL(`da${i}`, 1, [key({ dripDate: TODAY, dripCount: 1 })]));
    const choLuot = Array.from({ length: 3 }, (_, i) =>
      chL(`cho${i}`, 1, [key()]));
    const p = planCheckNow2([...daTai, ...choLuot], "Mỹ", TODAY);
    expect(p.run.map((x) => x.name)).toEqual(["cho0", "cho1", "cho2"]);
    expect(p.full.length).toBe(8);
    expect(p.slots).toBe(3);        // tối đa 3 video, KHÔNG phải 11
  });

  it("hạn mức 3/ngày, đã tải 1 → còn 2 suất", () => {
    const p = planCheckNow2(
      [chL("a", 3, [key({ dripDate: TODAY, dripCount: 1 })])], "Mỹ", TODAY);
    expect(p.run.length).toBe(1);
    expect(p.slots).toBe(2);
  });

  it("đếm CỘNG mọi key của kênh (2 key mỗi cái 1 video = đủ 2/ngày)", () => {
    const p = planCheckNow2([chL("a", 2, [
      key({ id: "x", dripDate: TODAY, dripCount: 1 }),
      key({ id: "y", dripDate: TODAY, dripCount: 1 }),
    ])], "Mỹ", TODAY);
    expect(p.full.length).toBe(1);
    expect(p.slots).toBe(0);
  });

  it("tải HÔM QUA không tính → hôm nay lại còn đủ suất", () => {
    const p = planCheckNow2(
      [chL("a", 1, [key({ dripDate: "2026-07-24", dripCount: 1 })])],
      "Mỹ", TODAY);
    expect(p.run.length).toBe(1);
    expect(p.slots).toBe(1);
  });

  it("hạn mức kẹp 1..3 giống backend (0 → 1, 99 → 3)", () => {
    expect(planCheckNow2([chL("a", 0, [key()])], "Mỹ", TODAY).slots).toBe(1);
    expect(planCheckNow2([chL("a", 99, [key()])], "Mỹ", TODAY).slots).toBe(3);
  });

  it("đã tải QUÁ hạn mức (dùng ➕ Thêm) → đứng yên, slots không âm", () => {
    const p = planCheckNow2(
      [chL("a", 1, [key({ dripDate: TODAY, dripCount: 5 })])], "Mỹ", TODAY);
    expect(p.full.length).toBe(1);
    expect(p.slots).toBe(0);
  });

  it("kênh chưa tích ✓ không tính vào đâu ngoài 'off'", () => {
    const p = planCheckNow2(
      [chL("off", 1, [key({ enabled: false, dripDate: TODAY, dripCount: 1 })])],
      "Mỹ", TODAY);
    expect(p.off.length).toBe(1);
    expect(p.run.length + p.full.length).toBe(0);
    expect(p.slots).toBe(0);
  });

  it("chỉ tính NHÓM đang lọc", () => {
    const p = planCheckNow2(
      [chL("my", 1, [key()]), { ...chL("han", 1, [key()]), group: "Hàn" }],
      "Mỹ", TODAY);
    expect(p.run.map((x) => x.name)).toEqual(["my"]);
  });
});

describe("takenToday", () => {
  it("cộng dồn mọi key, chỉ tính ngày hôm nay", () => {
    expect(takenToday([
      key({ dripDate: TODAY, dripCount: 2 }),
      key({ dripDate: TODAY, dripCount: 1 }),
      key({ dripDate: "2026-01-01", dripCount: 9 }),
      key({ dripDate: null, dripCount: 3 }),
    ], TODAY)).toBe(3);
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
