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

from bqbot import launcher  # noqa: E402
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


if __name__ == "__main__":
    unittest.main()
