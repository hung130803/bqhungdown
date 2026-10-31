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

import logging
import threading
import time
from typing import Callable

from . import fmt
from .app_client import AppClient, AppError, AppOffline

log = logging.getLogger("bqbot.monitor")

COOKIE_ALERT_EVERY = 3600
# Dấu hiệu lỗi ĐĂNG NHẬP thật (khớp error.rs::is_login_related) — chỉ những lỗi
# này mới đáng nhắc thay cookie.
LOGIN_ERROR_HINTS = ("sign in", "log in", "login", "confirm your age", "age-restricted",
                     "members-only", "join this channel", "private video", "use --cookies",
                     "http error 403", "cookie cần nạp lại")
# Mục đổi sang các trạng thái này thì vào đợt tải (kể cả mục có từ trước khi bot bật).
ACTIVE_OR_DONE = ("queued", "downloading", "completed", "failed", "skipped")
COOKIE_EXPIRY_EVERY = 12 * 3600
CHANNEL_ERR_EVERY = 600


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

    def expect_offline(self, seconds: float = 60) -> None:
        """Gọi khi người dùng CHỦ ĐỘNG đóng app — không báo 'app đã tắt' trùng."""
        self._expect_offline_until = self.clock() + seconds

    # ── tin tự cập nhật cho link người dùng gửi ──
    def track(self, chat_id: int, message_id: int, short_ids: list[str], folder: str | None, audio: bool) -> None:
        ids = [s for s in short_ids if s]
        if not ids:
            return
        with self._tracked_lock:
            self._tracked[(chat_id, message_id)] = {"ids": ids, "folder": folder, "audio": audio, "text": ""}

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
            data = self.app.queue("all", limit=5000)
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
        if self._online is False:
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

    def _diff(self, items: list[dict]) -> None:
        cur = {it["shortId"]: it.get("state", "") for it in items}
        with self._own_lock:
            now = self.clock()
            self._own = {k: t for k, t in self._own.items() if now - t < 600}
            own = set(self._own)
        if self._prev is None:  # nhịp đầu: chỉ ghi mốc
            self._prev = cur
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
            if sid not in self._batch_set and (before is None or st in ACTIVE_OR_DONE):
                self._batch_ids.append(sid)
                self._batch_set.add(sid)
                if self._batch_started is None:
                    self._batch_started = time.strftime("%H:%M")
        self._prev = cur

    # ── 1 tin cho cả đợt tải ──
    def _update_batch(self, items: list[dict], force: bool = False) -> None:
        if not self._batch_ids:
            return
        by_id = {it["shortId"]: it for it in items}
        cur = [by_id.get(s) or {"shortId": s, "state": "cancelled", "title": ""} for s in self._batch_ids]
        finished = not any(it.get("state") in fmt.ACTIVE_STATES for it in cur)
        now = self.clock()
        if not finished and not force and now - self._batch_last_edit < self.cfg.board_interval_sec:
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
                    else:
                        self.tg.edit_message(chat, mid, text, markup)
                except Exception as e:  # noqa: BLE001
                    log.info("Cập nhật tin đợt tải lỗi: %s", e)
        if finished:
            self._batch_ids, self._batch_set, self._batch_msgs = [], set(), {}
            self._batch_started, self._batch_text = None, ""

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
            rows.append([("📤 Gửi video về điện thoại", "q:completed:0")])
        if failed:
            rows.append([("❌ Xem lỗi", "q:failed:0"), ("🔁 Thử lại lỗi", "b:retry_failed")])
        return _kb(rows or [[("🏠 Menu", "m")]])

    def _maybe_cookie_alert(self, it: dict) -> None:
        err = (it.get("error") or "").lower()
        # Chỉ lời báo CHÍNH XÁC về cookie của app (error.rs::friendly_reason_for)
        # — câu chặn-bot cũng nhắc "thêm cookie" nhưng không phải cookie hỏng.
        if not ("cookie cần nạp lại" in err or "no longer valid" in err or err.startswith("🍪")):
            return
        # …và lỗi gốc PHẢI là lỗi đăng nhập. App bản cũ gắn nhãn cookie cho cả lỗi
        # mạng ("Error -3 while decompressing data") → bot từng bắt thay cookie oan.
        if not any(k in err for k in LOGIN_ERROR_HINTS):
            return
        site = (it.get("extractor") or "").lower() or "chung"
        now = self.clock()
        if now - self._cookie_alert.get(site, -COOKIE_ALERT_EVERY) < COOKIE_ALERT_EVERY:
            return
        self._cookie_alert[site] = now
        browser = None
        try:
            browser = next((s.get("browser") for s in self.app.cookies().get("sites", [])
                            if s.get("site") == site), None)
        except (AppOffline, AppError):
            pass
        if browser == "firefox":
            # Ô lấy cookie tự động từ Firefox → lỗi đăng nhập = Firefox đã đăng xuất.
            self._notify(f"🦊 <b>Firefox chưa đăng nhập {fmt.esc(site)}</b> (hoặc đã đăng xuất) — video cần "
                         "đăng nhập (giới hạn tuổi…) chưa tải được. Đăng nhập lại trên Firefox là app tự lấy.",
                         _kb([[("🦊 Mở Firefox để đăng nhập", "cfo")]]))
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
