"""Lệnh + nút bấm của bot. Mỗi lệnh trả về `Task` (nhẹ/nặng) cho Dispatcher.

THÊM LỆNH MỚI (quy trình 3 bước, xem README):
1. Thêm route Rust trong `control_api.rs` (nếu cần dữ liệu/thao tác mới).
2. Thêm hàm mỏng trong `app_client.py`.
3. Thêm 1 dòng vào `COMMANDS` / `CALLBACKS` dưới đây + hàm `_cmd_*`/`_cb_*`.
"""

from __future__ import annotations

import logging
import re
import threading
import time
from typing import Any, Callable

from . import fmt
from .app_client import AppClient, AppError, AppOffline
from .dispatcher import Dispatcher, Task

log = logging.getLogger("bqbot.handlers")

# PHẢI KHỚP `COOKIE_SITES` trong src-tauri/src/control_api.rs (test kiểm tra).
COOKIE_SITES: dict[str, tuple[str, ...]] = {
    "youtube": ("youtube.com", "google.com"),
    "tiktok": ("tiktok.com",),
    "douyin": ("douyin.com",),
    "facebook": ("facebook.com",),
    "instagram": ("instagram.com",),
    "bilibili": ("bilibili.com", "bilibili.tv"),
    "twitter": ("x.com", "twitter.com"),
}
SITE_ALIASES = {"yt": "youtube", "tt": "tiktok", "fb": "facebook", "ig": "instagram", "x": "twitter",
                "bili": "bilibili", "dy": "douyin"}
URL_RE = re.compile(r"https?://[^\s<>\"']+", re.I)
PER_PAGE = 8
PENDING_COOKIE_TTL = 600

BOT_COMMANDS = [
    ("menu", "Bảng điều khiển"),
    ("status", "Tình trạng tải + kênh"),
    ("queue", "Hàng đợi đang chạy"),
    ("failed", "Mục lỗi (thử lại)"),
    ("channels", "Kênh theo dõi"),
    ("check", "Quét kênh ngay (/check tên-nhóm)"),
    ("addchannel", "Thêm kênh theo dõi (/addchannel link)"),
    ("audio", "Tải âm thanh (/audio link…)"),
    ("cookies", "Tình trạng cookie / đổi cookie"),
    ("history", "Video tải xong gần đây"),
    ("file", "Gửi file đã tải (/file mã)"),
    ("board", "Bảng theo dõi tự cập nhật"),
    ("settings", "Cài đặt nhanh"),
    ("fix", "Sửa lỗi bộ tải (cập nhật yt-dlp)"),
    ("help", "Hướng dẫn"),
]

HELP = """<b>🤖 Bot BQHungDown</b>
• Gửi link (1 hay nhiều) → tự thêm vào hàng đợi tải.
• <code>/audio link</code> → tải âm thanh.
• Gửi file <b>cookies.txt</b> → thay cookie (ghi tên trang ở chú thích nếu muốn: youtube, tiktok, douyin, facebook, instagram, bilibili, twitter).
• /board → 1 tin tự cập nhật tiến độ (không spam).
• Bot tự báo: tải xong / lỗi / video mới từ kênh / cookie sắp hết hạn / app tắt.
Dùng /menu để bấm nút."""


def kb(rows: list[list[tuple[str, str]]]) -> dict:
    return {"inline_keyboard": [[{"text": t, "callback_data": d} for t, d in row] for row in rows if row]}


HOME = ("🏠 Menu", "m")


def main_menu() -> dict:
    return kb([
        [("📊 Tình trạng", "st"), ("⬇️ Đang chạy", "q:active:0")],
        [("❌ Lỗi", "q:failed:0"), ("✅ Xong", "q:completed:0")],
        [("📺 Kênh", "w:a:0"), ("🍪 Cookie", "ck")],
        [("⚙️ Cài đặt", "s"), ("📡 Bảng theo dõi", "bd")],
        [("🔧 Sửa lỗi tải", "fx"), ("🕘 Lịch sử", "h")],
    ])


class Router:
    def __init__(self, tg, app: AppClient, cfg, monitor=None):
        self.tg = tg
        self.app = app
        self.cfg = cfg
        self.monitor = monitor
        self.dispatcher: Dispatcher | None = None
        self._pending_cookie: dict[str, tuple[float, str]] = {}
        self._pending_lock = threading.Lock()
        self.commands: dict[str, tuple[Callable[[dict, str], None], bool]] = {
            # tên: (hàm, nặng?)
            "start": (self._cmd_menu, False),
            "menu": (self._cmd_menu, False),
            "help": (self._cmd_help, False),
            "status": (self._cmd_status, True),
            "queue": (self._cmd_queue, True),
            "failed": (self._cmd_failed, True),
            "channels": (self._cmd_channels, True),
            "check": (self._cmd_check, True),
            "addchannel": (self._cmd_addchannel, True),
            "audio": (self._cmd_audio, True),
            "cookies": (self._cmd_cookies, True),
            "history": (self._cmd_history, True),
            "file": (self._cmd_file, True),
            "board": (self._cmd_board, True),
            "stopboard": (self._cmd_stopboard, False),
            "settings": (self._cmd_settings, True),
            "fix": (self._cmd_fix, True),
            "id": (self._cmd_id, False),
        }

    # ───────────────────────── định tuyến ─────────────────────────
    def allowed(self, chat_id: int) -> bool:
        return chat_id in self.cfg.allowed_chat_ids

    def route_message(self, msg: dict) -> Task | None:
        chat_id = msg.get("chat", {}).get("id")
        if chat_id is None:
            return None
        text = (msg.get("text") or msg.get("caption") or "").strip()
        if not self.allowed(chat_id):
            return Task(lambda: self._deny(chat_id), "từ chối")
        if msg.get("document"):
            return Task(lambda: self._on_document(chat_id, msg), "nhận file", heavy=True, key=f"chat:{chat_id}")
        if text.startswith("/"):
            head, _, arg = text.partition(" ")
            name = head[1:].split("@", 1)[0].lower()
            entry = self.commands.get(name)
            if not entry:
                return Task(lambda: self._send(chat_id, "Không có lệnh này. Gõ /help."), "lệnh lạ")
            fn, heavy = entry
            return Task(lambda: fn(chat_id, arg.strip()), "/" + name, heavy=heavy, key=f"chat:{chat_id}")
        urls = URL_RE.findall(text)
        if urls:
            return Task(lambda: self._enqueue(chat_id, urls, audio=False), "thêm link", heavy=True,
                        key=f"chat:{chat_id}")
        return Task(lambda: self._send(chat_id, "Gửi link để tải, hoặc /menu.", main_menu()), "chữ thường")

    def route_callback(self, cb: dict) -> Task | None:
        msg = cb.get("message") or {}
        chat_id = msg.get("chat", {}).get("id")
        mid = msg.get("message_id")
        data = cb.get("data") or ""
        if chat_id is None or not self.allowed(chat_id):
            return None
        parts = data.split(":")
        verb, args = parts[0], parts[1:]
        handler = {
            "m": self._cb_menu, "st": self._cb_status, "q": self._cb_queue, "qa": self._cb_queue_action,
            "b": self._cb_bulk, "bc": self._cb_bulk_confirm, "w": self._cb_watch, "wc": self._cb_watch_check,
            "wq": self._cb_watch_check_group, "wm": self._cb_watch_more, "we": self._cb_watch_enable,
            "ck": self._cb_cookies, "cs": self._cb_cookie_set, "s": self._cb_settings, "sa": self._cb_set_auto,
            "sc": self._cb_set_conc, "si": self._cb_set_interval, "fx": self._cb_fix, "bd": self._cb_board,
            "bx": self._cb_board_stop, "h": self._cb_history, "f": self._cb_file, "x": self._cb_noop,
        }.get(verb)
        if not handler:
            return None
        # Bấm nút (khác dừng/bật bảng) ngay TRÊN tin bảng theo dõi → tin đó
        # chuyển thành màn hình khác; thôi tự cập nhật, kẻo 10 s sau bảng ghi đè
        # mất màn hình người dùng đang thao tác.
        if self.monitor and verb not in ("bx", "bd") and self.monitor.boards().get(chat_id) == mid:
            self.monitor.remove_board(chat_id)
        light = verb in ("m", "x")
        # Thao tác trên 1 mục/kênh: khoá theo đối tượng đó → bấm liên tục vẫn làm lần lượt.
        key = f"obj:{args[0]}" if verb in ("qa", "wc", "wm", "we") and args else f"chat:{chat_id}"
        return Task(lambda: handler(chat_id, mid, args), f"nút {verb}", heavy=not light, key=key)

    # ───────────────────────── tiện ích gửi ─────────────────────────
    def _send(self, chat_id: int, text: str, markup: dict | None = None) -> dict | None:
        return self.tg.send_message(chat_id, text, markup)

    def _show(self, chat_id: int, mid: int | None, text: str, markup: dict | None = None) -> None:
        """Bấm nút → SỬA tin cũ (không gửi tin mới); lệnh gõ → gửi tin."""
        if mid:
            self.tg.edit_message(chat_id, mid, text, markup)
        else:
            self._send(chat_id, text, markup)

    def _guard(self, chat_id: int, mid: int | None, fn: Callable[[], None]) -> None:
        try:
            fn()
        except AppOffline as e:
            self._show(chat_id, mid, f"⚠️ {fmt.esc(e)}", kb([[("🔄 Thử lại", "st"), HOME]]))
        except AppError as e:
            self._show(chat_id, mid, f"❌ {fmt.esc(e)}", kb([[HOME]]))

    def _deny(self, chat_id: int) -> None:
        self._send(chat_id, f"⛔ Bot riêng. Mã chat của bạn: <code>{chat_id}</code>\n"
                            "Chủ app thêm mã này vào <code>allowed_chat_ids</code> trong config.json để dùng.")

    # ───────────────────────── lệnh gõ ─────────────────────────
    def _cmd_menu(self, chat_id: int, _arg: str) -> None:
        self._send(chat_id, "<b>BQHungDown</b> — chọn việc:", main_menu())

    def _cb_menu(self, chat_id: int, mid: int, _args: list[str]) -> None:
        self._show(chat_id, mid, "<b>BQHungDown</b> — chọn việc:", main_menu())

    def _cb_noop(self, *_a) -> None:
        return None

    def _cmd_help(self, chat_id: int, _arg: str) -> None:
        self._send(chat_id, HELP, main_menu())

    def _cmd_id(self, chat_id: int, _arg: str) -> None:
        self._send(chat_id, f"Mã chat: <code>{chat_id}</code>")

    def _cmd_status(self, chat_id: int, _arg: str) -> None:
        self._cb_status(chat_id, None, [])

    def _cb_status(self, chat_id: int, mid: int | None, _args: list[str]) -> None:
        def go():
            st = self.app.status()
            self._show(chat_id, mid, fmt.status_text(st), kb([
                [("🔄 Làm mới", "st"), ("⬇️ Đang chạy", "q:active:0")],
                [("⏸ Dừng hết", "b:pause_all"), ("▶ Chạy tiếp", "b:resume_all")],
                [HOME],
            ]))
        self._guard(chat_id, mid, go)

    def _cmd_queue(self, chat_id: int, arg: str) -> None:
        state = arg if arg in ("all", "active", "queued", "downloading", "paused", "failed",
                               "completed", "skipped", "cancelled") else "active"
        self._cb_queue(chat_id, None, [state, "0"])

    def _cmd_failed(self, chat_id: int, _arg: str) -> None:
        self._cb_queue(chat_id, None, ["failed", "0"])

    # ───────────────────────── hàng đợi ─────────────────────────
    def _queue_markup(self, state: str, page: int, items: list[dict], total: int) -> dict:
        rows: list[list[tuple[str, str]]] = []
        for it in items[page * PER_PAGE:(page + 1) * PER_PAGE]:
            sid = it["shortId"]
            st = it.get("state")
            act = lambda a: f"qa:{sid}:{a}:{state}:{page}"  # noqa: E731
            if st == "downloading":
                rows.append([(f"⏸ {sid}", act("pause")), (f"✖ {sid}", act("cancel"))])
            elif st == "queued":
                rows.append([(f"✖ Huỷ {sid}", act("cancel"))])
            elif st == "paused":
                rows.append([(f"▶ {sid}", act("resume")), (f"✖ {sid}", act("cancel"))])
            elif st == "failed":
                rows.append([(f"🔁 {sid}", act("retry")), (f"🗑 {sid}", act("remove"))])
            elif st == "skipped":
                rows.append([(f"⬇️ Vẫn tải {sid}", act("force")), (f"🗑 {sid}", act("remove"))])
            elif st == "completed":
                rows.append([(f"📤 Gửi file {sid}", f"f:{sid}"), (f"🗑 {sid}", act("remove"))])
            elif st == "cancelled":
                rows.append([(f"🔁 {sid}", act("retry")), (f"🗑 {sid}", act("remove"))])
        pages = max(1, -(-min(total, len(items)) // PER_PAGE))
        nav = [("🔄", f"q:{state}:{page}")]
        if page > 0:
            nav.insert(0, ("◀", f"q:{state}:{page - 1}"))
        if page + 1 < pages:
            nav.append(("▶", f"q:{state}:{page + 1}"))
        rows.append(nav)
        if state == "failed":
            rows.append([("🔁 Thử lại TẤT CẢ mục lỗi", "b:retry_failed")])
        elif state in ("active", "downloading", "queued", "paused"):
            rows.append([("⏸ Dừng hết", "b:pause_all"), ("▶ Chạy tiếp", "b:resume_all"),
                         ("✖ Huỷ hết", "bc:cancel_all")])
        rows.append([("⬇️ Đang chạy", "q:active:0"), ("❌ Lỗi", "q:failed:0"), HOME])
        return kb(rows)

    def _cb_queue(self, chat_id: int, mid: int | None, args: list[str]) -> None:
        state = args[0] if args else "active"
        page = int(args[1]) if len(args) > 1 and args[1].isdigit() else 0

        def go():
            data = self.app.queue(state, limit=400)
            items = data.get("items", [])
            total = data.get("total", len(items))
            p = min(page, max(0, (min(total, len(items)) - 1) // PER_PAGE))
            self._show(chat_id, mid, fmt.queue_text(data, state, p, PER_PAGE),
                       self._queue_markup(state, p, items, total))
        self._guard(chat_id, mid, go)

    def _cb_queue_action(self, chat_id: int, mid: int, args: list[str]) -> None:
        if len(args) < 2:
            return
        sid, action = args[0], args[1]
        back = args[2:4] or ["active", "0"]

        def go():
            self.app.queue_action(sid, action)
            self._cb_queue(chat_id, mid, back)
        self._guard(chat_id, mid, go)

    def _cb_bulk(self, chat_id: int, mid: int, args: list[str]) -> None:
        action = args[0] if args else ""
        names = {"pause_all": "Đã tạm dừng", "resume_all": "Đã cho chạy tiếp", "retry_failed": "Đã thử lại",
                 "cancel_all": "Đã huỷ"}

        def go():
            n = self.app.bulk(action).get("count", 0)
            self._show(chat_id, mid, f"✅ {names.get(action, action)} {n} mục.",
                       kb([[("⬇️ Đang chạy", "q:active:0"), ("❌ Lỗi", "q:failed:0")], [HOME]]))
        self._guard(chat_id, mid, go)

    def _cb_bulk_confirm(self, chat_id: int, mid: int, args: list[str]) -> None:
        action = args[0] if args else ""
        self._show(chat_id, mid, "⚠️ Huỷ <b>TẤT CẢ</b> mục đang chờ/đang tải? File tải dở sẽ bị dọn.",
                   kb([[("✖ Huỷ hết", f"b:{action}"), ("Không", "q:active:0")]]))

    # ───────────────────────── tải link ─────────────────────────
    def _cmd_audio(self, chat_id: int, arg: str) -> None:
        urls = URL_RE.findall(arg)
        if not urls:
            self._send(chat_id, "Cách dùng: <code>/audio link1 link2…</code>")
            return
        self._enqueue(chat_id, urls, audio=True)

    def _enqueue(self, chat_id: int, urls: list[str], audio: bool) -> None:
        urls = list(dict.fromkeys(urls))  # bỏ trùng, giữ thứ tự

        def go():
            res = self.app.enqueue(urls, audio=audio)
            n = res.get("added", 0)
            if self.monitor:  # mục người dùng tự gửi — đừng báo lại là "mục mới"
                self.monitor.mark_own(it.get("shortId") for it in res.get("items", []))
            if n == 0:
                self._send(chat_id, "ℹ️ Không thêm mục nào — link đã có trong hàng đợi, đã tải, hoặc trang không hỗ trợ.")
                return
            kind = "âm thanh" if audio else "video"
            lines = [f"✅ Đã thêm <b>{n}</b> {kind} → <code>{fmt.esc(res.get('folder'))}</code>"]
            lines += [fmt.item_line(it) for it in res.get("items", [])[:10]]
            skipped = len(urls) - n
            if skipped > 0:
                lines.append(f"<i>Bỏ {skipped} link trùng/không hợp lệ.</i>")
            self._send(chat_id, "\n".join(lines), kb([[("⬇️ Xem tiến độ", "q:active:0"), ("📡 Bảng theo dõi", "bd")]]))
        self._guard(chat_id, None, go)

    # ───────────────────────── kênh theo dõi ─────────────────────────
    def _groups(self) -> list[str]:
        try:
            return list(self.app.status().get("watch", {}).get("groups") or [])
        except (AppError, AppOffline):
            return []

    def _cmd_channels(self, chat_id: int, _arg: str) -> None:
        self._cb_watch(chat_id, None, ["a", "0"])

    def _cb_watch(self, chat_id: int, mid: int | None, args: list[str]) -> None:
        flt = args[0] if args else "a"  # a = tất cả, e = đang lỗi, g<i> = nhóm thứ i
        page = int(args[1]) if len(args) > 1 and args[1].isdigit() else 0

        def go():
            groups = self._groups()
            group = None
            title = "Tất cả"
            if flt == "e":
                data = self.app.watch(errors_only=True)
                title = "Đang lỗi"
            elif flt.startswith("g") and flt[1:].isdigit() and int(flt[1:]) < len(groups):
                group = groups[int(flt[1:])]
                data = self.app.watch(group=group)
                title = f"Nhóm {group}"
            else:
                data = self.app.watch()
            items = data.get("items", [])
            chunk = items[page * PER_PAGE:(page + 1) * PER_PAGE]
            pages = max(1, -(-len(items) // PER_PAGE))
            lines = [f"<b>📺 Kênh — {fmt.esc(title)}</b> ({len(items)}) · trang {min(page + 1, pages)}/{pages}", ""]
            lines += [fmt.channel_line(c) for c in chunk] or ["Không có kênh."]
            for c in chunk:
                if c.get("lastError"):
                    name = fmt.esc(fmt.short(c.get("targetName") or c.get("title"), 25))
                    lines.append(f"🔴 {name}: <i>{fmt.esc(fmt.short(c['lastError'].split(chr(10))[0], 120))}</i>")
            rows: list[list[tuple[str, str]]] = []
            for c in chunk:
                name = fmt.short(c.get("targetName") or c.get("title") or c["id"], 18)
                tog = "⏸" if c.get("enabled") else "▶"
                rows.append([(f"🔄 {name}", f"wc:{c['id']}:{flt}:{page}"), ("+1", f"wm:{c['id']}:{flt}:{page}"),
                             (tog, f"we:{c['id']}:{0 if c.get('enabled') else 1}:{flt}:{page}")])
            nav = [("🔄", f"w:{flt}:{page}")]
            if page > 0:
                nav.insert(0, ("◀", f"w:{flt}:{page - 1}"))
            if page + 1 < pages:
                nav.append(("▶", f"w:{flt}:{page + 1}"))
            rows.append(nav)
            rows.append([("Tất cả", "w:a:0"), ("🔴 Đang lỗi", "w:e:0")])
            grow = [(fmt.short(g or "Chưa nhóm", 14), f"w:g{i}:0") for i, g in enumerate(groups[:6])]
            if grow:
                rows.append(grow[:3])
                rows.append(grow[3:])
            qlabel = "▶ Quét " + ("nhóm này" if group is not None else "tất cả")
            qdata = f"wq:{flt[1:]}" if group is not None else "wq:a"
            rows.append([(qlabel, qdata), HOME])
            self._show(chat_id, mid, "\n".join(lines), kb(rows))
        self._guard(chat_id, mid, go)

    def _cb_watch_check(self, chat_id: int, mid: int, args: list[str]) -> None:
        def go():
            self.app.watch_action(args[0], "check")
            self._cb_watch(chat_id, mid, args[1:3] or ["a", "0"])
        self._guard(chat_id, mid, go)

    def _cb_watch_more(self, chat_id: int, mid: int, args: list[str]) -> None:
        def go():
            self.app.watch_action(args[0], "more")
            self._cb_watch(chat_id, mid, args[1:3] or ["a", "0"])
        self._guard(chat_id, mid, go)

    def _cb_watch_enable(self, chat_id: int, mid: int, args: list[str]) -> None:
        def go():
            self.app.watch_action(args[0], "enabled", {"enabled": args[1] == "1"})
            self._cb_watch(chat_id, mid, args[2:4] or ["a", "0"])
        self._guard(chat_id, mid, go)

    def _cb_watch_check_group(self, chat_id: int, mid: int, args: list[str]) -> None:
        def go():
            group = None
            if args and args[0].isdigit():
                groups = self._groups()
                i = int(args[0])
                if i < len(groups):
                    group = groups[i]
            self.app.watch_check(group)
            where = f"nhóm <b>{fmt.esc(group or 'Chưa nhóm')}</b>" if group is not None else "<b>tất cả</b> kênh"
            self._show(chat_id, mid, f"▶ Đang quét {where} (chạy nền). Video mới sẽ được báo ở đây.",
                       kb([[("📺 Kênh", "w:a:0"), ("📡 Bảng theo dõi", "bd")], [HOME]]))
        self._guard(chat_id, mid, go)

    def _cmd_check(self, chat_id: int, arg: str) -> None:
        def go():
            group = arg if arg else None
            if group is not None and group not in self._groups():
                self._send(chat_id, f"Không có nhóm '{fmt.esc(group)}'. Dùng /channels để xem nhóm.")
                return
            self.app.watch_check(group)
            self._send(chat_id, "▶ Đang quét kênh (chạy nền). Video mới sẽ được báo ở đây.")
        self._guard(chat_id, None, go)

    def _cmd_addchannel(self, chat_id: int, arg: str) -> None:
        urls = URL_RE.findall(arg)
        if not urls:
            self._send(chat_id, "Cách dùng: <code>/addchannel link-kênh [videos|shorts|all]</code>")
            return
        tab = next((t for t in ("videos", "shorts", "all") if t in arg.split()), "all")
        url = urls[0]
        self._send(chat_id, f"⏳ Đang thêm kênh <code>{fmt.esc(url)}</code>… (có thể mất 1 phút)")

        def done(res: Any, err: BaseException | None) -> None:
            if err:
                self._send(chat_id, f"❌ Không thêm được kênh: {fmt.esc(err)}")
                return
            c = res.get("channel", {})
            self._send(chat_id, "✅ Đã theo dõi:\n" + fmt.channel_line(c)
                       + "\n<i>Đặt nhóm / thư mục đích trên app (trang Theo dõi).</i>", kb([[("📺 Kênh", "w:a:0")]]))
        self._job(f"thêm kênh {url}", lambda: self.app.watch_add(url, tab), done)

    # ───────────────────────── cookie ─────────────────────────
    def _cmd_cookies(self, chat_id: int, _arg: str) -> None:
        self._cb_cookies(chat_id, None, [])

    def _cb_cookies(self, chat_id: int, mid: int | None, _args: list[str]) -> None:
        def go():
            self.app.cache.clear("cookies")
            self._show(chat_id, mid, fmt.cookies_text(self.app.cookies()), kb([[("🔄 Làm mới", "ck"), HOME]]))
        self._guard(chat_id, mid, go)

    def _on_document(self, chat_id: int, msg: dict) -> None:
        doc = msg["document"]
        name = (doc.get("file_name") or "").lower()
        if not (name.endswith(".txt") or "cookie" in name):
            self._send(chat_id, "Bot chỉ nhận file cookie dạng <b>.txt</b> (Netscape, xuất bằng tiện ích Get cookies.txt).")
            return
        try:
            raw = self.tg.download_file(doc["file_id"])
        except Exception as e:
            self._send(chat_id, f"❌ Không tải được file từ Telegram: {fmt.esc(e)}")
            return
        content = raw.decode("utf-8", errors="replace")
        cap = (msg.get("caption") or "").strip().lower()
        site = SITE_ALIASES.get(cap, cap) if cap else None
        if site and site not in COOKIE_SITES:
            site = None
        site = site or fmt.guess_cookie_site(content, COOKIE_SITES)
        if site:
            self._apply_cookie(chat_id, None, site, content)
            return
        token = str(msg.get("message_id"))
        key = f"{chat_id}:{token}"
        with self._pending_lock:
            now = time.monotonic()
            self._pending_cookie = {k: v for k, v in self._pending_cookie.items() if now - v[0] < PENDING_COOKIE_TTL}
            self._pending_cookie[key] = (now, content)
        self._send(chat_id, "File này là cookie của trang nào?", self._cookie_site_buttons(token))

    def _cb_cookie_set(self, chat_id: int, mid: int, args: list[str]) -> None:
        if len(args) < 2:
            return
        key = f"{chat_id}:{args[0]}"
        with self._pending_lock:
            entry = self._pending_cookie.get(key)
        if not entry:
            self._show(chat_id, mid, "⌛ File đã quá hạn chờ — gửi lại file cookie.")
            return
        self._apply_cookie(chat_id, mid, args[1], entry[1], retry_token=args[0])

    def _cookie_site_buttons(self, token: str) -> dict:
        sites = list(COOKIE_SITES)
        return kb([[(k, f"cs:{token}:{k}") for k in sites[i:i + 3]] for i in range(0, len(sites), 3)])

    def _apply_cookie(self, chat_id: int, mid: int | None, site: str, content: str,
                      retry_token: str | None = None) -> None:
        def go():
            try:
                res = self.app.set_cookie(site, content)
            except AppError as e:
                if retry_token is None:
                    raise
                # Chọn nhầm trang → GIỮ file, cho chọn lại (không bắt gửi lại file).
                self._show(chat_id, mid, f"❌ {fmt.esc(e)}\nChọn lại trang đúng:",
                           self._cookie_site_buttons(retry_token))
                return
            if retry_token is not None:
                with self._pending_lock:
                    self._pending_cookie.pop(f"{chat_id}:{retry_token}", None)
            dl = fmt.days_left(res.get("earliestExpiry"))
            exp = f" · hạn sớm nhất còn {dl:.0f} ngày" if dl is not None else ""
            warn = f"\n⚠️ {res['expired']} cookie trong file đã hết hạn." if res.get("expired") else ""
            self._show(chat_id, mid, f"✅ Đã thay cookie <b>{fmt.esc(site)}</b>: {res.get('cookies')} cookie{exp}.{warn}\n"
                                     "Lượt tải tiếp theo dùng cookie mới; mục lỗi bấm 🔁 để thử lại.",
                       kb([[("🔁 Thử lại mục lỗi", "b:retry_failed"), ("🍪 Cookie", "ck")]]))
        self._guard(chat_id, mid, go)

    # ───────────────────────── cài đặt ─────────────────────────
    def _cmd_settings(self, chat_id: int, _arg: str) -> None:
        self._cb_settings(chat_id, None, [])

    def _cb_settings(self, chat_id: int, mid: int | None, _args: list[str]) -> None:
        def go():
            s = self.app.status().get("settings", {})
            auto = s.get("watchAutoEnabled")
            text = (f"<b>⚙️ Cài đặt nhanh</b>\n🤖 Tự quét kênh: <b>{'BẬT' if auto else 'TẮT'}</b>"
                    f" · mỗi {s.get('watchIntervalMin')} phút\n🧵 Số luồng tải: <b>{s.get('maxConcurrency')}</b>"
                    f"\n🌐 Proxy: {s.get('proxies', 0)}\n📁 Thư mục mặc định: <code>{fmt.esc(s.get('defaultFolder'))}</code>")
            self._show(chat_id, mid, text, kb([
                [(("⏸ Tắt" if auto else "▶ Bật") + " tự quét", f"sa:{0 if auto else 1}")],
                [("➖ Luồng", "sc:-"), ("➕ Luồng", "sc:+")],
                [("15 phút", "si:15"), ("30 phút", "si:30"), ("60 phút", "si:60")],
                [HOME],
            ]))
        self._guard(chat_id, mid, go)

    def _cb_set_auto(self, chat_id: int, mid: int, args: list[str]) -> None:
        self._guard(chat_id, mid, lambda: (self.app.settings(watchAutoEnabled=args[:1] == ["1"]),
                                           self._cb_settings(chat_id, mid, [])))

    def _cb_set_conc(self, chat_id: int, mid: int, args: list[str]) -> None:
        def go():
            cur = int(self.app.status().get("settings", {}).get("maxConcurrency") or 3)
            nxt = cur + (1 if args[:1] == ["+"] else -1)
            self.app.settings(maxConcurrency=max(1, min(32, nxt)))
            self._cb_settings(chat_id, mid, [])
        self._guard(chat_id, mid, go)

    def _cb_set_interval(self, chat_id: int, mid: int, args: list[str]) -> None:
        if not args or not args[0].isdigit():
            return
        self._guard(chat_id, mid, lambda: (self.app.settings(watchIntervalMin=int(args[0])),
                                           self._cb_settings(chat_id, mid, [])))

    # ───────────────────────── sửa bộ tải ─────────────────────────
    def _cmd_fix(self, chat_id: int, _arg: str) -> None:
        self._cb_fix(chat_id, None, [])

    def _cb_fix(self, chat_id: int, mid: int | None, _args: list[str]) -> None:
        self._show(chat_id, mid, "🔧 Đang sửa bộ tải (cập nhật yt-dlp, kiểm tra JS runtime)… sẽ báo khi xong.")

        def done(res: Any, err: BaseException | None) -> None:
            if err:
                self._send(chat_id, f"❌ Sửa bộ tải lỗi: {fmt.esc(err)}")
            else:
                self._send(chat_id, f"✅ {fmt.esc(res.get('message', 'Xong'))}",
                           kb([[("🔁 Thử lại mục lỗi", "b:retry_failed"), HOME]]))
        self._job("sửa bộ tải", self.app.fix_engine, done)

    # ───────────────────────── lịch sử / gửi file ─────────────────────────
    def _cmd_history(self, chat_id: int, _arg: str) -> None:
        self._cb_history(chat_id, None, [])

    def _cb_history(self, chat_id: int, mid: int | None, _args: list[str]) -> None:
        def go():
            # Lịch sử có cả mục lỗi/huỷ — chỉ hiện mục tải XONG (mới có file để gửi).
            items = [e for e in self.app.history(limit=30).get("items", [])
                     if e.get("status") == "completed"][:10]
            lines = ["<b>🕘 Tải xong gần đây</b>", ""]
            rows = []
            for e in items:
                lines.append(f"✅ <code>{fmt.esc(e.get('shortId'))}</code> {fmt.esc(fmt.short(e.get('title'), 55))}"
                             f" · {fmt.ago(e.get('finishedAt'))}")
                rows.append([(f"📤 {e.get('shortId')}", f"f:{e.get('shortId')}")])
            if not items:
                lines.append("Chưa có.")
            rows = [sum(rows[i:i + 2], []) for i in range(0, len(rows), 2)]
            rows.append([("🔄", "h"), HOME])
            self._show(chat_id, mid, "\n".join(lines), kb(rows))
        self._guard(chat_id, mid, go)

    def _cmd_file(self, chat_id: int, arg: str) -> None:
        if not arg:
            self._send(chat_id, "Cách dùng: <code>/file mã</code> (mã xem ở /history)")
            return
        self._cb_file(chat_id, None, [arg.split()[0]])

    def _find_output(self, sid: str) -> tuple[str | None, str]:
        for it in self.app.queue("completed", limit=2000).get("items", []):
            if it.get("shortId") == sid:
                return it.get("outputPath"), it.get("title") or ""
        for e in self.app.history(limit=500).get("items", []):
            if e.get("shortId") == sid:
                return e.get("outputPath"), e.get("title") or ""
        return None, ""

    def _cb_file(self, chat_id: int, mid: int | None, args: list[str]) -> None:
        if not args:
            return
        sid = args[0]

        def go():
            import os
            path, title = self._find_output(sid)
            if not path or not os.path.isfile(path):
                self._send(chat_id, f"Không thấy file của <code>{fmt.esc(sid)}</code> trên máy (có thể đã bị tool cắt xử lý/xoá).")
                return
            mb = os.path.getsize(path) / 1024 / 1024
            if mb > self.cfg.max_send_file_mb:
                self._send(chat_id, f"📁 File {mb:.0f} MB — quá giới hạn gửi qua bot ({self.cfg.max_send_file_mb} MB).\n"
                                    f"<code>{fmt.esc(path)}</code>")
                return
            self._send(chat_id, f"📤 Đang gửi {fmt.esc(fmt.short(title, 50))} ({mb:.1f} MB)…")
            as_video = path.lower().endswith((".mp4", ".mov", ".m4v"))

            def done(_res: Any, err: BaseException | None) -> None:
                if err:
                    self._send(chat_id, f"❌ Gửi file lỗi: {fmt.esc(err)}")
            self._job(f"gửi file {sid}",
                      lambda: self.tg.send_file(chat_id, path, fmt.esc(fmt.short(title, 200)), as_video), done)
        self._guard(chat_id, mid, go)

    # ───────────────────────── bảng theo dõi ─────────────────────────
    def _cmd_board(self, chat_id: int, _arg: str) -> None:
        self._cb_board(chat_id, None, [])

    def _cb_board(self, chat_id: int, _mid: int | None, _args: list[str]) -> None:
        if not self.monitor:
            return

        def go():
            text = self.monitor.board_text()
            sent = self._send(chat_id, text, kb([[("⏹ Dừng bảng", "bx")]]))
            if sent:
                self.monitor.add_board(chat_id, sent["message_id"])
        self._guard(chat_id, None, go)

    def _cmd_stopboard(self, chat_id: int, _arg: str) -> None:
        if self.monitor:
            self.monitor.remove_board(chat_id)
        self._send(chat_id, "⏹ Đã dừng bảng theo dõi.")

    def _cb_board_stop(self, chat_id: int, mid: int, _args: list[str]) -> None:
        # Chỉ tắt bảng ĐANG chạy khi bấm đúng trên nó (nút trên bảng cũ không tắt bảng mới).
        if self.monitor and self.monitor.boards().get(chat_id) == mid:
            self.monitor.remove_board(chat_id)
        self._show(chat_id, mid, "⏹ Đã dừng bảng theo dõi.", kb([[("📡 Bật lại", "bd"), HOME]]))

    # ───────────────────────── việc dài ─────────────────────────
    def _job(self, name: str, fn: Callable[[], Any], on_done: Callable[[Any, BaseException | None], None]) -> None:
        """Gửi việc dài đi rồi KHÔNG chờ (Dispatcher báo kết quả khi xong)."""
        if self.dispatcher is None:  # chạy đồng bộ (test/không có dispatcher)
            try:
                on_done(fn(), None)
            except Exception as e:  # noqa: BLE001
                on_done(None, e)
            return
        self.dispatcher.submit_job(name, fn, on_done)
