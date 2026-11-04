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
from bqbot.app_client import AppError  # noqa: E402
from bqbot.monitor import APP_MO_LAI_CHO, LICH_TAI_NHO, Monitor  # noqa: E402


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


# ───────────── Vòng duyệt cdb2d8e: app tắt bất thường, chống báo trùng, nút ↑ theo tên ─────────────

class AppChetTests(NoNetworkCase):
    # Lịch tải BẬT → bot tự mở lại (N2: Lịch tắt thì chỉ báo, xem AppChetLichTatTests).
    def _mon(self, song=False, thoat_em=False, lich=True, cap_nhat=None, **kw):
        tg = FakeTG()
        app = FakeApp({"/api/queue": {"ok": True, "items": []},
                       "/api/daychuyen": {"ok": True, "lichTaiEnabled": lich}})
        clock = {"t": 1000.0}
        m = Monitor(tg, app, cfg(**kw), clock=lambda: clock["t"])
        wall = {"t": 1_800_000_000.0}
        m.wall = lambda: wall["t"]
        m.hoi_tasklist = []
        m.app_dang_chay = lambda: m.hoi_tasklist.append(1) or song
        m.da_thoat_em = lambda: thoat_em
        m.dang_cap_nhat = lambda: cap_nhat
        mo = []
        m.mo_app = lambda: mo.append(1)
        m.tick()  # app đang chạy
        app.offline = True
        m.tick()  # vừa mất
        return tg, app, m, clock, wall, mo

    def test_tien_trinh_mat_thi_bao_bat_thuong_va_tu_mo_lai(self):
        tg, app, m, clock, wall, mo = self._mon()
        self.assertEqual(mo, [], "chưa đủ 30 giây")
        clock["t"] += 31
        m.tick()
        self.assertEqual(mo, [1])
        self.assertIn("tắt bất thường", tg.texts()[-1])
        self.assertIn("1/3", tg.texts()[-1])
        clock["t"] += 600
        m.tick()
        self.assertEqual(mo, [1], "1 lần mất chỉ tự mở 1 lần")

    def test_toi_da_3_lan_moi_gio(self):
        tg, app, m, clock, wall, mo = self._mon()
        for _ in range(4):
            clock["t"] += 31
            m.tick()
            app.offline = False
            m.tick()  # app lên
            app.offline = True
            m.tick()  # lại chết
            wall["t"] += 60
        clock["t"] += 31
        m.tick()
        self.assertEqual(len(mo), 3)
        self.assertIn("THÔI tự mở", tg.texts()[-1])

    def test_thoat_em_thi_khong_tu_mo(self):
        tg, app, m, clock, wall, mo = self._mon(thoat_em=True)
        clock["t"] += 400
        m.tick()
        self.assertEqual(mo, [])
        self.assertFalse(any("bất thường" in t for t in tg.texts()))

    def test_con_tien_trinh_ma_cong_im_3_phut_thi_bao_treo_khong_mo_ban_2(self):
        tg, app, m, clock, wall, mo = self._mon(song=True)
        clock["t"] += 120
        m.tick()
        self.assertFalse(any("treo" in t for t in tg.texts()), "chưa đủ 3 phút")
        clock["t"] += 61
        m.tick()
        self.assertEqual(mo, [])
        self.assertIn("treo", tg.texts()[-1])

    def test_mo_lai_loi_thi_bao(self):
        tg, app, m, clock, wall, mo = self._mon()

        def hong():
            raise OSError("không tìm thấy bqhungdown.exe")
        m.mo_app = hong
        clock["t"] += 31
        m.tick()
        self.assertIn("KHÔNG được", tg.texts()[-1])

    def test_dau_thoat_em_doc_dung_file_app_ghi(self):
        with tempfile.TemporaryDirectory() as d:
            self.assertFalse(launcher.da_thoat_em(Path(d)))
            (Path(d) / "thoat_em.json").write_text('{"luc":"x"}', encoding="utf-8")
            self.assertTrue(launcher.da_thoat_em(Path(d)))


class ChongBaoTrungTests(NoNetworkCase):
    def _mon(self, tg=None):
        tg = tg or FakeTG()
        state = {"dc": _dc(dangChay=None)}
        app = FakeApp({"/api/queue": {"ok": True, "items": []}, "/api/daychuyen": lambda _b: state["dc"]})
        m = Monitor(tg, app, cfg(notify_chat_ids=[CHAT, 222]))
        clock = {"t": 1_800_000_000.0}
        m.wall = lambda: clock["t"]
        return tg, m, state, clock

    def _tick(self, m):
        m._last_daychuyen = float("-inf")
        m.tick()

    def test_s4_nhieu_id_khong_bi_rot_theo_chu_cai_roi_bao_lai(self):
        with tempfile.TemporaryDirectory() as d:
            tg, m, state, clock = self._mon()
            m.state_path = Path(d) / "live_msgs.json"
            ids = [f"ket-2026110406{i:04d}" for i in range(60)] + ["bo-sot-x-k1", "tre-20261104-01"]
            state["dc"]["canhBao"] = [{"id": i, "loai": i.split("-")[0], "luc": "", "text": i} for i in ids]
            for _ in range(3):
                self._tick(m)
                clock["t"] += 3601
            n = len(tg.texts())
            # Bot khởi động lại, 3 giờ sau: không báo lại id nào.
            tg2, m2, state2, clock2 = self._mon()
            m2.state_path = m.state_path
            state2["dc"]["canhBao"] = state["dc"]["canhBao"]
            clock2["t"] = clock["t"] + 3 * 3600
            self._tick(m2)
            self.assertEqual(tg2.texts(), [], "không báo lại cảnh báo đã gửi")
            self.assertGreater(n, 0)
            # Id app đã bỏ khỏi danh sách thì sổ cũng bỏ (sổ không phình mãi).
            state2["dc"]["canhBao"] = []
            self._tick(m2)
            self.assertEqual(m2._canh_bao_da_gui, set())

    def test_gui_loi_1_phan_chi_gui_lai_chat_chua_nhan(self):
        class TG1(FakeTG):
            def send_message(self, chat_id, *a, **k):
                if chat_id == 222 and self.hong:
                    raise OSError("chat 222 mất mạng")
                return super().send_message(chat_id, *a, **k)
        tg = TG1()
        tg.hong = True
        tg, m, state, clock = self._mon(tg)
        state["dc"]["vanDe"] = [{"loai": "loi_tat", "text": "Không tạo được lối tắt"}]
        self._tick(m)
        tg.hong = False
        self._tick(m)
        nhan = [c[1] for c in tg.calls if c[0] == "send" and "lối tắt" in c[2]]
        self.assertEqual(sorted(nhan), sorted([222, CHAT]), "mỗi chat nhận ĐÚNG 1 lần")


class LichNutTheoTenTests(LichBoardTests):
    def test_s10_nut_len_theo_ten_bam_lap_khong_hoan_tac(self):
        tg, app, r, state = self._router(_dc(nhom=["MỸ", "HÀN", "NHẬT"]))
        r._cmd_lich(CHAT, "")
        up_nhat = [b["callback_data"] for row in tg.calls[-1][3]["inline_keyboard"] for b in row
                   if b["text"].startswith("↑ NHẬT")][0]

        def settings(b):
            state["dc"] = {**state["dc"], "nhom": b["lichTaiNhom"]}
            return {"ok": True}
        app.routes["/api/settings"] = settings
        self._cb(r, up_nhat)
        self.assertEqual(state["dc"]["nhom"], ["MỸ", "NHẬT", "HÀN"])
        self._cb(r, up_nhat)  # bấm lặp đúng nút đó
        self.assertEqual(state["dc"]["nhom"], ["NHẬT", "MỸ", "HÀN"], "lên tiếp, không đổi ngược lại")
        self._cb(r, up_nhat)
        self.assertEqual(state["dc"]["nhom"], ["NHẬT", "MỸ", "HÀN"], "đã ở đầu thì thôi")
        self.assertIn("đã ở đầu", tg.texts()[-1])

    def test_lich_han_ngoai_khoang_hien_so_da_kep(self):
        tg, app, r, _ = self._router()
        r._cmd_lich(CHAT, "han 99")
        posts = [b for m, p, b in app.requests if p == "/api/settings"]
        self.assertEqual(posts, [{"lichTaiHanChotGio": 24}])
        self.assertIn("<b>24 giờ</b>", tg.texts()[-1])
        self.assertIn("kẹp 99", tg.texts()[-1])
        r._cmd_lich(CHAT, "han 0")
        self.assertIn("<b>1 giờ</b>", tg.texts()[-1])


class AppChetLichTatTests(NoNetworkCase):
    """N1, N2 + việc nhỏ của vòng duyệt 9ffa5a3."""
    _mon = AppChetTests._mon

    def test_n2_lich_tat_chi_bao_khong_tu_mo(self):
        tg, app, m, clock, wall, mo = self._mon(lich=False)
        clock["t"] += 31
        m.tick()
        self.assertEqual(mo, [])
        self.assertIn("tắt bất thường", tg.texts()[-1])
        self.assertIn("▶ Mở app", tg.texts()[-1])

    def test_n2_lich_tat_nhung_bat_tuy_chon_thi_tu_mo(self):
        tg, app, m, clock, wall, mo = self._mon(lich=False, tu_mo_lai_app=True)
        clock["t"] += 31
        m.tick()
        self.assertEqual(mo, [1])

    def test_n1_dang_tu_cap_nhat_thi_khong_mo_ban_cu(self):
        tg, app, m, clock, wall, mo = self._mon(cap_nhat=1_800_000_000.0 + 600)
        clock["t"] += 400
        wall["t"] += 400
        m.tick()
        self.assertEqual(mo, [], "trình cài đang chạy: không mở lại bản cũ")
        self.assertFalse(any("bất thường" in t for t in tg.texts()))
        wall["t"] += 300  # quá hạn 10 phút mà app chưa lên
        m.tick()
        self.assertEqual(mo, [])
        self.assertIn("quá hạn chờ", tg.texts()[-1])
        n = len(tg.texts())
        m.tick()
        self.assertEqual(len(tg.texts()), n, "báo 1 lần")

    def test_gian_tasklist_khi_con_tien_trinh_ma_cong_im(self):
        tg, app, m, clock, wall, mo = self._mon(song=True)
        for _ in range(30):  # 30 nhịp × 5 giây, trong khoảng 30 s – 180 s
            clock["t"] += 5
            m.tick()
        self.assertLessEqual(len(m.hoi_tasklist), 5, "tasklist tối đa 1 lần / 30 giây")

    def test_tu_mo_lai_lau_hon_120_giay_khong_bao_chay_lai_lap(self):
        tg, app, m, clock, wall, mo = self._mon()
        clock["t"] += 31
        m.tick()
        clock["t"] += 300  # app lên chậm
        app.offline = False
        m.tick()
        self.assertFalse(any("đã chạy lại" in t for t in tg.texts()), tg.texts())

    def test_so_da_nhan_khong_phinh(self):
        tg, app, m, clock, wall, mo = self._mon()
        for i in range(80):
            m._notify_ok(f"tin {i}", key=f"k{i}")
            wall["t"] += 1
        self.assertLessEqual(len(m._da_nhan), 50)
        wall["t"] += 4000
        m._notify_ok("x", key="moi")
        self.assertEqual(list(m._da_nhan), [], "khoá cũ > 1 giờ bị bỏ, khoá đã gửi đủ thì xoá")

    def test_loi_doc_tam_giu_lich_tai_biet_cuoi_nen_van_tu_mo(self):
        tg, app, m, clock, wall, mo = self._mon()  # đã đọc được Lịch BẬT
        app.offline = False
        app.routes["/api/daychuyen"] = {"ok": False, "error": "lỗi tạm"}
        app._transport_cu = app._fake

        def loi(method, path, body, timeout):
            if path.startswith("/api/daychuyen"):
                raise AppError("lỗi tạm")
            return app._transport_cu(method, path, body, timeout)
        app._transport = loi
        m._last_daychuyen = float("-inf")
        m.tick()
        self.assertTrue(m._lich_tai, "lỗi tạm không xoá giá trị biết cuối")
        clock["t"] += LICH_TAI_NHO + 1
        m._last_daychuyen = float("-inf")
        m.tick()
        self.assertFalse(m._lich_tai, "quá hạn nhớ thì về TẮT")

    def test_app_chet_ngay_sau_doc_loi_van_tu_mo(self):
        tg, app, m, clock, wall, mo = self._mon()
        clock["t"] += 31
        m.tick()
        self.assertEqual(mo, [1], "Lịch biết cuối là BẬT → vẫn tự mở")

    def test_tu_mo_lai_ma_khong_len_thi_go_co_va_bao(self):
        tg, app, m, clock, wall, mo = self._mon()
        clock["t"] += 31
        m.tick()
        self.assertTrue(m._cho_mo_lai)
        clock["t"] += APP_MO_LAI_CHO + 1
        m.tick()
        self.assertFalse(m._cho_mo_lai)
        self.assertIn("vẫn chưa lên", tg.texts()[-1])

    def test_dau_cap_nhat_doc_han(self):
        with tempfile.TemporaryDirectory() as d:
            self.assertIsNone(launcher.dang_cap_nhat(Path(d)))
            (Path(d) / "dang_cap_nhat.json").write_text('{"het_han":"2026-11-04T06:10:12+07:00"}', encoding="utf-8")
            self.assertEqual(launcher.dang_cap_nhat(Path(d)), fmt.parse_time("2026-11-04T06:10:12+07:00"))


if __name__ == "__main__":
    unittest.main()
