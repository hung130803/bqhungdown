"""Bộ kiểm tra cookie Firefox: phân loại kết quả yt-dlp + báo khi đổi trạng thái.
Không chạy yt-dlp thật, không gửi Telegram thật."""

import subprocess
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from bqbot import cookie_check as cc, fmt  # noqa: E402
from bqbot.monitor import Monitor  # noqa: E402

from test_bot import FakeApp, FakeTG, cfg  # noqa: E402


def fake_run(code, out="", err=""):
    def run(cmd, **_kw):
        assert "--cookies-from-browser" in cmd and "firefox" in cmd
        return subprocess.CompletedProcess(cmd, code, out, err)
    return run


class ClassifyTests(unittest.TestCase):
    def test_ok(self):
        r = cc.run_check("yt.exe", fake_run(0, "mye1epXXk8c|18\n"))
        self.assertTrue(r.ok)

    def test_logged_out_age_wall(self):
        r = cc.run_check("yt.exe", fake_run(1, "", "WARNING: [youtube] The provided YouTube account cookies "
                                                  "are no longer valid.\nERROR: [youtube] x: Sign in to confirm your age."))
        self.assertEqual(r.status, cc.LOGGED_OUT)

    def test_no_firefox(self):
        r = cc.run_check("yt.exe", fake_run(1, "", "ERROR: could not find firefox cookies database in C:\\x"))
        self.assertEqual(r.status, cc.NO_FIREFOX)

    def test_other_error_is_not_called_logged_out(self):
        r = cc.run_check("yt.exe", fake_run(1, "", "ERROR: Unable to download webpage: timed out"))
        self.assertEqual(r.status, cc.ERROR)

    def test_timeout(self):
        def run(cmd, **_kw):
            raise subprocess.TimeoutExpired(cmd, 1)
        self.assertEqual(cc.run_check("yt.exe", run).status, cc.ERROR)

    def test_checker_single_flight(self):
        ch = cc.CookieChecker(check=lambda: cc.CheckResult(cc.OK, 1.0))
        ch._busy.acquire()
        self.assertIsNone(ch.check_now(), "đang có lượt khác → không chạy chồng")
        ch._busy.release()
        self.assertTrue(ch.check_now().ok)
        self.assertTrue(ch.latest().ok)


class ScreenTests(unittest.TestCase):
    DATA = {"sites": [{"site": "youtube", "label": "YouTube", "browser": "firefox"},
                      {"site": "douyin", "label": "Douyin", "file": "x.txt", "exists": False}]}

    def test_screen_states(self):
        self.assertIn("Chưa kiểm tra", fmt.cookies_text(self.DATA))
        ok = fmt.cookies_text(self.DATA, check=cc.CheckResult(cc.OK, 0))
        self.assertIn("🟢", ok)
        self.assertIn("không cần làm gì", ok)
        bad = fmt.cookies_text(self.DATA, check=cc.CheckResult(cc.LOGGED_OUT, 0, "Firefox đã bị đăng xuất"))
        self.assertIn("🔴", bad)
        self.assertIn("đăng xuất", bad)
        self.assertIn("đã bị xoá", bad)


class MonitorAlertTests(unittest.TestCase):
    def _mon(self, results):
        tg = FakeTG()
        app = FakeApp({"/api/cookies": {"ok": True, "sites": [{"site": "youtube", "browser": "firefox"}]}})
        m = Monitor(tg, app, cfg())
        it = iter(results)
        m.cookie_checker = cc.CookieChecker(check=lambda: next(it))
        return tg, m

    def test_alert_on_break_then_recover_no_spam(self):
        bad = cc.CheckResult(cc.LOGGED_OUT, 0, "đăng xuất")
        tg, m = self._mon([cc.CheckResult(cc.OK, 0), bad, bad, cc.CheckResult(cc.OK, 0)])
        m.firefox_check()
        self.assertEqual(tg.texts(), [], "đang ổn → im lặng")
        m.firefox_check()
        self.assertIn("đang hỏng", tg.texts()[-1])
        m.firefox_check()
        self.assertEqual(len(tg.texts()), 1, "vẫn hỏng → không nhắc lại trong 24 giờ")
        m.firefox_check()
        self.assertIn("hoạt động lại", tg.texts()[-1])

    def test_skip_when_slot_not_firefox(self):
        tg = FakeTG()
        app = FakeApp({"/api/cookies": {"ok": True, "sites": [{"site": "youtube", "file": "c.txt"}]}})
        m = Monitor(tg, app, cfg())
        m.cookie_checker = cc.CookieChecker(check=lambda: self.fail("không được chạy"))
        m.firefox_check()


if __name__ == "__main__":
    unittest.main()
