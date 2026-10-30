"""Test bot — KHÔNG gửi tin Telegram thật, KHÔNG cần app đang chạy.

Chạy:  python -m unittest discover -s tests -v   (trong thư mục telegram-bot)
"""

from __future__ import annotations

import os
import re
import socket
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(HERE))

import requests  # noqa: E402
import urllib3.util.connection as urllib3_connection  # noqa: E402

from bqbot import config as config_mod  # noqa: E402
from bqbot import fmt  # noqa: E402
from bqbot.app_client import AppClient, AppOffline, TTLCache  # noqa: E402
from bqbot.config import Config  # noqa: E402
from bqbot.dispatcher import Dispatcher, Task  # noqa: E402
from bqbot.handlers import BOT_COMMANDS, COOKIE_SITES, Router  # noqa: E402
from bqbot.monitor import Monitor  # noqa: E402
from bqbot.net import SEND_GAP_SEC, TelegramAPI  # noqa: E402

CHAT = 111


def _no_network(*_a, **_k):
    raise AssertionError("Test KHÔNG được gọi mạng thật")


class FakeTG:
    """Thay TelegramAPI ở tầng cao: ghi lại mọi lời gọi."""

    def __init__(self):
        self.calls: list[tuple] = []
        self.answered_at: float | None = None
        self.lock = threading.Lock()
        self._mid = 1000
        self.files: dict[str, bytes] = {}

    def send_message(self, chat_id, text, reply_markup=None, **_kw):
        with self.lock:
            self._mid += 1
            self.calls.append(("send", chat_id, text, reply_markup))
            return {"message_id": self._mid}

    def edit_message(self, chat_id, mid, text, reply_markup=None):
        with self.lock:
            self.calls.append(("edit", chat_id, text, reply_markup, mid))

    def answer_callback(self, cid, text="", alert=False):
        with self.lock:
            self.answered_at = time.perf_counter()
            self.calls.append(("answer", cid, text))

    def download_file(self, file_id, max_bytes=0):
        return self.files[file_id]

    def texts(self) -> list[str]:
        return [c[2] for c in self.calls if c[0] in ("send", "edit")]


class FakeApp(AppClient):
    """AppClient với transport giả. Đường dẫn khớp ĐÚNG (bỏ query) — bot gọi
    sai route thì lộ ra ngay thay vì được 'khớp tiền tố' che đi."""

    def __init__(self, routes: dict | None = None, delay: float = 0.0):
        self.routes = routes or {}
        self.requests: list[tuple] = []
        self.delay = delay
        self.offline = False
        super().__init__(base_url="http://x", token="t", transport=self._fake)

    def _fake(self, method, path, body, timeout):
        if self.offline:
            raise AppOffline("tắt")
        if self.delay:
            time.sleep(self.delay)
        self.requests.append((method, path, body))
        bare = path.split("?", 1)[0]
        if bare in self.routes:
            resp = self.routes[bare]
            data = resp(body) if callable(resp) else resp
            return (200 if data.get("ok", True) else 400), data
        if method == "POST":
            return 200, {"ok": True}
        return 404, {"ok": False, "error": f"không có route giả {bare}"}


def stop_and_wait(d: Dispatcher) -> None:
    """Dừng dispatcher VÀ chờ luồng đang chạy xong — không để luồng rò sang test khác."""
    d.stop.set()
    for ex in (d.pool, d.fast, d.jobs_pool, d.tick_pool):
        ex.shutdown(wait=True, cancel_futures=True)


def cfg(**kw) -> Config:
    base = dict(telegram_token="x", allowed_chat_ids=[CHAT], notify_chat_ids=[CHAT],
                digest_interval_sec=0, board_interval_sec=0)
    base.update(kw)
    return Config(**base)


STATUS = {"ok": True, "version": "0.5.3", "counts": {"downloading": 1, "failed": 2}, "active": [],
          "speedBps": 0, "settings": {"maxConcurrency": 3, "watchAutoEnabled": True, "watchIntervalMin": 30},
          "watch": {"channels": 2, "enabled": 2, "withError": 0, "groups": ["VN", "US"]}}


class NoNetworkCase(unittest.TestCase):
    def setUp(self):
        p = mock.patch.object(requests.Session, "request", _no_network)
        p.start()
        self.addCleanup(p.stop)


# ───────────────────────── tầng mạng ─────────────────────────
class NetTests(NoNetworkCase):
    def test_ipv4_forced(self):
        self.assertEqual(urllib3_connection.allowed_gai_family(), socket.AF_INET)

    def _api(self, replies):
        sleeps: list[float] = []
        clock = [0.0]

        def transport(method, data, files, rt):
            if isinstance(replies.get(method), list):
                return replies[method].pop(0)
            return {"ok": True, "result": {}}

        def sleep(s):
            sleeps.append(s)
            clock[0] += s

        api = TelegramAPI("T", transport=transport, sleep=sleep, clock=lambda: clock[0])
        return api, sleeps

    def test_429_edit_is_dropped_without_waiting(self):
        api, sleeps = self._api({"editMessageText": [{"ok": False, "error_code": 429, "parameters": {"retry_after": 30}}]})
        self.assertIsNone(api.edit_message(1, 2, "x"))
        self.assertEqual(sleeps, [])

    def test_429_answer_and_delete_dropped(self):
        for m in ("answerCallbackQuery", "deleteMessage", "editMessageReplyMarkup"):
            api, sleeps = self._api({m: [{"ok": False, "error_code": 429, "parameters": {"retry_after": 9}}]})
            self.assertIsNone(api.call(m, {}))
            self.assertEqual(sleeps, [])

    def test_429_send_waits_retry_after_then_resends(self):
        api, sleeps = self._api({"sendMessage": [
            {"ok": False, "error_code": 429, "parameters": {"retry_after": 7}},
            {"ok": True, "result": {"message_id": 5}}]})
        self.assertEqual(api.send_message(1, "hi")["message_id"], 5)
        self.assertAlmostEqual(sum(sleeps), 7.0, places=2)

    def test_429_on_send_blocks_all_senders(self):
        api, sleeps = self._api({"sendMessage": [
            {"ok": False, "error_code": 429, "parameters": {"retry_after": 12}},
            {"ok": True, "result": {}}, {"ok": True, "result": {}}]})
        api.send_message(1, "a")
        api.send_message(1, "b")
        self.assertAlmostEqual(sum(sleeps), 12 + SEND_GAP_SEC, places=2)

    def test_send_spacing_only_for_send_methods(self):
        api, sleeps = self._api({})
        api.send_message(1, "a")
        api.send_message(1, "b")
        api.edit_message(1, 1, "c")
        api.answer_callback("q")
        self.assertEqual(len(sleeps), 1)
        self.assertAlmostEqual(sleeps[0], SEND_GAP_SEC, places=2)

    def test_not_modified_is_ok(self):
        api, _ = self._api({"editMessageText": [{"ok": False, "error_code": 400,
                                                  "description": "Bad Request: message is not modified"}]})
        self.assertIsNone(api.edit_message(1, 1, "x"))

    def test_token_never_leaks_in_errors(self):
        def transport(method, data, files, rt):
            raise requests.ConnectionError("Max retries exceeded with url: /botSECRET123:ABC/getUpdates")

        api = TelegramAPI("SECRET123:ABC", transport=transport, sleep=lambda s: None)
        with self.assertRaises(Exception) as cm:
            api.call("getUpdates", {})
        self.assertNotIn("SECRET123:ABC", str(cm.exception))
        self.assertIn("***", str(cm.exception))

    def test_read_timeout_on_send_is_not_resent(self):
        calls = []

        def transport(method, data, files, rt):
            calls.append(method)
            raise requests.ReadTimeout("read timed out")

        api = TelegramAPI("T", transport=transport, sleep=lambda s: None)
        with self.assertRaises(Exception):
            api.send_message(1, "x")
        self.assertEqual(len(calls), 1, "có thể Telegram đã nhận — gửi lại là trùng tin")

    def test_connection_error_on_send_is_retried(self):
        calls = []

        def transport(method, data, files, rt):
            calls.append(method)
            if len(calls) == 1:
                raise requests.ConnectionError("refused")
            return {"ok": True, "result": {"message_id": 1}}

        api = TelegramAPI("T", transport=transport, sleep=lambda s: None)
        self.assertEqual(api.send_message(1, "x")["message_id"], 1)
        self.assertEqual(len(calls), 2)

    def test_log_filter_redacts_token(self):
        sys.path.insert(0, str(HERE))
        import logging
        import bot
        rec = logging.LogRecord("x", logging.WARNING, __file__, 1, "url /bot%s/getMe", ("TOK:EN",), None)
        bot.RedactToken("TOK:EN").filter(rec)
        self.assertNotIn("TOK:EN", rec.getMessage())


# ───────────────────────── điều phối ─────────────────────────
class DispatcherTests(NoNetworkCase):
    def test_button_answer_under_one_second_even_when_work_is_slow(self):
        tg = FakeTG()
        started = threading.Event()

        class SlowRouter:
            def route_callback(self, cb):
                return Task(lambda: (started.set(), time.sleep(1.0)), "chậm", heavy=True, key="k")

        d = Dispatcher(tg, SlowRouter())
        try:
            t0 = time.perf_counter()
            d.handle_update({"update_id": 1, "callback_query": {"id": "cb1", "data": "x",
                                                                "message": {"chat": {"id": CHAT}, "message_id": 1}}})
            returned = time.perf_counter() - t0
            deadline = time.perf_counter() + 1.0
            while tg.answered_at is None and time.perf_counter() < deadline:
                time.sleep(0.005)
            self.assertIsNotNone(tg.answered_at, "Nút phải được trả lời trong 1 giây")
            self.assertLess(tg.answered_at - t0, 1.0)
            self.assertLess(returned, 0.1, "Vòng chính không được chờ việc nặng")
            self.assertIn(("answer", "cb1", "⏳ Đang làm..."), tg.calls)
            self.assertTrue(started.wait(1))
        finally:
            stop_and_wait(d)

    def test_real_router_buttons_respond_fast(self):
        """Mọi nút thật của bot: bấm → answerCallbackQuery < 1 giây, kể cả khi
        app phản hồi chậm 1,5 giây."""
        tg = FakeTG()
        app = FakeApp({"/api/status": STATUS, "/api/queue": {"ok": True, "items": [], "total": 0},
                       "/api/watch": {"ok": True, "items": []}, "/api/cookies": {"ok": True, "sites": []},
                       "/api/history": {"ok": True, "items": []}}, delay=1.5)
        router = Router(tg, app, cfg())
        d = Dispatcher(tg, router)
        try:
            for i, data in enumerate(("st", "q:failed:0", "w:a:0", "ck", "s", "h", "b:retry_failed")):
                tg.answered_at = None
                t0 = time.perf_counter()
                d.handle_update({"update_id": 1, "callback_query": {
                    "id": data, "data": data, "message": {"chat": {"id": CHAT + i}, "message_id": 9}}})
                deadline = time.perf_counter() + 1.0
                while tg.answered_at is None and time.perf_counter() < deadline:
                    time.sleep(0.005)
                self.assertIsNotNone(tg.answered_at, data)
                self.assertLess(tg.answered_at - t0, 1.0, data)
        finally:
            stop_and_wait(d)

    def test_same_key_runs_one_at_a_time(self):
        d = Dispatcher(FakeTG(), None)
        active = [0]
        peak = [0]
        lk = threading.Lock()

        def work():
            with lk:
                active[0] += 1
                peak[0] = max(peak[0], active[0])
            time.sleep(0.05)
            with lk:
                active[0] -= 1

        try:
            futs = [d.run_task(Task(work, "w", heavy=True, key="item:1")) for _ in range(4)]
            for f in futs:
                f.result(2)
            self.assertEqual(peak[0], 1)
        finally:
            stop_and_wait(d)

    def test_waiting_on_busy_key_does_not_hold_workers(self):
        d = Dispatcher(FakeTG(), None, workers=3)
        gate = threading.Event()
        other = threading.Event()
        try:
            futs = [d.run_task(Task(gate.wait, "chờ", heavy=True, key="chat:1")) for _ in range(5)]
            d.run_task(Task(other.set, "khác", heavy=True, key="chat:2"))
            self.assertTrue(other.wait(1), "việc của key khác phải chạy được ngay")
            gate.set()
            for f in futs:
                f.result(2)
            self.assertEqual(d._keyed, {}, "hết việc thì key phải được dọn")
        finally:
            stop_and_wait(d)

    def test_long_jobs_do_not_block_heavy_pool_or_tick(self):
        d = Dispatcher(FakeTG(), None, workers=3)
        gate = threading.Event()
        ran = threading.Event()
        ticked = threading.Event()
        try:
            for _ in range(3):
                d.submit_job("gửi file", gate.wait, lambda r, e: None)
            d.run_task(Task(ran.set, "lệnh", heavy=True))
            d._start_tick(ticked.set)
            self.assertTrue(ran.wait(1))
            self.assertTrue(ticked.wait(1))
        finally:
            gate.set()
            stop_and_wait(d)

    def test_pool_is_capped_at_three(self):
        d = Dispatcher(FakeTG(), None, workers=3)
        try:
            self.assertEqual(d.pool._max_workers, 3)
        finally:
            stop_and_wait(d)

    def test_light_task_runs_off_main_loop(self):
        """Lệnh nhẹ chạy ngay ở luồng nhanh — vòng chính KHÔNG tự gửi tin (429
        chờ 30 s cũng không làm nút khác quay vòng)."""
        d = Dispatcher(FakeTG(), None)
        ran = threading.Event()
        try:
            t0 = time.perf_counter()
            fut = d.run_task(Task(lambda: (time.sleep(0.3), ran.set()), "nhẹ"))
            self.assertLess(time.perf_counter() - t0, 0.05)
            fut.result(2)
            self.assertTrue(ran.is_set())
        finally:
            stop_and_wait(d)

    def test_event_wakes_loop_immediately(self):
        tg = FakeTG()
        handled = threading.Event()

        class R:
            def route_message(self, msg):
                return Task(handled.set, "m")

        d = Dispatcher(tg, R())
        t = threading.Thread(target=d.run, kwargs={"tick": lambda: None, "tick_every": 3600}, daemon=True)
        t.start()
        try:
            time.sleep(0.05)
            t0 = time.perf_counter()
            d.updates.put({"update_id": 1, "message": {"chat": {"id": CHAT}, "text": "x"}})
            d.wake.set()
            self.assertTrue(handled.wait(1))
            self.assertLess(time.perf_counter() - t0, 0.2)
        finally:
            stop_and_wait(d)
            d.wake.set()
            t.join(1)

    def test_long_job_is_fire_and_forget(self):
        d = Dispatcher(FakeTG(), None)
        got = []
        try:
            t0 = time.perf_counter()
            d.submit_job("dài", lambda: (time.sleep(0.2), 42)[1], lambda r, e: got.append((r, e)))
            self.assertLess(time.perf_counter() - t0, 0.05)
            deadline = time.perf_counter() + 2
            while not got and time.perf_counter() < deadline:
                d.wake.wait(0.5)
                d.wake.clear()
                d.collect_jobs()
                time.sleep(0.01)
            self.assertEqual(got, [(42, None)])
        finally:
            stop_and_wait(d)

    def test_shutdown_with_pending_jobs_is_clean(self):
        d = Dispatcher(FakeTG(), None)
        gate = threading.Event()
        for _ in range(5):
            d.submit_job("dài", gate.wait, lambda r, e: None)
        d.shutdown()
        gate.set()
        time.sleep(0.05)
        d.collect_jobs()  # không được ném CancelledError
        self.assertEqual(d.drain(), 0)
        d.handle_update({"update_id": 1, "message": {"chat": {"id": 1}, "text": "x"}})  # không ném RuntimeError

    def test_slow_command_is_logged(self):
        clock = iter([0.0, 3.5])
        d = Dispatcher(FakeTG(), None, clock=lambda: next(clock))
        try:
            with self.assertLogs("bqbot.dispatch", level="WARNING") as cm:
                d.run_task(Task(lambda: None, "/status")).result(2)
            self.assertTrue(any("Lệnh chậm" in m and "/status" in m for m in cm.output))
        finally:
            stop_and_wait(d)


# ───────────────────────── router ─────────────────────────
class RouterTests(NoNetworkCase):
    def run_task(self, task):
        self.assertIsNotNone(task)
        task.fn()

    def test_unknown_chat_is_denied_with_its_id(self):
        tg = FakeTG()
        r = Router(tg, FakeApp(), cfg())
        self.run_task(r.route_message({"chat": {"id": 999}, "text": "/status"}))
        self.assertIn("999", tg.texts()[-1])
        self.assertIsNone(r.route_callback({"id": "c", "data": "st", "message": {"chat": {"id": 999}, "message_id": 1}}))

    def test_links_are_enqueued(self):
        tg = FakeTG()
        app = FakeApp({"/api/enqueue": lambda b: {"ok": True, "added": len(b["urls"]), "folder": "D:\\x",
                                                  "items": [{"shortId": "ab12", "title": "T", "state": "queued"}]}})
        r = Router(tg, app, cfg())
        task = r.route_message({"chat": {"id": CHAT}, "text": "tải https://youtu.be/a và https://youtu.be/a https://youtu.be/b"})
        self.assertTrue(task.heavy)
        self.run_task(task)
        body = [b for m, p, b in app.requests if p == "/api/enqueue"][0]
        self.assertEqual(body["urls"], ["https://youtu.be/a", "https://youtu.be/b"])
        self.assertIn("Đang tải", tg.texts()[-1])
        self.assertIn("Lưu trên máy tính", tg.texts()[-1])

    def test_sent_link_message_updates_itself_until_done(self):
        """Gửi link → 1 tin TỰ CẬP NHẬT tới khi xong, có nút gửi về điện thoại;
        không gửi thêm tin 'tải xong' trùng."""
        tg = FakeTG()
        state = {"s": "queued"}
        app = FakeApp({
            "/api/enqueue": {"ok": True, "added": 1, "folder": "D:\\Video",
                             "items": [{"shortId": "v1", "title": "https://youtu.be/a", "state": "queued"}]},
            "/api/queue": lambda _b: {"ok": True, "items": [
                {"shortId": "v1", "title": "Phim hay", "state": state["s"], "percent": 40.0,
                 "outputPath": "D:\\Video\\Phim hay.mp4"}]},
            "/api/cookies": {"ok": True, "sites": []}, "/api/watch": {"ok": True, "items": []}})
        mon = Monitor(tg, app, cfg())
        r = Router(tg, app, cfg(), mon)
        mon.tick()  # mốc
        self.run_task(r.route_message({"chat": {"id": CHAT}, "text": "https://youtu.be/a"}))
        self.assertIn("đang lấy tên video", tg.texts()[-1], "chưa có tên thì không hiện link thô")
        sent_mid = tg._mid
        state["s"] = "downloading"
        mon.tick()
        self.assertEqual(tg.calls[-1][0], "edit")
        self.assertEqual(tg.calls[-1][4], sent_mid)
        self.assertIn("40%", tg.calls[-1][2])
        state["s"] = "completed"
        mon.tick()
        last = tg.calls[-1]
        self.assertEqual((last[0], last[4]), ("edit", sent_mid))
        self.assertIn("Xong", last[2])
        buttons = [b["callback_data"] for row in last[3]["inline_keyboard"] for b in row]
        self.assertIn("f:v1", buttons)
        self.assertEqual(sum(1 for c in tg.calls if c[0] == "send"), 1, "không gửi thêm tin trùng")
        mon.tick()
        self.assertEqual(tg.calls[-1], last, "xong rồi thì thôi theo dõi")

    def test_more_button_reports_empty_channel(self):
        """Bấm '+1' ở kênh đã hết video: phải BÁO RÕ, không im lặng."""
        tg = FakeTG()
        ch = {"id": "c1", "title": "Bodycam", "enabled": True, "dailyLimit": 1, "sourceEmpty": True}
        app = FakeApp({"/api/watch/c1/more": {"ok": True, "count": 0, "channel": ch},
                       "/api/watch": {"ok": True, "items": [ch]}})
        r = Router(tg, app, cfg())
        self.run_task(r.route_callback({"id": "c", "data": "wm:c1:a:0",
                                        "message": {"chat": {"id": CHAT}, "message_id": 5}}))
        texts = tg.texts()
        self.assertIn("Đang tìm", texts[0], "báo ngay là đang làm")
        self.assertIn("HẾT video chưa tải", texts[-1])

    def test_more_button_reports_added_video(self):
        tg = FakeTG()
        ch = {"id": "c1", "title": "K", "enabled": True, "dailyLimit": 1, "lastPick": "🔥 tự lấy: Video X"}
        app = FakeApp({"/api/watch/c1/more": {"ok": True, "count": 1, "channel": ch},
                       "/api/watch": {"ok": True, "items": [ch]}})
        r = Router(tg, app, cfg())
        self.run_task(r.route_callback({"id": "c", "data": "wm:c1:a:0",
                                        "message": {"chat": {"id": CHAT}, "message_id": 5}}))
        self.assertIn("Đã thêm 1 video", tg.texts()[-1])
        self.assertIn("Video X", tg.texts()[-1])

    def test_item_and_channel_detail_screens(self):
        tg = FakeTG()
        app = FakeApp({
            "/api/queue": {"ok": True, "items": [{"shortId": "q1", "title": "Video A", "state": "failed",
                                                  "error": "⚠️ Tải thất bại. Sửa lỗi tải ngay"}], "total": 1},
            "/api/watch": {"ok": True, "items": [{"id": "c1", "title": "Kênh X", "enabled": True,
                                                  "lastError": "lỗi quét", "dailyLimit": 1}]}})
        r = Router(tg, app, cfg())
        cb = {"id": "c", "message": {"chat": {"id": CHAT}, "message_id": 5}}
        self.run_task(r.route_callback({**cb, "data": "q:failed:0"}))
        labels = [b["text"] for row in tg.calls[-1][3]["inline_keyboard"] for b in row]
        self.assertTrue(any("Video A" in t for t in labels), "nút mang tên video, không phải mã")
        self.run_task(r.route_callback({**cb, "data": "qi:q1:failed:0"}))
        labels = [b["text"] for row in tg.calls[-1][3]["inline_keyboard"] for b in row]
        self.assertIn("🔁 Thử lại", labels)
        self.assertIn("🔧 Sửa lỗi tải ngay", labels)
        self.run_task(r.route_callback({**cb, "data": "wi:c1:a:0"}))
        labels = [b["text"] for row in tg.calls[-1][3]["inline_keyboard"] for b in row]
        self.assertIn("🔄 Quét ngay", labels)
        self.assertIn("⏸ Tắt theo dõi", labels)
        self.assertIn("lỗi quét", tg.calls[-1][2])

    def test_audio_command(self):
        tg = FakeTG()
        app = FakeApp({"/api/enqueue": {"ok": True, "added": 1, "folder": "D", "items": []}})
        r = Router(tg, app, cfg())
        self.run_task(r.route_message({"chat": {"id": CHAT}, "text": "/audio https://youtu.be/a"}))
        self.assertTrue([b for m, p, b in app.requests if p == "/api/enqueue"][0]["audio"])

    def test_queue_item_action_then_rerender(self):
        tg = FakeTG()
        app = FakeApp({"/api/queue/ab12/retry": {"ok": True},
                       "/api/queue": {"ok": True, "items": [], "total": 0}})
        r = Router(tg, app, cfg())
        self.run_task(r.route_callback({"id": "c", "data": "qa:ab12:retry:failed:0",
                                        "message": {"chat": {"id": CHAT}, "message_id": 7}}))
        self.assertIn(("POST", "/api/queue/ab12/retry", {}), app.requests)
        self.assertEqual(tg.calls[-1][0], "edit", "bấm nút phải SỬA tin, không gửi tin mới")

    def test_cookie_upload_with_caption(self):
        tg = FakeTG()
        tg.files = {"F1": b".youtube.com\tTRUE\t/\tTRUE\t2000000000\tSID\tabc\n"}
        app = FakeApp({"/api/cookies/youtube": {"ok": True, "cookies": 1, "expired": 0, "earliestExpiry": 2000000000}})
        r = Router(tg, app, cfg())
        self.run_task(r.route_message({"chat": {"id": CHAT}, "message_id": 5, "caption": "yt",
                                       "document": {"file_id": "F1", "file_name": "cookies.txt"}}))
        self.assertTrue(any(p == "/api/cookies/youtube" for _, p, _ in app.requests))
        self.assertIn("Đã thay cookie", tg.texts()[-1])

    def test_cookie_upload_ambiguous_asks_site(self):
        tg = FakeTG()
        tg.files = {"F": b"# Netscape\n.example.com\tTRUE\t/\tTRUE\t0\ta\tb\n"}
        app = FakeApp({"/api/cookies/tiktok": {"ok": True, "cookies": 1, "expired": 0}})
        r = Router(tg, app, cfg())
        self.run_task(r.route_message({"chat": {"id": CHAT}, "message_id": 8,
                                       "document": {"file_id": "F", "file_name": "cookies.txt"}}))
        self.assertIn("trang nào", tg.texts()[-1])
        self.run_task(r.route_callback({"id": "c", "data": "cs:8:tiktok", "message": {"chat": {"id": CHAT}, "message_id": 9}}))
        self.assertTrue(any(p == "/api/cookies/tiktok" for _, p, _ in app.requests))

    def test_wrong_cookie_site_keeps_file_for_retry(self):
        tg = FakeTG()
        tg.files = {"F": b"# Netscape\n.example.com\tTRUE\t/\tTRUE\t0\ta\tb\n"}
        app = FakeApp({"/api/cookies/tiktok": {"ok": False, "error": "File không có cookie nào của TikTok"},
                       "/api/cookies/youtube": {"ok": True, "cookies": 1}})
        r = Router(tg, app, cfg())
        self.run_task(r.route_message({"chat": {"id": CHAT}, "message_id": 8,
                                       "document": {"file_id": "F", "file_name": "cookies.txt"}}))
        cb = {"id": "c", "message": {"chat": {"id": CHAT}, "message_id": 9}}
        self.run_task(r.route_callback({**cb, "data": "cs:8:tiktok"}))
        self.assertIn("Chọn lại", tg.texts()[-1])
        self.run_task(r.route_callback({**cb, "data": "cs:8:youtube"}))
        self.assertIn("Đã thay cookie", tg.texts()[-1])

    def test_app_offline_is_reported_not_crashing(self):
        tg = FakeTG()
        app = FakeApp()
        app.offline = True
        r = Router(tg, app, cfg())
        self.run_task(r.route_message({"chat": {"id": CHAT}, "text": "/status"}))
        self.assertIn("⚠️", tg.texts()[-1])

    def test_button_on_board_takes_it_over(self):
        tg = FakeTG()
        app = FakeApp({"/api/queue": {"ok": True, "items": [], "total": 0}})
        mon = Monitor(tg, app, cfg())
        r = Router(tg, app, cfg(), mon)
        mon.add_board(CHAT, 77)
        r.route_callback({"id": "c", "data": "q:failed:0", "message": {"chat": {"id": CHAT}, "message_id": 77}})
        self.assertEqual(mon.boards(), {}, "bảng thôi tự cập nhật khi tin đã thành màn hình khác")
        mon.add_board(CHAT, 80)
        self.run_task(r.route_callback({"id": "c", "data": "bx", "message": {"chat": {"id": CHAT}, "message_id": 70}}))
        self.assertEqual(mon.boards(), {CHAT: 80}, "nút dừng trên bảng CŨ không tắt bảng mới")

    def test_big_file_is_compressed_then_sent(self):
        tg = FakeTG()
        sent = []
        tg.send_file = lambda chat, path, cap="", as_video=False: sent.append((path, as_video))
        tg.delete_message = lambda *a: None
        with tempfile.TemporaryDirectory() as d:
            big = Path(d) / "phim.mp4"
            big.write_bytes(b"x" * (60 * 1024 * 1024))
            app = FakeApp({"/api/queue": {"ok": True, "items": [
                {"shortId": "v1", "state": "completed", "title": "Phim", "outputPath": str(big)}]},
                "/api/status": STATUS})
            r = Router(tg, app, cfg())
            small = Path(d) / "nho.mp4"
            small.write_bytes(b"y")
            with mock.patch("bqbot.compress.find_ffmpeg", return_value="ffmpeg"), \
                    mock.patch("bqbot.compress.compress", return_value=str(small)) as comp, \
                    mock.patch("bqbot.compress.cleanup") as clean:
                self.run_task(r.route_callback({"id": "c", "data": "f:v1",
                                                "message": {"chat": {"id": CHAT}, "message_id": 3}}))
            comp.assert_called_once()
            clean.assert_called_once_with(str(small))
            self.assertEqual(sent, [(str(small), True)], "gửi BẢN NÉN, không phải file gốc 60 MB")
            self.assertTrue(big.exists(), "file gốc trên máy tính giữ nguyên")

    def test_history_shows_only_completed(self):
        tg = FakeTG()
        app = FakeApp({"/api/history": {"ok": True, "items": [
            {"shortId": "ok1", "title": "Xong", "status": "completed"},
            {"shortId": "bad1", "title": "Hong", "status": "failed"}]}})
        r = Router(tg, app, cfg())
        self.run_task(r.route_message({"chat": {"id": CHAT}, "text": "/history"}))
        self.assertIn("Xong", tg.texts()[-1])
        self.assertNotIn("Hong", tg.texts()[-1])
        buttons = [b["callback_data"] for row in tg.calls[-1][3]["inline_keyboard"] for b in row]
        self.assertIn("f:ok1", buttons)
        self.assertNotIn("f:bad1", buttons)

    def test_callback_data_fits_telegram_limit(self):
        tg = FakeTG()
        items = [{"shortId": "abcdefgh", "state": s, "title": "t"} for s in
                 ("downloading", "queued", "paused", "failed", "skipped", "completed", "cancelled")]
        markup = Router(tg, FakeApp(), cfg())._queue_markup("failed", 3, items * 10, 70)
        for row in markup["inline_keyboard"]:
            for b in row:
                self.assertLessEqual(len(b["callback_data"].encode()), 64)

    def test_every_command_has_handler(self):
        r = Router(FakeTG(), FakeApp(), cfg())
        for name, _ in BOT_COMMANDS:
            self.assertIn(name, r.commands)


# ───────────────────────── theo dõi ─────────────────────────
class MonitorTests(NoNetworkCase):
    def test_first_tick_is_baseline_then_digest(self):
        tg = FakeTG()
        items = [{"shortId": "a1", "state": "downloading", "title": "Video A"},
                 {"shortId": "b2", "state": "queued", "title": "Video B"}]
        app = FakeApp({"/api/queue": lambda _b: {"ok": True, "items": items},
                       "/api/cookies": {"ok": True, "sites": []},
                       "/api/watch": {"ok": True, "items": []}})
        m = Monitor(tg, app, cfg())
        m.tick()
        self.assertEqual(tg.texts(), [], "nhịp đầu chỉ ghi mốc")
        items[0] = {"shortId": "a1", "state": "completed", "title": "Video A"}
        items[1] = {"shortId": "b2", "state": "failed", "title": "Video B", "error": "⚠️ Tải thất bại"}
        items.append({"shortId": "c3", "state": "queued", "title": "Video C"})
        m.mark_own(["d4"])
        items.append({"shortId": "d4", "state": "queued", "title": "Link tự gửi"})
        m.tick()
        text = tg.texts()[-1]
        self.assertIn("Tải xong 1", text)
        self.assertIn("Lỗi 1", text)
        self.assertIn("1 mục mới", text)
        self.assertNotIn("Link tự gửi", text, "mục người dùng tự gửi không báo lại là mới")
        buttons = [b["callback_data"] for row in tg.calls[-1][3]["inline_keyboard"] for b in row]
        self.assertIn("qa:b2:retry:failed:0", buttons)

    def test_offline_and_back_online(self):
        tg = FakeTG()
        app = FakeApp({"/api/queue": {"ok": True, "items": []}, "/api/cookies": {"ok": True, "sites": []},
                       "/api/watch": {"ok": True, "items": []}})
        m = Monitor(tg, app, cfg())
        m.tick()
        app.offline = True
        m.tick()
        self.assertIn("đã tắt", tg.texts()[-1])
        app.offline = False
        m.tick()
        self.assertIn("chạy lại", tg.texts()[-1])

    def test_cookie_alert_only_for_real_cookie_errors(self):
        tg = FakeTG()
        m = Monitor(tg, FakeApp(), cfg())
        m._maybe_cookie_alert({"extractor": "youtube", "error": "🚫 YouTube đang chặn tải… thêm cookie (Cài đặt → Cookie)"})
        self.assertEqual(tg.texts(), [])
        m._maybe_cookie_alert({"extractor": "youtube", "error": "x\n👉 Cookie cần nạp lại là của trang YouTube"})
        self.assertIn("Cookie youtube", tg.texts()[-1])
        m._maybe_cookie_alert({"extractor": "youtube", "error": "👉 Cookie cần nạp lại"})
        self.assertEqual(len(tg.texts()), 1, "tối đa 1 lần/giờ/trang")

    def test_board_is_edited_not_resent(self):
        tg = FakeTG()
        app = FakeApp({"/api/queue": {"ok": True, "items": []}, "/api/status": STATUS,
                       "/api/cookies": {"ok": True, "sites": []}, "/api/watch": {"ok": True, "items": []}})
        m = Monitor(tg, app, cfg())
        m.add_board(CHAT, 50)
        m.tick()
        m.tick()
        kinds = [c[0] for c in tg.calls]
        self.assertEqual(kinds.count("send"), 0)
        self.assertGreaterEqual(kinds.count("edit"), 2)


# ───────────────────────── tiện ích ─────────────────────────
class UtilTests(unittest.TestCase):
    def test_cache_ttl(self):
        t = [0.0]
        c = TTLCache(clock=lambda: t[0])
        n = [0]

        def load():
            n[0] += 1
            return n[0]

        self.assertEqual(c.get("k", 15, load), 1)
        t[0] = 10
        self.assertEqual(c.get("k", 15, load), 1)
        t[0] = 16
        self.assertEqual(c.get("k", 15, load), 2)

    def test_guess_cookie_site(self):
        text = ".tiktok.com\tTRUE\t/\tTRUE\t0\ta\tb\n#HttpOnly_.tiktok.com\tTRUE\t/\tTRUE\t0\tc\td\n.youtube.com\tT\t/\tT\t0\te\tf\n"
        self.assertEqual(fmt.guess_cookie_site(text, COOKIE_SITES), "tiktok")
        self.assertIsNone(fmt.guess_cookie_site("evil-tiktok.com\tT\t/\tT\t0\ta\tb\n", COOKIE_SITES))

    def test_cookie_sites_match_rust(self):
        rust = (HERE.parent / "src-tauri" / "src" / "control_api.rs").read_text(encoding="utf-8")
        start = rust.index("pub const COOKIE_SITES")
        block = rust[start:rust.index("];", start)]
        parsed = {m.group(1): tuple(re.findall(r'"([^"]+)"', m.group(2)))
                  for m in re.finditer(r'\("(\w+)", &\[([^\]]*)\]\)', block)}
        self.assertEqual(parsed, COOKIE_SITES, "Sửa COOKIE_SITES ở CẢ Rust và Python")

    def test_parse_chrono_nanoseconds(self):
        self.assertAlmostEqual(fmt.parse_time("2026-10-30T01:02:03.123456789Z"),
                               fmt.parse_time("2026-10-30T01:02:03.123456Z"))

    def test_config_with_bom_and_missing_token(self):
        with tempfile.TemporaryDirectory() as d, mock.patch.dict(os.environ, {}, clear=False):
            os.environ.pop("BQBOT_TOKEN", None)
            p = Path(d) / "config.json"
            p.write_text('{"telegram_token": "123:x", "allowed_chat_ids": [5]}', encoding="utf-8-sig")
            self.assertEqual(config_mod.load(p).allowed_chat_ids, [5])
            p.write_text('{"telegram_token": ""}', encoding="utf-8")
            with self.assertRaises(config_mod.ConfigError):
                config_mod.load(p)
            p.write_text("{hỏng", encoding="utf-8")
            with self.assertRaises(config_mod.ConfigError):
                config_mod.load(p)

    def test_compress_plan(self):
        from bqbot import compress
        self.assertAlmostEqual(compress.parse_duration("  Duration: 00:13:12.50, start: 0"), 792.5)
        vk, h = compress.plan(792.5, 45)
        self.assertGreater(vk, 300)
        self.assertIn(h, (360, 480, 720))
        # Bitrate tính ra phải giữ tổng dung lượng dưới mục tiêu.
        self.assertLessEqual((vk + compress.AUDIO_KBPS) * 792.5 / 8192, 45.01)
        self.assertIsNone(compress.plan(4 * 3600, 45), "video quá dài thì báo, không gửi bản vỡ hình")

    def test_format_helpers(self):
        self.assertEqual(fmt.bar(50, 4), "██░░")
        self.assertEqual(fmt.size(1536), "1.5 KB")
        self.assertEqual(fmt.eta(3725), "1g02p")
        self.assertIn("&lt;b&gt;", fmt.esc("<b>"))


if __name__ == "__main__":
    unittest.main()
