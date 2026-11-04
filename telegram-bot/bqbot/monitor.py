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

from . import fmt, launcher, report
from .app_client import AppClient, AppError, AppOffline
from .cookie_check import OK as CK_OK, CookieChecker

log = logging.getLogger("bqbot.monitor")

COOKIE_ALERT_EVERY = 3600
# Dấu hiệu lỗi ĐĂNG NHẬP thật (khớp error.rs::is_login_related) — chỉ những lỗi
# này mới đáng nhắc thay cookie. KHÔNG gồm chặn-bot ("not a bot") / 403: đó là
# YouTube chặn theo IP, thay cookie không giải quyết được.
LOGIN_ERROR_HINTS = ("sign in to view", "log in", "login", "confirm your age", "age-restricted",
                     "members-only", "join this channel", "private video", "use --cookies")
NOT_COOKIE_HINTS = ("not a bot", "http error 403")
NETWORK_HINTS = ("decompressing", "timed out", "connection")
# Mục đổi sang các trạng thái này thì vào đợt tải (kể cả mục có từ trước khi bot bật).
ACTIVE_OR_DONE = ("queued", "downloading", "completed", "failed", "skipped")
COOKIE_EXPIRY_EVERY = 12 * 3600
CHANNEL_ERR_EVERY = 600
# Tự kiểm tra cookie YouTube lấy từ Firefox: lần đầu ~1 phút sau khi bot chạy,
# sau đó mỗi 6 giờ. Hỏng → báo ngay; vẫn hỏng thì nhắc lại mỗi 24 giờ.
FIREFOX_CHECK_FIRST = 60
FIREFOX_CHECK_EVERY = 6 * 3600
FIREFOX_REMIND_EVERY = 24 * 3600
# Đợt tải lưu lại từ lần chạy trước chỉ nhận lại nếu chưa quá ngần này giây.
RESTORE_MAX_AGE = 6 * 3600
# Đọc trạng thái Lịch tải (dây chuyền) của app mỗi ngần này giây.
DAYCHUYEN_EVERY = 60
# Báo lỗi Lịch tải tối đa 1 tin/giờ MỖI LOẠI; vòng lịch im quá ngần này = kẹt.
DAYCHUYEN_ALERT_EVERY = 3600
DAYCHUYEN_NHIP_KET = 300
# App tải TẮT BẤT THƯỜNG (bản release panic = abort cả app, mất điện app…):
# tiến trình không còn → sau 30 giây; còn tiến trình mà cổng điều khiển im
# → sau 3 phút. Tự mở lại tối đa APP_MO_LAI_MAX lần / giờ.
APP_CHET_KHONG_TIEN_TRINH = 30
APP_CHET_CONG_IM = 180
APP_MO_LAI_MAX = 3


class LichBoards:
    """Bảng Lịch tải đang mở ở mỗi chat: (tin, mã bảng). Mã đổi khi gửi bảng
    MỚI → nút trên bảng cũ hết hạn. `pause` = đang hỏi xác nhận, khỏi tự sửa đè."""

    def __init__(self, clock: Callable[[], float] = time.monotonic):
        self.clock = clock
        self._b: dict[int, dict] = {}
        self._lock = threading.Lock()

    def set(self, chat_id: int, mid: int, tok: str, text: str, pause: float = 0.0) -> None:
        with self._lock:
            self._b[chat_id] = {"mid": mid, "tok": tok, "text": text, "pause": self.clock() + pause}

    def get(self, chat_id: int) -> dict | None:
        with self._lock:
            b = self._b.get(chat_id)
            return dict(b) if b else None

    def remove(self, chat_id: int) -> None:
        with self._lock:
            self._b.pop(chat_id, None)

    def items(self) -> list[tuple[int, dict]]:
        with self._lock:
            return [(c, dict(b)) for c, b in self._b.items()]


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
        self._report_sent: str | None = None
        self._report_retry_at = float("-inf")
        self._save_lock = threading.Lock()
        self._saved_text = ""
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
        # Dây chuyền (#28): Lịch tải bật thì bot tải KHÔNG mở "đợt tải" đoán
        # trộn nhóm nữa — báo cáo theo nhóm do bot cắt gửi (đọc phiếu đợt).
        self._lich_tai = False
        self._last_daychuyen = float("-inf")
        self._canh_bao_da_gui: set[str] | None = None
        # Lỗi Lịch tải theo loại đã báo (chờ báo "đã ổn lại") + giờ báo gần nhất.
        self._dc_active: dict[str, str] = {}
        self._dc_last: dict[str, float] = {}
        self.wall: Callable[[], float] = time.time
        self.lich_boards = LichBoards(clock)
        # Chat đã nhận tin cảnh báo (theo khoá tin) — gửi lỗi 1 phần thì lần sau
        # chỉ gửi lại cho chat CHƯA nhận.
        self._da_nhan: dict[str, set[int]] = {}
        # App tắt bất thường → báo + tự mở lại.
        self._offline_since: float | None = None
        self._chet_da_xu_ly = False
        self._lan_mo_lai: list[float] = []
        self.app_dang_chay: Callable[[], bool] = launcher.app_dang_chay
        self.da_thoat_em: Callable[[], bool] = launcher.da_thoat_em
        self.mo_app: Callable[[], None] = lambda: self._mo_app_that()
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
        with self._save_lock:
            with self._tracked_lock:
                tracked = [{"chat": c, "mid": m, "ids": list(t["ids"]), "folder": t["folder"],
                            "audio": t["audio"]} for (c, m), t in list(self._tracked.items())]
            data = {"batch": [[c, m] for c, m in list(self._batch_msgs.items())], "tracked": tracked,
                    "batch_ids": list(self._batch_ids), "batch_started": self._batch_started,
                    "batch_t0": self._batch_t0}
            text = json.dumps(data)
            if text == self._saved_text:
                return  # không đổi → khỏi ghi đĩa mỗi nhịp
            try:
                self.state_path.parent.mkdir(parents=True, exist_ok=True)
                tmp = self.state_path.with_suffix(".tmp")
                tmp.write_text(text, encoding="utf-8")
                tmp.replace(self.state_path)
                self._saved_text = text
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
        t0 = data.get("batch_t0")
        if ids and t0 and time.time() - float(t0) > RESTORE_MAX_AGE:
            ids = []  # bot tắt quá lâu → đợt đó đã cũ, khỏi báo cáo sai
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
        self._notify_ok(text, markup)

    def _notify_ok(self, text: str, markup: dict | None = None, key: str | None = None) -> bool:
        """Gửi mọi chat nhận báo; True khi TẤT CẢ đã nhận. Có `key` → nhớ chat đã
        nhận, lần gửi lại chỉ gửi cho chat CHƯA nhận (không gửi trùng)."""
        da = self._da_nhan.setdefault(key, set()) if key else set()
        ok = bool(self.cfg.notify_chat_ids)
        for chat in self.cfg.notify_chat_ids:
            if chat in da:
                continue
            try:
                self.tg.send_message(chat, text, markup)
                da.add(chat)
            except Exception as e:  # noqa: BLE001
                log.warning("Gửi thông báo lỗi: %s", e)
                ok = False
        if ok and key:
            self._da_nhan.pop(key, None)
        return ok

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
                self._offline_since = None
                return
            if self._online:
                self._notify("⚠️ <b>App BQHungDown trên máy tính đã tắt.</b> Bot vẫn chạy — "
                             "bấm nút dưới để bật lại app từ xa, hoặc chờ bot báo khi app chạy lại.",
                             _kb([[("▶ Mở app trên máy tính", "oa")]]))
                self._offline_since = self.clock()
                self._chet_da_xu_ly = False
            self._online = False
            self._check_app_chet()
            return
        except AppError as e:
            log.warning("Đọc hàng đợi lỗi: %s", e)
            return
        if self._online is False and self.clock() >= self._quiet_online_until:
            self._notify("✅ App BQHungDown đã chạy lại.")
        self._online = True
        self._offline_since = None
        if now - self._last_daychuyen >= DAYCHUYEN_EVERY:
            self._last_daychuyen = now
            self._check_daychuyen()
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
        self._maybe_daily_report()
        if now >= self._ff_next:
            self._ff_next = now + FIREFOX_CHECK_EVERY
            threading.Thread(target=self.firefox_check, name="kiem-cookie-firefox", daemon=True).start()

    def _maybe_daily_report(self) -> None:
        """Tự gửi BÁO CÁO NGÀY 1 lần lúc >= DAILY_REPORT_HOUR (giờ máy). Nhớ ngày đã
        gửi trong file để bot khởi động lại không gửi trùng."""
        lt = time.localtime()
        if lt.tm_hour < getattr(self.cfg, "daily_report_hour", 23):
            return
        today = time.strftime("%Y-%m-%d", lt)
        if self._report_sent == today or self.clock() < self._report_retry_at:
            return
        path = self.state_path.parent / "daily_report.json" if self.state_path else None
        if self._report_sent is None and path and path.exists():
            try:
                self._report_sent = json.loads(path.read_text(encoding="utf-8")).get("sent")
            except (OSError, ValueError):
                pass
            if self._report_sent == today:
                return
        self._report_sent = today  # chặn tick sau mở thêm luồng trong lúc đang gửi

        def send():
            try:
                text, markup = report.build(self.app, self.cookie_checker.latest())
            except (AppOffline, AppError) as e:
                # Chưa gửi được (app tắt…) → 10 phút sau thử lại, KHÔNG mất báo cáo hôm nay.
                log.info("Báo cáo ngày lỗi: %s", e)
                self._report_sent = None
                self._report_retry_at = self.clock() + 600
                return
            self._notify(text, markup)
            if path:
                try:
                    path.write_text(json.dumps({"sent": today}), encoding="utf-8")
                except OSError:
                    pass
        threading.Thread(target=send, name="bao-cao-ngay", daemon=True).start()

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

    def _check_daychuyen(self) -> None:
        """Lịch tải bật/tắt, tự sửa bảng Lịch tải, và BÁO LỖI của app:
        - lỗi ĐANG CÓ (`vanDe`, theo loại): báo khi mới có / đổi nội dung, tối đa
          1 tin/giờ mỗi loại; hết lỗi thì báo "đã ổn lại";
        - sự kiện 1 lần (`canhBao`, theo id): gộp theo loại, 1 tin/giờ mỗi loại.
        Chỉ đánh dấu ĐÃ BÁO khi gửi thành công (lỗi mạng thì nhịp sau gửi lại).
        App bản cũ chưa có route → coi như Lịch tải tắt, bot chạy y như trước."""
        try:
            d = self.app.daychuyen()
        except (AppOffline, AppError):
            self._lich_tai = False
            return
        if d.get("ok") is False:
            return
        self._lich_tai = bool(d.get("lichTaiEnabled"))
        self._update_lich_boards(d)
        path = self.state_path.parent / "daychuyen_alerts.json" if self.state_path else None
        if self._canh_bao_da_gui is None:
            self._canh_bao_da_gui = set()
            if path and path.exists():
                try:
                    saved = json.loads(path.read_text(encoding="utf-8"))
                    self._canh_bao_da_gui = set(saved.get("sent", []))
                    self._dc_active = dict(saved.get("active", {}))
                    self._dc_last = {k: float(v) for k, v in saved.get("last", {}).items()}
                except (OSError, ValueError, AttributeError, TypeError):
                    pass
        now = self.wall()
        changed = False
        van_de = {v["loai"]: v.get("text", "") for v in d.get("vanDe") or [] if v.get("loai")}
        nhip = d.get("nhipCuoi")
        if nhip:
            try:
                im = now - fmt.parse_time(nhip)
            except ValueError:
                im = 0
            if im > DAYCHUYEN_NHIP_KET:
                van_de.setdefault("ket", f"Bộ Lịch tải đã {int(im // 60)} phút không chạy nhịp nào — bộ lịch có thể bị kẹt "
                                         "(thử /closeapp rồi /openapp).")
        for loai, text in sorted(van_de.items()):
            moi_co = self._dc_active.get(loai) != text
            if moi_co and now - self._dc_last.get(loai, float("-inf")) >= DAYCHUYEN_ALERT_EVERY:
                if self._notify_ok("⚠️ <b>Dây chuyền tải</b>: " + fmt.esc(text), key=f"vd:{loai}:{text}"):
                    self._dc_active[loai] = text
                    self._dc_last[loai] = now
                    changed = True
        for loai in [k for k in self._dc_active if k not in van_de]:
            if self._notify_ok("✅ <b>Dây chuyền tải</b> đã ổn lại: " + fmt.esc(self._dc_active[loai]),
                               key=f"ok:{loai}:{self._dc_active[loai]}"):
                del self._dc_active[loai]
                changed = True
        # S4: chỉ giữ id còn trong danh sách app đang trả (app tự bỏ cảnh báo
        # > 2 ngày) — KHÔNG cắt theo thứ tự chữ, kẻo id bị rớt rồi báo lại.
        hien_tai = {c["id"] for c in d.get("canhBao") or [] if c.get("id")}
        if self._canh_bao_da_gui - hien_tai:
            self._canh_bao_da_gui &= hien_tai
            changed = True
        moi = [c for c in d.get("canhBao") or [] if c.get("id") and c["id"] not in self._canh_bao_da_gui]
        theo_loai: dict[str, list[dict]] = {}
        for c in moi:
            theo_loai.setdefault(c.get("loai") or "", []).append(c)
        for loai, ds in theo_loai.items():
            key = "su_kien:" + loai
            if now - self._dc_last.get(key, float("-inf")) < DAYCHUYEN_ALERT_EVERY:
                continue  # đã báo loại này trong giờ qua → gộp vào tin sau
            text = "\n".join("• " + fmt.esc(c.get("text", "")) for c in ds) if len(ds) > 1 else fmt.esc(ds[0].get("text", ""))
            if self._notify_ok("⚠️ <b>Dây chuyền tải</b>: " + text, key="ev:" + ",".join(sorted(c["id"] for c in ds))):
                self._canh_bao_da_gui.update(c["id"] for c in ds)
                self._dc_last[key] = now
                changed = True
        if changed and path:
            try:
                path.write_text(json.dumps({"sent": sorted(self._canh_bao_da_gui), "active": self._dc_active,
                                            "last": self._dc_last}), encoding="utf-8")
            except OSError:
                pass

    # ── app tắt bất thường ──
    def _mo_app_that(self) -> None:
        exe = launcher.find_app(self.cfg.app_exe)
        if not exe:
            raise OSError("không tìm thấy bqhungdown.exe (điền app_exe trong config.json)")
        launcher.open_app(exe)

    def _check_app_chet(self) -> None:
        """App đang chạy rồi MẤT mà không thoát êm (không có dấu `thoat_em.json`):
        báo "tắt bất thường" và tự mở lại (tối đa APP_MO_LAI_MAX lần/giờ). Còn tiến
        trình mà cổng im 3 phút = TREO → chỉ báo, không mở bản thứ 2."""
        if self._offline_since is None or self._chet_da_xu_ly:
            return
        if self.da_thoat_em():
            self._chet_da_xu_ly = True  # anh tự thoát (khay / lệnh đóng app) → không tự mở
            return
        im = self.clock() - self._offline_since
        if im < APP_CHET_KHONG_TIEN_TRINH:
            return
        song = self.app_dang_chay()
        if song and im < APP_CHET_CONG_IM:
            return
        self._chet_da_xu_ly = True
        if song:
            self._notify(f"⚠️ <b>App tải không trả lời đã {int(im // 60)} phút</b> (tiến trình vẫn còn — có thể bị treo). "
                         "Bot không mở thêm bản thứ 2. Bấm ⛔ Buộc tắt rồi ▶ Mở app.",
                         _kb([[("⛔ Buộc tắt", "qf"), ("▶ Mở app", "oa")]]))
            return
        now = self.wall()
        self._lan_mo_lai = [t for t in self._lan_mo_lai if now - t < 3600]
        if len(self._lan_mo_lai) >= APP_MO_LAI_MAX:
            self._notify(f"⚠️ <b>App tải đã tắt bất thường</b> — đã tự mở lại {APP_MO_LAI_MAX} lần trong 1 giờ, "
                         "bot THÔI tự mở để khỏi lặp. Kiểm tra máy rồi bấm ▶ Mở app.",
                         _kb([[("▶ Mở app trên máy tính", "oa")]]))
            return
        try:
            self.mo_app()
        except Exception as e:  # noqa: BLE001
            self._notify(f"⚠️ <b>App tải đã tắt bất thường</b> — tự mở lại KHÔNG được: {fmt.esc(e)}",
                         _kb([[("▶ Mở app trên máy tính", "oa")]]))
            return
        self._lan_mo_lai.append(now)
        self.quiet_back_online(120)
        self._notify(f"⚠️ <b>App tải đã tắt bất thường</b> — bot đã tự mở lại "
                     f"(lần {len(self._lan_mo_lai)}/{APP_MO_LAI_MAX} trong giờ). Lịch tải tự nối đợt dở khi app lên.")

    def _update_lich_boards(self, d: dict) -> None:
        """Tự sửa bảng Lịch tải đang mở (chỉ khi nội dung đổi, không đè màn xác nhận)."""
        for chat, b in self.lich_boards.items():
            if self.clock() < b["pause"]:
                continue
            text = fmt.lich_text(d)
            if text == b["text"]:
                continue
            try:
                self.tg.edit_message(chat, b["mid"], text, fmt.lich_markup(d, b["tok"]))
                self.lich_boards.set(chat, b["mid"], b["tok"], text)
            except Exception as e:  # noqa: BLE001
                if "not found" in str(e).lower() or "can't be edited" in str(e).lower():
                    self.lich_boards.remove(chat)

    def _open_batch(self, ids: list[str]) -> None:
        if self._lich_tai:
            return  # Lịch tải bật: không gửi tin "đợt" cũ (bot cắt báo theo nhóm)
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
        # Mục đã bị XOÁ khỏi hàng đợi (người dùng dọn danh sách) → bỏ khỏi đợt,
        # KHÔNG tính là "huỷ" (từng ra báo cáo "🚫 Huỷ N" sai).
        gone = [s for s in self._batch_ids if s not in by_id]
        if gone:
            self._batch_ids = [s for s in self._batch_ids if s in by_id]
            self._batch_set -= set(gone)
            if not self._batch_ids:
                self._batch_msgs, self._batch_started, self._batch_text = {}, None, ""
                self._batch_t0, self._batch_done_since = None, None
                self._save_live()
                return
        cur = [by_id[s] for s in self._batch_ids]
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
        # App ĐÃ kết luận là cookie hỏng (🍪 / "no longer valid" — kể cả khi kèm
        # "not a bot") → tin theo app. Còn lại: phải là lỗi đăng nhập thật.
        if any(k in err for k in NETWORK_HINTS):
            return  # lỗi mạng (app cũ từng dán nhãn 🍪 oan) — thay cookie không giúp
        expired = "no longer valid" in err  # yt-dlp nói thẳng cookie hết hạn
        if not expired and (any(k in err for k in NOT_COOKIE_HINTS)
                            or not any(k in err for k in LOGIN_ERROR_HINTS)):
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
                    icon, label, _k = fmt.channel_status(c)
                    lines.append(f"{icon} {name}: <b>{label}</b>\n    "
                                 f"<i>{fmt.esc(fmt.short((c.get('lastError') or '').split(chr(10))[0], 110))}</i>")
                self._notify("\n".join(lines), _kb([[("📺 Kênh lỗi", "w:e:0")]]))
        self._channel_errors = cur
