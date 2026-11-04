"""Test bot phần DÂY CHUYỀN (04/11/2026) — không mạng thật, không cần app chạy.

- #3: `/openapp` không mở bản thứ 2 khi app đã có tiến trình (vừa tự mở cùng
  Windows, cổng điều khiển chưa lên) — tránh hộp thoại khoá 1-phiên.
- #28: Lịch tải bật → bot tải không mở "đợt tải" đoán trộn nhóm.
- D21: cảnh báo của app (ổ đầy → không mở đợt) gửi Telegram ĐÚNG 1 lần.
"""

from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from test_bot import CHAT, FakeApp, FakeTG, NoNetworkCase, cfg  # noqa: E402

from bqbot import fmt, launcher  # noqa: E402
from bqbot.handlers import Router  # noqa: E402
from bqbot.monitor import Monitor  # noqa: E402


class OpenAppTests(NoNetworkCase):
    def test_app_da_co_tien_trinh_thi_khong_mo_them(self):
        with mock.patch("subprocess.Popen") as popen:
            with self.assertRaises(launcher.AppDangChay):
                launcher.open_app("C:/x/bqhungdown.exe", dang_chay=lambda: True)
            popen.assert_not_called()
            launcher.open_app("C:/x/bqhungdown.exe", dang_chay=lambda: False)
            popen.assert_called_once()

    def test_doc_tasklist_chi_doc_khong_dung_file_khoa(self):
        out = '"bqhungdown.exe","1234","Console","1","120,000 K"\r\n'
        with mock.patch("os.name", "nt"), \
                mock.patch("subprocess.run", return_value=subprocess.CompletedProcess([], 0, out, "")) as run:
            self.assertTrue(launcher.app_dang_chay())
            self.assertIn("tasklist", run.call_args[0][0])
        none = "INFO: No tasks are running which match the specified criteria.\r\n"
        with mock.patch("os.name", "nt"), \
                mock.patch("subprocess.run", return_value=subprocess.CompletedProcess([], 0, none, "")):
            self.assertFalse(launcher.app_dang_chay())
        with mock.patch("os.name", "nt"), mock.patch("subprocess.run", side_effect=OSError("x")):
            self.assertFalse(launcher.app_dang_chay(), "lỗi đọc thì coi như chưa chạy (không chặn mở app)")

    def test_openapp_khi_app_dang_khoi_dong_thi_cho_khong_bat_hop_thoai(self):
        tg = FakeTG()
        app = FakeApp({"/api/ping": {"ok": True, "version": "0.5.4"}})
        app.offline = True
        r = Router(tg, app, cfg())
        sleeps = []

        def sleep(s):
            sleeps.append(s)
            app.offline = False  # cổng điều khiển lên sau ít giây

        with mock.patch("bqbot.launcher.find_app", return_value="C:/x/bqhungdown.exe"), \
                mock.patch("bqbot.launcher.app_dang_chay", return_value=True), \
                mock.patch("subprocess.Popen") as popen:
            r._cb_openapp(CHAT, 9, [], sleep=sleep)  # không có dispatcher → chạy đồng bộ
        popen.assert_not_called()
        self.assertTrue(any("đang khởi động" in t for t in tg.texts()), tg.texts())
        self.assertIn("Đã mở app", tg.texts()[-1])


def _items(*states):
    return [{"shortId": f"s{i}", "state": st, "title": f"V{i}"} for i, st in enumerate(states)]


class MonitorDayChuyenTests(NoNetworkCase):
    def _mon(self, lich_tai: bool, canh_bao=None, items=None):
        tg = FakeTG()
        state = {"items": items if items is not None else []}
        app = FakeApp({
            "/api/queue": lambda _b: {"ok": True, "items": state["items"]},
            "/api/daychuyen": {"ok": True, "lichTaiEnabled": lich_tai, "canhBao": canh_bao or []},
        })
        m = Monitor(tg, app, cfg())
        return tg, app, m, state

    def test_lich_tai_bat_thi_khong_mo_dot_tai_cu(self):
        tg, app, m, state = self._mon(True)
        m.tick()  # nhịp đầu ghi mốc
        state["items"] = _items("queued", "downloading")
        m.tick()
        self.assertEqual(m._batch_ids, [], "Lịch tải bật: không gom đợt trộn nhóm")
        self.assertFalse(any("đợt" in t.lower() for t in tg.texts()), tg.texts())

    def test_lich_tai_tat_hoac_app_cu_thi_dot_tai_nhu_truoc(self):
        tg = FakeTG()
        state = {"items": []}
        app = FakeApp({"/api/queue": lambda _b: {"ok": True, "items": state["items"]}})  # app cũ: không có route
        m = Monitor(tg, app, cfg())
        m.tick()
        state["items"] = _items("queued")
        m.tick()
        self.assertEqual(m._batch_ids, ["s0"], "không có Lịch tải → vẫn gom đợt như cũ")

    def test_canh_bao_o_day_gui_dung_1_lan_ke_ca_bot_khoi_dong_lai(self):
        cb = [{"id": "o-dia-20261104", "luc": "2026-11-04T06:00:00+07:00",
               "text": "Không mở đợt tải: ổ D:\\ chỉ còn 12.0 GB (cần ≥ 30 GB)"}]
        with tempfile.TemporaryDirectory() as d:
            tg, app, m, _ = self._mon(True, cb)
            m.state_path = Path(d) / "live_msgs.json"
            m.tick()
            m._last_daychuyen = float("-inf")
            m.tick()
            sent = [t for t in tg.texts() if "Dây chuyền" in t]
            self.assertEqual(len(sent), 1, tg.texts())
            self.assertIn("30 GB", sent[0])
            tg2, app2, m2, _ = self._mon(True, cb)
            m2.state_path = Path(d) / "live_msgs.json"
            m2.tick()
            self.assertFalse(any("Dây chuyền" in t for t in tg2.texts()), "khởi động lại không gửi lại")


# ───────────── Đợt 1b: bảng Lịch tải + nút + báo lỗi theo loại ─────────────

def _dc(**kw):
    base = {"ok": True, "lichTaiEnabled": True, "gio": "06:00", "hanChotGio": 3, "nhom": ["MỸ", "HÀN", ""],
            "nhomDaQuet": ["MỸ"], "dangChay": "20261104-02 (HÀN)", "dangDung": False,
            "lanToi": "2026-11-05T06:00:00+07:00", "lyDoHoan": None, "dots": [], "tre": 0,
            "canhBao": [], "vanDe": []}
    base.update(kw)
    return base


class LichBoardTests(NoNetworkCase):
    def _router(self, dc=None, actions=None):
        tg = FakeTG()
        state = {"dc": dc or _dc(), "chay": 0}

        def chay(_b):
            state["chay"] += 1
            if state["chay"] > 1:  # app: compare_exchange → lượt 2 bị từ chối
                return {"ok": False, "error": "Lịch tải đang chạy rồi (20261104-01 (MỸ)) — không chạy thêm lượt thứ 2."}
            return {"ok": True, "message": "Bắt đầu Lịch tải 2 nhóm: HÀN → Chưa phân nhóm"}

        routes = {"/api/daychuyen": lambda _b: state["dc"], "/api/daychuyen/chay": chay}
        routes.update(actions or {})
        app = FakeApp(routes)
        return tg, app, Router(tg, app, cfg()), state

    def _cb(self, r, data):
        mid = r.lich_boards.get(CHAT)["mid"]
        t = r.route_callback({"id": "c", "data": data, "message": {"chat": {"id": CHAT}, "message_id": mid}})
        t.fn()
        return mid

    def _btn(self, tg, prefix):
        markup = [c for c in tg.calls if c[0] in ("send", "edit")][-1][3]
        for row in markup["inline_keyboard"]:
            for b in row:
                if b["callback_data"].split(":")[2:3] == [prefix]:
                    return b["callback_data"]
        raise AssertionError(f"không có nút {prefix}: {markup}")

    def test_bang_trang_thai_du_thong_tin(self):
        tg, app, r, _ = self._router(_dc(lyDoHoan="06:00 — ổ D đầy", tre=2,
                                         dots=[{"dotId": "20261104-01", "nhom": "MỸ", "trangThai": "dang_tai",
                                                "xong": 3, "video": 5}],
                                         vanDe=[{"loai": "nhom_mat", "text": "Nhóm trong Lịch tải không còn: «X»"}]))
        r._cmd_lich(CHAT, "")
        t = tg.texts()[-1]
        for want in ("BẬT", "06:00", "3 giờ", "✅MỸ", "HÀN", "Chưa phân nhóm", "Đang làm: <b>20261104-02 (HÀN)",
                     "Lần chạy tới: 06:00", "ổ D đầy", "20261104-01 (MỸ: 3/5 xong)", "trễ hôm nay: 2", "«X»"):
            self.assertIn(want, t)

    def test_chay_ngay_bam_2_lan_chi_1_luot(self):
        tg, app, r, state = self._router()
        r._cmd_lich(CHAT, "")
        self._cb(r, self._btn(tg, "chay"))
        self.assertIn("Bắt đầu Lịch tải", tg.texts()[-1])
        self._cb(r, self._btn(tg, "chay"))
        self.assertIn("đang chạy rồi", tg.texts()[-1])
        self.assertEqual(state["chay"], 2, "bot gửi đúng lệnh, app quyết định chỉ 1 lượt")

    def test_nut_tren_bang_cu_het_han_thi_bao_khong_lam_gi(self):
        tg, app, r, state = self._router()
        r._cmd_lich(CHAT, "")
        cu = self._btn(tg, "chay")
        old_mid = r.lich_boards.get(CHAT)["mid"]
        r._cmd_lich(CHAT, "")  # bảng mới → mã mới
        t = r.route_callback({"id": "c", "data": cu, "message": {"chat": {"id": CHAT}, "message_id": old_mid}})
        t.fn()
        self.assertIn("hết hạn", tg.texts()[-1])
        self.assertEqual(state["chay"], 0, "nút hết hạn KHÔNG chạy")

    def test_dung_hoi_xac_nhan_roi_moi_dung_va_tuy_chon_huy_job(self):
        sent = []
        tg, app, r, _ = self._router(actions={"/api/daychuyen/dung": lambda b: sent.append(b) or
                                              {"ok": True, "message": "Đã nhận lệnh Dừng"}})
        r._cmd_lich(CHAT, "")
        self._cb(r, self._btn(tg, "dung?"))
        self.assertIn("Dừng lượt Lịch tải?", tg.texts()[-1])
        self.assertEqual(sent, [], "chưa xác nhận thì chưa dừng")
        markup = tg.calls[-1][3]
        huy = [b["callback_data"] for row in markup["inline_keyboard"] for b in row
               if b["callback_data"].endswith("dung:1")][0]
        self._cb(r, huy)
        self.assertEqual(sent, [{"huyJob": True}])
        self.assertIn("Đã nhận lệnh Dừng", tg.texts()[-1])

    def test_chinh_gio_han_thu_tu_nhom_qua_nut_va_lenh(self):
        tg, app, r, state = self._router(_dc(gio="00:00"))
        r._cmd_lich(CHAT, "")
        self._cb(r, self._btn(tg, "gio").replace("gio:30", "gio:-30"))
        self._cb(r, self._btn(tg, "han").replace("han:-1", "han:1"))
        self._cb(r, self._btn(tg, "up"))  # ↑ HÀN
        posts = [b for m, p, b in app.requests if p == "/api/settings"]
        self.assertEqual(posts, [{"lichTaiGio": "23:30"}, {"lichTaiHanChotGio": 4},
                                 {"lichTaiNhom": ["HÀN", "MỸ", ""]}])
        r._cmd_lich(CHAT, "nhom MỸ, -, HÀN")
        posts = [b for m, p, b in app.requests if p == "/api/settings"]
        self.assertEqual(posts[-1], {"lichTaiNhom": ["MỸ", "", "HÀN"]})

    def test_gio_sai_app_tu_choi_thi_bao_ro(self):
        tg, app, r, _ = self._router(actions={"/api/settings": lambda b: {
            "ok": False, "error": "Giờ bắt đầu Lịch tải «6h» sai định dạng — nhập dạng HH:MM"}})
        r._cmd_lich(CHAT, "gio 6h")
        self.assertIn("sai định dạng", tg.texts()[-1])
        self.assertIn("🗓 Lịch tải", tg.texts()[-1], "vẫn hiện bảng để sửa tiếp")


class DayChuyenAlertTests(NoNetworkCase):
    def _mon(self, tg=None):
        tg = tg or FakeTG()
        state = {"dc": _dc(dangChay=None)}
        app = FakeApp({"/api/queue": {"ok": True, "items": []}, "/api/daychuyen": lambda _b: state["dc"]})
        m = Monitor(tg, app, cfg())
        clock = {"t": 1_800_000_000.0}
        m.wall = lambda: clock["t"]
        return tg, m, state, clock

    def _tick(self, m):
        m._last_daychuyen = float("-inf")
        m.tick()

    def test_loi_theo_loai_1_lan_gio_va_bao_on_lai(self):
        tg, m, state, clock = self._mon()
        state["dc"]["vanDe"] = [{"loai": "khong_chay", "text": "Lịch tải không chạy được: ổ D đầy"}]
        self._tick(m)
        self._tick(m)
        self.assertEqual(len([t for t in tg.texts() if "ổ D đầy" in t]), 1, "lỗi kéo dài không gửi lặp")
        state["dc"]["vanDe"] = [{"loai": "khong_chay", "text": "Lịch tải không chạy được: ổ E đầy"}]
        clock["t"] += 600
        self._tick(m)
        self.assertFalse(any("ổ E đầy" in t for t in tg.texts()), "cùng loại trong 1 giờ: chưa gửi")
        clock["t"] += 3600
        self._tick(m)
        self.assertTrue(any("ổ E đầy" in t for t in tg.texts()), "qua 1 giờ: gửi nội dung mới")
        state["dc"]["vanDe"] = []
        self._tick(m)
        self.assertIn("đã ổn lại", tg.texts()[-1])
        n = len(tg.texts())
        self._tick(m)
        self.assertEqual(len(tg.texts()), n, "báo ổn lại 1 lần")

    def test_gui_loi_that_bai_thi_khong_danh_dau_nhip_sau_gui_lai(self):
        class TGHong(FakeTG):
            hong = True

            def send_message(self, *a, **k):
                if self.hong:
                    raise OSError("mất mạng")
                return super().send_message(*a, **k)

        tg, m, state, clock = self._mon(TGHong())
        state["dc"]["vanDe"] = [{"loai": "loi_tat", "text": "Không tạo được lối tắt Mở cùng Windows: x"}]
        state["dc"]["canhBao"] = [{"id": "tre-20261104-01", "loai": "tre", "luc": "",
                                   "text": "Đợt 20261104-01 còn 3 video trễ"}]
        self._tick(m)
        self.assertEqual(m._dc_active, {})
        tg.hong = False
        self._tick(m)
        self.assertTrue(any("lối tắt" in t for t in tg.texts()))
        self.assertTrue(any("3 video trễ" in t for t in tg.texts()))

    def test_su_kien_cung_loai_gop_1_tin_gio(self):
        tg, m, state, clock = self._mon()
        state["dc"]["canhBao"] = [{"id": "a", "loai": "o_day_giua", "luc": "", "text": "Ổ đầy giữa chừng A"}]
        self._tick(m)
        state["dc"]["canhBao"].append({"id": "b", "loai": "o_day_giua", "luc": "", "text": "Ổ đầy giữa chừng B"})
        state["dc"]["canhBao"].append({"id": "c", "loai": "o_day_giua", "luc": "", "text": "Ổ đầy giữa chừng C"})
        self._tick(m)
        self.assertFalse(any("giữa chừng B" in t for t in tg.texts()), "trong 1 giờ: giữ lại")
        clock["t"] += 3601
        self._tick(m)
        gop = [t for t in tg.texts() if "giữa chừng B" in t]
        self.assertEqual(len(gop), 1)
        self.assertIn("giữa chừng C", gop[0], "gộp 1 tin")

    def test_vong_lich_im_lau_thi_bao_ket(self):
        tg, m, state, clock = self._mon()
        state["dc"]["nhipCuoi"] = "2026-01-15T08:00:00+07:00"
        clock["t"] = fmt.parse_time("2026-01-15T08:20:00+07:00")
        self._tick(m)
        self.assertTrue(any("kẹt" in t for t in tg.texts()), tg.texts())

    def test_bang_lich_tu_sua_khi_doi_va_khong_de_man_xac_nhan(self):
        tg, m, state, clock = self._mon()
        m.lich_boards.set(CHAT, 77, "abc", "cũ")
        self._tick(m)
        self.assertEqual(len([c for c in tg.calls if c[0] == "edit" and c[4] == 77]), 1)
        self._tick(m)
        self.assertEqual(len([c for c in tg.calls if c[0] == "edit"]), 1, "không đổi thì không sửa")
        m.lich_boards.set(CHAT, 77, "abc", "xác nhận", pause=120)
        state["dc"]["dangChay"] = "20261104-03 (X)"
        self._tick(m)
        self.assertEqual(len([c for c in tg.calls if c[0] == "edit"]), 1, "đang hỏi xác nhận: không đè")


if __name__ == "__main__":
    unittest.main()
