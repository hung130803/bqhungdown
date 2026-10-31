"""Báo cáo ngày + phân loại trạng thái kênh: mỗi lỗi báo ĐÚNG loại.
Câu lỗi dùng trong test là câu app THẬT báo (31/10/2026)."""

import sys
import unittest
from datetime import date
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from bqbot import cookie_check as cc, fmt, report  # noqa: E402

TODAY = date(2026, 10, 31)
T = "2026-10-31"

DEAD = ("❌ Kênh KHÔNG CÒN trên YouTube (YouTube báo 404: kênh đã bị xoá, bị khoá hoặc đổi tên @). "
        "Mở link kênh trên trình duyệt để kiểm tra — mất thật thì tắt theo dõi kênh này.")
EMPTY = "📭 Kênh còn nhưng HẾT VIDEO ở mục đang theo dõi (chủ kênh đã xoá/ẩn hết, hoặc kênh chỉ đăng loại khác)."
OLD_GENERIC = ("⚠️ Tải thất bại. Cách sửa nhanh: mở Cài đặt → bấm \"Sửa lỗi tải ngay\" → bấm Thử lại.\n"
               "(Chi tiết kỹ thuật: yt-dlp thất bại: Không tìm thấy video trên kênh (kênh trống, sai link, "
               "hoặc tab không có video).)")


def ch(cid, group="MỸ", **kw):
    base = {"id": cid, "title": cid, "group": group, "enabled": True, "dailyLimit": 1,
            "destDir": f"D:\\v\\{cid}", "dripDate": T, "dripCount": 0, "lastChecked": "2026-10-31T03:00:00Z"}
    base.update(kw)
    return base


class StatusTests(unittest.TestCase):
    def test_each_error_has_its_own_label(self):
        self.assertEqual(fmt.channel_status(ch("a", lastError=DEAD), T)[2], "dead")
        self.assertEqual(fmt.channel_status(ch("a", lastError=EMPTY), T)[2], "empty")
        self.assertEqual(fmt.channel_status(ch("a", lastError="🤖 YouTube đang chặn tạm"), T)[2], "blocked")
        self.assertEqual(fmt.channel_status(ch("a", lastError="📡 Lỗi mạng — kiểm tra"), T)[2], "net")
        self.assertEqual(fmt.channel_status(ch("a", lastError="🔞 Firefox chưa đăng nhập"), T)[2], "cookie")
        self.assertEqual(fmt.channel_status(ch("a", lastError=OLD_GENERIC), T)[2], "err",
                         "câu cũ mơ hồ → 'lỗi khác', không đoán bừa")

    def test_no_error_states(self):
        self.assertEqual(fmt.channel_status(ch("a", dripCount=1), T)[2], "full")
        self.assertEqual(fmt.channel_status(ch("a", downloading=1), T)[2], "dl")
        self.assertEqual(fmt.channel_status(ch("a", sourceEmpty=True), T)[2], "empty")
        self.assertEqual(fmt.channel_status(ch("a"), T)[2], "wait")
        self.assertEqual(fmt.channel_status(ch("a", enabled=False, lastError=DEAD), T)[2], "off")
        self.assertEqual(fmt.channel_status(ch("a", dripDate="2026-10-30", dripCount=1), T)[2], "wait",
                         "số của ngày cũ không tính là đủ hôm nay")
        st = fmt.channel_status(ch("a", lastChecked="2026-09-22T04:36:48Z"), T)
        self.assertEqual(st[2], "notrun", "quét lần cuối 22/09 → KHÔNG phải 'chờ lượt'")
        self.assertIn("22/09", st[1])
        self.assertEqual(fmt.channel_status(ch("a", lastChecked=None), T)[2], "notrun")

    def test_button_and_detail_show_reason(self):
        self.assertIn("kênh không còn", fmt.channel_button(ch("a", lastError=DEAD)))
        d = fmt.channel_detail(ch("a", lastError=EMPTY))
        self.assertIn("hết video", d)
        self.assertNotIn("Sửa lỗi tải ngay", d)


class DailyReportTests(unittest.TestCase):
    def test_report_counts_and_reasons(self):
        chans = [ch("k1", dripCount=1), ch("k2"), ch("k3", lastError=DEAD), ch("k4", lastError=EMPTY),
                 ch("j1", group="Nhật", dripCount=1), ch("off", enabled=False)]
        hist = [{"finishedAt": "2026-10-31T03:00:00Z", "saveFolder": "D:\\v\\k1", "title": "A"},
                {"finishedAt": "2026-10-31T04:00:00Z", "saveFolder": "D:\\v\\j1", "title": "B"},
                {"finishedAt": "2026-10-29T04:00:00Z", "saveFolder": "D:\\v\\k1", "title": "cũ"}]
        queue = [{"state": "failed", "finishedAt": "2026-10-31T05:00:00Z", "title": "Video lỗi",
                  "error": "📡 Lỗi mạng — kiểm tra Internet"},
                 {"state": "skipped", "finishedAt": "2026-10-31T05:00:00Z", "title": "Live"}]
        t = report.daily_report(chans, hist, queue, cc.CheckResult(cc.OK, 0), today=TODAY)
        self.assertIn("BÁO CÁO NGÀY 31/10", t)
        self.assertIn("Đã tải hôm nay: <b>2</b>", t, "chỉ tính video xong HÔM NAY")
        self.assertIn("Lỗi <b>1</b>", t)
        self.assertIn("Bỏ qua 1", t)
        self.assertIn("📁 <b>MỸ</b>", t)
        self.assertIn("📁 <b>Nhật</b>", t)
        self.assertIn("❌ <b>kênh không còn</b> (1): k3", t)
        self.assertIn("📭 <b>hết video</b> (1): k4", t)
        self.assertIn("Video lỗi", t)
        self.assertIn("🟢", t)
        self.assertLess(len(t), 4096)

    def test_group_not_run_today_says_so(self):
        chans = [ch("j1", group="Nhật", lastChecked="2026-09-22T04:36:48Z")]
        t = report.daily_report(chans, [], [], today=TODAY, auto_scan=False)
        self.assertIn("💤 1 chưa quét hôm nay", t)
        self.assertIn("lần quét gần nhất 22/09", t)
        self.assertIn("Tự quét kênh đang <b>TẮT</b>", t)

    def test_report_all_good(self):
        t = report.daily_report([ch("k1", dripCount=1)], [], [], today=TODAY)
        self.assertIn("Không có kênh nào lỗi", t)

    def test_report_stays_under_telegram_limit(self):
        chans = [ch(f"kenh-rat-dai-ten-{i:03d}", group=f"Nhóm {i % 9}", lastError=DEAD) for i in range(300)]
        self.assertLessEqual(len(report.daily_report(chans, [], [], today=TODAY)), 4096)


class AutoSendTests(unittest.TestCase):
    def test_sent_once_per_day_even_after_restart(self):
        import tempfile
        import threading
        import time as _t
        from test_bot import FakeApp, FakeTG, cfg
        from bqbot.monitor import Monitor

        def wait_threads():
            for th in threading.enumerate():
                if th.name == "bao-cao-ngay":
                    th.join(5)

        routes = {"/api/queue": {"ok": True, "items": []}, "/api/cookies": {"ok": True, "sites": []},
                  "/api/watch": {"ok": True, "items": [ch("k1", dripCount=1, dripDate=_t.strftime("%Y-%m-%d"))]},
                  "/api/history": {"ok": True, "items": []},
                  "/api/status": {"ok": True, "watch": {"groups": ["MỸ"]}}}
        with tempfile.TemporaryDirectory() as d:
            tg = FakeTG()
            m = Monitor(tg, FakeApp(routes), cfg(daily_report_hour=0))
            m.state_path = Path(d) / "live.json"
            m.tick()
            wait_threads()
            m.tick()
            wait_threads()
            reports = [t for t in tg.texts() if "BÁO CÁO NGÀY" in t]
            self.assertEqual(len(reports), 1, "1 lần/ngày")
            m2 = Monitor(tg, FakeApp(routes), cfg(daily_report_hour=0))
            m2.state_path = m.state_path
            m2.tick()
            wait_threads()
            self.assertEqual(len([t for t in tg.texts() if "BÁO CÁO NGÀY" in t]), 1, "bot bật lại không gửi trùng")


class ReviewRegressionTests(unittest.TestCase):
    """Các lỗi do rà soát độc lập 31/10/2026 tìm ra — giữ để không tái phát."""

    def test_nano_seconds_timestamp_parses(self):
        self.assertEqual(fmt.local_day("2026-10-31T02:03:50.617210400Z")[:7], "2026-10")
        self.assertIsNone(fmt.local_day("rác"))

    def test_expired_cookie_with_bot_wall_still_alerts(self):
        from test_bot import FakeApp, FakeTG, cfg
        from bqbot.monitor import Monitor
        tg = FakeTG()
        m = Monitor(tg, FakeApp({"/api/cookies": {"ok": True, "sites": []}}), cfg())
        m._maybe_cookie_alert({"extractor": "youtube", "error": "🍪 Bộ tải báo phiên cookie YouTube…\n(Chi tiết "
                               "kỹ thuật: YouTube cookies are no longer valid. ERROR: Sign in to confirm "
                               "you're not a bot)"})
        self.assertEqual(len(tg.texts()), 1)

    def test_bot_wall_is_not_logged_out(self):
        import subprocess
        r = cc.run_check("yt.exe", lambda cmd, **k: subprocess.CompletedProcess(
            cmd, 1, "", "ERROR: [youtube] x: Sign in to confirm you're not a bot"))
        self.assertEqual(r.status, cc.ERROR)

    def test_wait_result_reuses_running_check(self):
        import threading
        import time as _t
        calls = []
        gate = threading.Event()

        def slow():
            calls.append(1)
            gate.wait(2)
            return cc.CheckResult(cc.OK, 1.0)
        ch = cc.CookieChecker(check=slow)
        th = threading.Thread(target=ch.check_now)
        th.start()
        _t.sleep(0.05)
        threading.Timer(0.1, gate.set).start()
        self.assertTrue(ch.wait_result(timeout=3).ok)
        th.join()
        self.assertEqual(len(calls), 1, "không chạy chồng lượt thứ 2")

    def test_removed_items_are_not_counted_cancelled(self):
        from test_bot import FakeApp, FakeTG, cfg
        from bqbot.monitor import Monitor
        items = [{"shortId": "a", "state": "queued", "title": "A"}]
        tg = FakeTG()
        m = Monitor(tg, FakeApp({"/api/queue": {"ok": True, "items": items},
                                 "/api/cookies": {"ok": True, "sites": []},
                                 "/api/watch": {"ok": True, "items": []}}), cfg())
        m.tick()
        items.append({"shortId": "b", "state": "queued", "title": "B"})
        m.tick()
        items[0]["state"] = "completed"
        del items[1]  # người dùng xoá mục B khỏi hàng đợi
        m.tick()
        report_txt = tg.calls[-1][2]
        self.assertIn("ĐÃ XONG ĐỢT TẢI", report_txt)
        self.assertNotIn("Huỷ", report_txt)

    def test_old_saved_batch_is_dropped(self):
        import json
        import tempfile
        import time as _t
        from test_bot import FakeApp, FakeTG, cfg
        from bqbot.monitor import Monitor
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "live.json"
            p.write_text(json.dumps({"batch_ids": ["a"], "batch": [[1, 5]], "batch_t0": _t.time() - 86400}))
            m = Monitor(FakeTG(), FakeApp(), cfg())
            m.state_path = p
            self.assertEqual(m.restore_live_messages(), 0)

    def test_batch_report_fits_telegram(self):
        items = [{"shortId": str(i), "state": "failed", "title": "x" * 80, "error": "y" * 200,
                  "saveFolder": f"D:\\g{i}"} for i in range(400)]
        chans = [{"id": str(i), "group": f"Nhóm {i}", "destDir": f"D:\\g{i}", "enabled": True} for i in range(400)]
        self.assertLessEqual(len(fmt.batch_report(items, chans, "10:00", None)), 4096)


class KeyboardTests(unittest.TestCase):
    def test_report_button_on_keyboard_and_menu(self):
        from bqbot.handlers import KEYBOARD_COMMANDS, main_menu, reply_keyboard
        self.assertEqual(KEYBOARD_COMMANDS["📋 Báo cáo ngày"], "report")
        labels = [b["text"] for row in reply_keyboard()["keyboard"] for b in row]
        self.assertEqual(labels, list(KEYBOARD_COMMANDS), "mọi nút đều hiện")
        datas = [b["callback_data"] for row in main_menu()["inline_keyboard"] for b in row]
        self.assertIn("rp", datas)


if __name__ == "__main__":
    unittest.main()
