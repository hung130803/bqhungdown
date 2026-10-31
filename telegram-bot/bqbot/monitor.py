"""Theo dõi nền: so ảnh chụp hàng đợi giữa 2 nhịp → báo sự kiện.

- Mục mới / tải xong / lỗi / bỏ qua (quét kênh, chạy nhóm, tải trên máy) → gom
  vào 1 ĐỢT TẢI = 1 tin tự cập nhật (tổng tiến độ + đang tải + lỗi); xong hết
  thì chốt tổng kết kèm nút gửi video về điện thoại. Không gửi tin rời từng video.
  Link người dùng tự gửi có tin tự cập nhật riêng (`track`).
- Lỗi liên quan cookie → nhắc gửi cookie mới cho ĐÚNG trang (tối đa 1 lần/giờ/trang).
- Cookie sắp hết hạn (< 3 ngày) → nhắc 1 lần/12 giờ.
- Kênh mới bị lỗi quét → báo.
- App tắt / chạy lại → báo.
- Bảng theo dõi: SỬA 1 tin (editMessageText) theo nhịp, không gửi tin mới.

Nhịp đầu tiên chỉ ghi mốc (không báo những gì đã có sẵn).
"""

from __future__ import annotations

import json
import logging
import threading
import time
from pathlib import Path
from typing import Callable

from . import fmt
from .app_client import AppClient, AppError, AppOffline
from .cookie_check import OK as CK_OK, CookieChecker

log = logging.getLogger("bqbot.monitor")

COOKIE_ALERT_EVERY = 3600
# Dấu hiệu lỗi ĐĂNG NHẬP thật (khớp error.rs::is_login_related) — chỉ những lỗi
# này mới đáng nhắc thay cookie. KHÔNG gồm chặn-bot ("not a bot") / 403: đó là
# YouTube chặn theo IP, thay cookie không giải quyết được.
LOGIN_ERROR_HINTS = ("sign in to view", "log in", "login", "confirm your age", "age-restricted",
                     "members-only", "join this channel", "private video", "use --cookies")
NOT_COOKIE_HINTS = ("not a bot", "http error 403", "decompressing", "timed out", "connection")
# Mục đổi sang các trạng thái này thì vào đợt tải (kể cả mục có từ trước khi bot bật).
ACTIVE_OR_DONE = ("queued", "downloading", "completed", "failed", "skipped")
COOKIE_EXPIRY_EVERY = 12 * 3600
CHANNEL_ERR_EVERY = 600
# Tự kiểm tra cookie YouTube lấy từ Firefox: lần đầu ~1 phút sau khi bot chạy,
# sau đó mỗi 6 giờ. Hỏng → báo ngay; vẫn hỏng thì nhắc lại mỗi 24 giờ.
FIREFOX_CHECK_FIRST = 60
FIREFOX_CHECK_EVERY = 6 * 3600
FIREFOX_REMIND_EVERY = 24 * 3600


def _kb(rows):
    return {"inline_keyboard": [[{"text": t, "callback_data": d} for t, d in r] for r in rows if r]}


class Monitor:
    def __init__(self, tg, app: AppClient, cfg, clock: Callable[[], float] = time.monotonic):
        self.tg = tg
        self.app = app
        self.cfg = cfg
        self.clock = clock
        self._prev: dict[str, str] | None = None
        # shortId do CHÍNH bot thêm (người dùng gửi link) → không báo "mục mới".
        self._own: dict[str, float] = {}
        self._own_lock = threading.Lock()
        self.cookie_checker = CookieChecker()
        self._ff_next = clock() + FIREFOX_CHECK_FIRST
        self._ff_last_alert = float("-inf")
        self._online: bool | None = None
        self._boards: dict[int, int] = {}
        self._boards_lock = threading.Lock()
        self._last_board = 0.0
        self._cookie_alert: dict[str, float] = {}
        self._last_expiry_check = -COOKIE_EXPIRY_EVERY
        self._last_channel_check = -CHANNEL_ERR_EVERY
        self._channel_errors: set[str] | None = None
        # Tin "đã thêm" do người dùng gửi link: (chat, message_id) → mục cần theo.
        # Monitor SỬA chính tin đó theo tiến độ tới khi mọi mục xong/lỗi.
        self._tracked: dict[tuple[int, int], dict] = {}
        self._tracked_lock = threading.Lock()
        self._expect_offline_until = float("-inf")
        # Đợt tải hiện tại (1 tin tự cập nhật / chat).
        self._batch_ids: list[str] = []
        self._batch_set: set[str] = set()
        self._batch_msgs: dict[int, int] = {}
        self._batch_started: str | None = None
        self._batch_text = ""
        self._batch_last_edit = float("-inf")
        self._batch_t0: float | None = None      # giờ máy (epoch) lúc mở đợt
        self._batch_done_since: float | None = None
        self._quiet_online_until = float("-inf")
        # Nơi lưu id các tin đang tự cập nhật — bot khởi động lại thì chốt chúng
        # (không để tin "đang tải" đứng hình vĩnh viễn). None = không lưu (test).
        self.state_path: Path | None = None

    def expect_offline(self, seconds: float = 60) -> None:
        """Gọi khi người dùng CHỦ ĐỘNG đóng app — không báo 'app đã tắt' trùng."""
        self._expect_offline_until = self.clock() + seconds

    def quiet_back_online(self, seconds: float = 90) -> None:
        """Người dùng vừa mở app bằng bot (đã có tin "Đã mở app") — khỏi báo trùng."""
        self._quiet_online_until = self.clock() + seconds

    def is_live_message(self, chat_id: int, message_id: int | None) -> bool:
        """Tin này đang được bot TỰ SỬA (đợt tải / tin link tự gửi)? Bấm nút trên
        nó thì màn hình mới phải gửi thành tin MỚI, kẻo vài giây sau bị đè."""
        if message_id is None:
            return False
        with self._tracked_lock:
            if (chat_id, message_id) in self._tracked:
                return True
        return self._batch_msgs.get(chat_id) == message_id

    # ── chốt tin tự cập nhật khi bot khởi động lại ──
    def _save_live(self) -> None:
        if not self.state_path:
            return
        with self._tracked_lock:
            tracked = [{"chat": c, "mid": m, "ids": t["ids"], "folder": t["folder"], "audio": t["audio"]}
                       for (c, m), t in self._tracked.items()]
        data = {"batch": [[c, m] for c, m in self._batch_msgs.items()], "tracked": tracked,
                "batch_ids": list(self._batch_ids), "batch_started": self._batch_started,
                "batch_t0": self._batch_t0}
        try:
            self.state_path.parent.mkdir(parents=True, exist_ok=True)
            self.state_path.write_text(json.dumps(data), encoding="utf-8")
        except OSError:
            pass

    def restore_live_messages(self) -> int:
        """Gọi lúc bot khởi động: NHẬN LẠI đợt tải + tin tự cập nhật của lần chạy
        trước → bot khởi động lại giữa chừng vẫn sửa tiếp đúng tin cũ và vẫn gửi
        báo cáo khi đợt xong (trước đây đợt bị "quên" → xong mà không báo gì)."""
        if not self.state_path or not self.state_path.exists():
            return 0
        try:
            data = json.loads(self.state_path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            return 0
        n = 0
        ids = [str(x) for x in data.get("batch_ids") or []]
        if ids:
            self._batch_ids, self._batch_set = ids, set(ids)
            self._batch_msgs = {int(c): int(m) for c, m in data.get("batch") or []}
            self._batch_started = data.get("batch_started") or time.strftime("%H:%M")
            self._batch_t0 = data.get("batch_t0") or time.time()
            n += 1
        for t in data.get("tracked") or []:
            if not isinstance(t, dict):
                continue  # định dạng cũ (chỉ [chat, mid]) — bỏ
            with self._tracked_lock:
                self._tracked[(int(t["chat"]), int(t["mid"]))] = {
                    "ids": list(t.get("ids") or []), "folder": t.get("folder"),
                    "audio": bool(t.get("audio")), "text": ""}
            n += 1
        return n

    # ── tin tự cập nhật cho link người dùng gửi ──
    def track(self, chat_id: int, message_id: int, short_ids: list[str], folder: str | None, audio: bool) -> None:
        ids = [s for s in short_ids if s]
        if not ids:
            return
        with self._tracked_lock:
            self._tracked[(chat_id, message_id)] = {"ids": ids, "folder": folder, "audio": audio, "text": ""}
        self._save_live()

    def _tracked_ids(self) -> set[str]:
        with self._tracked_lock:
            return {sid for t in self._tracked.values() for sid in t["ids"]}

    def _update_tracked(self, items: list[dict]) -> None:
        with self._tracked_lock:
            tracked = list(self._tracked.items())
        if not tracked:
            return
        by_id = {it["shortId"]: it for it in items}
        for (chat, mid), t in tracked:
            # Mục đã bị xoá khỏi hàng đợi → coi như đã huỷ.
            cur = [by_id.get(sid) or {"shortId": sid, "state": "cancelled", "title": ""} for sid in t["ids"]]
            finished = all(it.get("state") in ("completed", "failed", "cancelled", "skipped") for it in cur)
            text = fmt.tracked_text(cur, t["folder"], t["audio"])
            if text == t["text"] and not finished:
                continue
            t["text"] = text
            try:
                self.tg.edit_message(chat, mid, text, self._tracked_markup(cur, finished))
            except Exception as e:  # noqa: BLE001
                log.info("Sửa tin tiến độ lỗi: %s", e)
                if "not found" in str(e).lower() or "can't be edited" in str(e).lower():
                    finished = True
            if finished:
                with self._tracked_lock:
                    self._tracked.pop((chat, mid), None)
                self._save_live()

    @staticmethod
    def _tracked_markup(items: list[dict], finished: bool) -> dict:
        rows: list[list[tuple[str, str]]] = []
        if finished:
            many = len(items) > 1
            for n, it in enumerate(items[:8], 1):
                sid = it["shortId"]
                label = f"{n}. " if many else ""
                if it.get("state") == "completed":
                    rows.append([(f"📤 Gửi về điện thoại {label}".strip(), f"f:{sid}")])
                elif it.get("state") == "failed":
                    rows.append([(f"🔁 Thử lại {label}".strip(), f"qa:{sid}:retry:failed:0"),
                                 (f"ℹ️ Xem lỗi {label}".strip(), f"qi:{sid}:failed:0")])
        else:
            rows.append([("📋 Chi tiết / dừng / huỷ", "q:active:0")])
        return _kb(rows) if rows else _kb([[("🏠 Menu", "m")]])

    # ── gửi ──
    def _notify(self, text: str, markup: dict | None = None) -> None:
        for chat in self.cfg.notify_chat_ids:
            try:
                self.tg.send_message(chat, text, markup)
            except Exception as e:  # noqa: BLE001
                log.warning("Gửi thông báo lỗi: %s", e)

    # ── bảng theo dõi ──
    def add_board(self, chat_id: int, message_id: int) -> None:
        with self._boards_lock:
            old = self._boards.get(chat_id)
            self._boards[chat_id] = message_id
        if old and old != message_id:
            try:
                self.tg.edit_message(chat_id, old, "⏹ Bảng cũ — đã chuyển sang bảng mới bên dưới.")
            except Exception:  # noqa: BLE001
                pass

    def mark_own(self, short_ids) -> None:
        now = self.clock()
        with self._own_lock:
            for sid in short_ids:
                if sid:
                    self._own[sid] = now

    def remove_board(self, chat_id: int) -> None:
        with self._boards_lock:
            self._boards.pop(chat_id, None)

    def boards(self) -> dict[int, int]:
        with self._boards_lock:
            return dict(self._boards)

    def board_text(self) -> str:
        try:
            st = self.app.status()
        except (AppOffline, AppError) as e:
            return f"📡 <b>Bảng theo dõi</b>\n⚠️ {fmt.esc(e)}"
        return "📡 <b>Bảng theo dõi</b> (tự cập nhật)\n\n" + fmt.status_text(st) + \
            f"\n\n<i>Cập nhật {time.strftime('%H:%M:%S')}</i>"

    def _update_boards(self) -> None:
        boards = self.boards()
        if not boards:
            return
        text = self.board_text()
        markup = _kb([[("⏹ Dừng bảng", "bx"), ("❌ Lỗi", "q:failed:0")]])
        for chat, mid in boards.items():
            try:
                self.tg.edit_message(chat, mid, text, markup)
            except Exception as e:  # noqa: BLE001
                # Tin bị xoá / quá cũ → thôi cập nhật bảng này.
                if "not found" in str(e).lower() or "can't be edited" in str(e).lower():
                    self.remove_board(chat)

    # ── nhịp chính ──
    def tick(self) -> None:
        now = self.clock()
        try:
            data = self.app.queue("all", limit=2000)
        except AppOffline:
            # Người dùng vừa đóng app bằng bot → đã có tin "Đã đóng app", khỏi báo trùng.
            if self._online and self.clock() < self._expect_offline_until:
                self._online = False
                return
            if self._online:
                self._notify("⚠️ <b>App BQHungDown trên máy tính đã tắt.</b> Bot vẫn chạy — "
                             "bấm nút dưới để bật lại app từ xa, hoặc chờ bot báo khi app chạy lại.",
                             _kb([[("▶ Mở app trên máy tính", "oa")]]))
            self._online = False
            return
        except AppError as e:
            log.warning("Đọc hàng đợi lỗi: %s", e)
            return
        if self._online is False and self.clock() >= self._quiet_online_until:
            self._notify("✅ App BQHungDown đã chạy lại.")
        self._online = True
        items = data.get("items", [])
        self._diff(items)
        self._update_tracked(items)
        self._update_batch(items)
        if now - self._last_board >= self.cfg.board_interval_sec:
            self._last_board = now
            self._update_boards()
        if now - self._last_expiry_check >= COOKIE_EXPIRY_EVERY:
            self._last_expiry_check = now
            self._check_cookie_expiry()
        if now - self._last_channel_check >= CHANNEL_ERR_EVERY:
            self._last_channel_check = now
            self._check_channel_errors()
        if now >= self._ff_next:
            self._ff_next = now + FIREFOX_CHECK_EVERY
            threading.Thread(target=self.firefox_check, name="kiem-cookie-firefox", daemon=True).start()

    def firefox_check(self) -> None:
        """Chạy ở luồng riêng (mất 5–30 giây). Chỉ khi ô YouTube đang lấy từ Firefox."""
        try:
            sites = self.app.cookies().get("sites", [])
        except (AppOffline, AppError):
            return
        if not any(s.get("site") == "youtube" and s.get("browser") == "firefox" for s in sites):
            return
        prev = self.cookie_checker.latest()
        res = self.cookie_checker.check_now()
        if res is None:
            return
        log.info("Kiểm tra cookie Firefox: %s (%s)", res.status, res.detail)
        self.report_firefox(res, prev)

    def report_firefox(self, res, prev) -> None:
        """Báo khi ĐỔI trạng thái (hỏng ↔ ổn); vẫn hỏng thì nhắc lại mỗi 24 giờ."""
        now = self.clock()
        was_ok = prev is None or prev.status == CK_OK
        if res.status == CK_OK:
            if prev is not None and prev.status != CK_OK:
                self._notify("🟢 <b>Cookie YouTube đã hoạt động lại</b> — app tự lấy từ Firefox, "
                             "tải được video 18+. Anh không cần làm gì thêm.")
            return
        if was_ok or now - self._ff_last_alert >= FIREFOX_REMIND_EVERY:
            self._ff_last_alert = now
            self._notify("🔴 <b>Cookie YouTube đang hỏng</b>\n" + fmt.esc(res.detail) +
                         "\n\nVideo thường vẫn tải bình thường; chỉ video cần đăng nhập (18+…) bị lỗi.\n"
                         "👉 Mở Firefox trên máy tính, đăng nhập YouTube, rồi bấm <b>🧪 Kiểm tra lại</b>.",
                         _kb([[("🦊 Mở Firefox để đăng nhập", "cfo")], [("🧪 Kiểm tra lại", "ckt")]]))

    def _diff(self, items: list[dict]) -> None:
        cur = {it["shortId"]: it.get("state", "") for it in items}
        with self._own_lock:
            now = self.clock()
            self._own = {k: t for k, t in self._own.items() if now - t < 600}
            own = set(self._own)
        if self._prev is None:  # nhịp đầu: ghi mốc
            self._prev = cur
            if not self._batch_ids:
                tracked0 = self._tracked_ids()
                active = [it["shortId"] for it in items if it.get("state") in fmt.ACTIVE_STATES
                          and it["shortId"] not in own and it["shortId"] not in tracked0]
                if active:
                    # Bot bật lúc app đang tải dở → vẫn theo dõi để cuối đợt có báo cáo.
                    self._open_batch(active)
            return
        # Mục có tin tự cập nhật riêng (link người dùng gửi) → không đưa vào đợt.
        tracked = self._tracked_ids()
        for it in items:
            sid, st = it["shortId"], it.get("state")
            before = self._prev.get(sid)
            if before == st:
                continue
            if st == "failed":
                self._maybe_cookie_alert(it)
            if sid in own or sid in tracked:
                continue
            # Mọi mục mới / đổi trạng thái (do quét kênh, chạy nhóm, tải trên máy…)
            # gom vào ĐỢT TẢI hiện tại → 1 tin tự cập nhật thay vì nhiều tin rời.
            # Mục lần đầu thấy đã ở trạng thái KẾT THÚC (vd bị Bỏ qua ngay vì live)
            # chỉ gộp vào đợt ĐANG mở — không tự mở đợt mới (từng sinh 1 tin
            # "Xong đợt tải" cho mỗi mục bỏ qua).
            first_seen_done = before is None and st not in fmt.ACTIVE_STATES
            if first_seen_done and not self._batch_ids:
                continue
            if sid not in self._batch_set and (before is None or st in ACTIVE_OR_DONE):
                self._open_batch([sid])
        self._prev = cur

    def _open_batch(self, ids: list[str]) -> None:
        for sid in ids:
            if sid not in self._batch_set:
                self._batch_ids.append(sid)
                self._batch_set.add(sid)
        if self._batch_started is None:
            self._batch_started = time.strftime("%H:%M")
            self._batch_t0 = time.time()
        self._batch_done_since = None

    # ── 1 tin cho cả đợt tải ──
    def _update_batch(self, items: list[dict], force: bool = False) -> None:
        if not self._batch_ids:
            return
        # Link người dùng tự gửi có thể lọt vào đợt khi tick chạy ĐÚNG lúc app đang
        # thêm dần từng link (trước khi bot kịp mark_own) → loại ra, tránh tin trùng.
        with self._own_lock:
            own = set(self._own)
        excl = own | self._tracked_ids()
        if excl & self._batch_set:
            self._batch_ids = [x for x in self._batch_ids if x not in excl]
            self._batch_set -= excl
            if not self._batch_ids:
                return
        by_id = {it["shortId"]: it for it in items}
        cur = [by_id.get(s) or {"shortId": s, "state": "cancelled", "title": ""} for s in self._batch_ids]
        finished = not any(it.get("state") in fmt.ACTIVE_STATES for it in cur)
        now = self.clock()
        if finished:
            # Quét nhóm thêm video dần từng kênh → chờ thêm một lúc mới chốt,
            # kẻo 1 nhóm ra nhiều báo cáo lẻ.
            if self._batch_done_since is None:
                self._batch_done_since = now
            grace = getattr(self.cfg, "batch_finish_grace_sec", 90)
            if not force and now - self._batch_done_since < grace:
                return
        else:
            self._batch_done_since = None
        if not finished and not force and now - self._batch_last_edit < self.cfg.board_interval_sec:
            return
        if finished and not self._batch_msgs and all(it.get("state") in ("skipped", "cancelled") for it in cur):
            # Đợt chỉ toàn mục bị bỏ qua/huỷ và chưa từng gửi tin → khỏi báo.
            self._batch_ids, self._batch_set = [], set()
            self._batch_started, self._batch_text = None, ""
            return
        text = fmt.batch_text(cur, self._batch_started or "", finished)
        markup = self._batch_markup(cur, finished)
        if text != self._batch_text or finished:
            self._batch_last_edit = now
            self._batch_text = text
            for chat in self.cfg.notify_chat_ids:
                mid = self._batch_msgs.get(chat)
                try:
                    if mid is None:
                        sent = self.tg.send_message(chat, text, markup)
                        if sent:
                            self._batch_msgs[chat] = sent["message_id"]
                            self._save_live()
                    else:
                        self.tg.edit_message(chat, mid, text, markup)
                except Exception as e:  # noqa: BLE001
                    log.info("Cập nhật tin đợt tải lỗi: %s", e)
        if finished:
            self._send_batch_report(cur)
            self._batch_ids, self._batch_set, self._batch_msgs = [], set(), {}
            self._batch_started, self._batch_text = None, ""
            self._batch_t0, self._batch_done_since = None, None
        self._save_live()

    def _send_batch_report(self, cur: list[dict]) -> None:
        """BÁO CÁO TỔNG gửi thành TIN MỚI (điện thoại có thông báo) — tin tiến độ
        cũ chỉ được sửa nên nằm tít phía trên, người dùng không thấy đợt đã xong."""
        try:
            channels = self.app.watch().get("items", [])
        except (AppOffline, AppError):
            channels = []
        text = fmt.batch_report(cur, channels, self._batch_started or "", self._batch_t0)
        markup = self._batch_markup(cur, True)
        for chat in self.cfg.notify_chat_ids:
            try:
                self.tg.send_message(chat, text, markup)
            except Exception as e:  # noqa: BLE001
                log.info("Gửi báo cáo đợt tải lỗi: %s", e)

    @staticmethod
    def _batch_markup(items: list[dict], finished: bool) -> dict:
        failed = any(it.get("state") == "failed" for it in items)
        if not finished:
            rows = [[("📋 Chi tiết / dừng / huỷ", "q:active:0")]]
            if failed:
                rows.append([("❌ Xem lỗi", "q:failed:0")])
            return _kb(rows)
        rows = []
        if any(it.get("state") == "completed" for it in items):
            rows.append([("📤 Chọn video gửi về điện thoại", "q:completed:0")])
        if failed:
            rows.append([("❌ Xem lỗi", "q:failed:0"), ("🔁 Thử lại mọi mục lỗi", "b:retry_failed")])
        return _kb(rows or [[("🏠 Menu", "m")]])

    def _maybe_cookie_alert(self, it: dict) -> None:
        err = (it.get("error") or "").lower()
        site = (it.get("extractor") or "").lower() or "chung"
        # Lỗi gốc PHẢI là lỗi đăng nhập; chặn-bot/403/lỗi mạng KHÔNG phải lỗi cookie
        # (app cũ từng gắn nhãn 🍪 cho cả "Error -3 while decompressing data").
        if any(k in err for k in NOT_COOKIE_HINTS) or not any(k in err for k in LOGIN_ERROR_HINTS):
            return
        browser = None
        try:
            browser = next((s.get("browser") for s in self.app.cookies().get("sites", [])
                            if s.get("site") == site), None)
        except (AppOffline, AppError):
            pass
        is_cookie_msg = err.startswith("🍪") or "no longer valid" in err
        # Ô lấy cookie từ Firefox: Firefox đăng xuất KHÔNG sinh cảnh báo "cookie
        # hết hạn" — lỗi ra là "🔞 giới hạn tuổi". Nên xét cả trường hợp đó.
        if not (is_cookie_msg or (browser == "firefox" and err.startswith("🔞"))):
            return
        now = self.clock()
        if now - self._cookie_alert.get(site, -COOKIE_ALERT_EVERY) < COOKIE_ALERT_EVERY:
            return
        self._cookie_alert[site] = now
        if browser == "firefox":
            # Ô lấy cookie tự động từ Firefox → lỗi đăng nhập = Firefox đã đăng xuất.
            self._notify(f"🦊 <b>Firefox chưa đăng nhập {fmt.esc(site)}</b> (hoặc đã đăng xuất) — video cần "
                         "đăng nhập (giới hạn tuổi…) chưa tải được. Đăng nhập lại trên Firefox là app tự lấy.",
                         _kb([[("🦊 Mở Firefox để đăng nhập", "cfo")], [("🧪 Kiểm tra lại", "ckt")]]))
            return
        self._notify(f"🍪 <b>Cookie {fmt.esc(site)} có vẻ đã hỏng/hết hạn</b> — "
                     f"gửi file cookies.txt mới vào đây (chú thích: <code>{fmt.esc(site)}</code>).",
                     _kb([[("🍪 Xem cookie", "ck")]]))

    def _check_cookie_expiry(self) -> None:
        try:
            self.app.cache.clear("cookies")
            data = self.app.cookies()
        except (AppOffline, AppError):
            return
        warn = []
        for s in data.get("sites", []):
            dl = fmt.days_left(s.get("earliestExpiry"))
            if s.get("exists") and dl is not None and dl < 3:
                warn.append(f"• {fmt.esc(s.get('label'))}: {'ĐÃ hết hạn' if dl < 0 else f'còn {dl:.1f} ngày'}")
        if warn:
            self._notify("🍪 <b>Cookie sắp hết hạn</b>\n" + "\n".join(warn) + "\nGửi file cookies.txt mới để thay.",
                         _kb([[("🍪 Xem cookie", "ck")]]))

    def _check_channel_errors(self) -> None:
        try:
            self.app.cache.clear("watch")
            items = self.app.watch(errors_only=True).get("items", [])
        except (AppOffline, AppError):
            return
        cur = {c["id"] for c in items if c.get("enabled")}
        if self._channel_errors is not None:
            fresh = [c for c in items if c["id"] in cur - self._channel_errors]
            if fresh:
                lines = [f"🔴 <b>{len(fresh)} kênh quét lỗi</b>"]
                for c in fresh[:6]:
                    name = fmt.esc(fmt.short(c.get("targetName") or c.get("title") or c.get("url"), 35))
                    lines.append(f"• {name}: <i>{fmt.esc(fmt.short((c.get('lastError') or '').split(chr(10))[0], 100))}</i>")
                self._notify("\n".join(lines), _kb([[("📺 Kênh lỗi", "w:e:0")]]))
        self._channel_errors = cur
