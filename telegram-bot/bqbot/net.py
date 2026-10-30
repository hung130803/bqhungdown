"""Tầng mạng: ép IPv4, mỗi luồng 1 requests.Session, gọi Telegram Bot API.

Quy tắc tốc độ (theo yêu cầu chủ app):
- IPv4 bắt buộc (`allowed_gai_family`) — nhiều mạng VN dò IPv6 rất chậm.
- Mỗi luồng 1 Session (threading.local) giữ kết nối keep-alive, timeout=(5, đọc).
- 429: edit*/answerCallbackQuery/deleteMessage → BỎ QUA (không chờ);
  send* → chờ retry_after rồi gửi lại.
- Giãn 0,35 s giữa các lần gọi send* (không áp cho edit/answer).
"""

from __future__ import annotations

import json
import logging
import socket
import threading
import time
from typing import Any, Callable

import requests
import urllib3.util.connection as urllib3_connection

log = logging.getLogger("bqbot.net")

# 1. Ép requests/urllib3 chỉ phân giải IPv4.
urllib3_connection.allowed_gai_family = lambda: socket.AF_INET

CONNECT_TIMEOUT = 5
SEND_GAP_SEC = 0.35
# Phương thức "bỏ qua khi 429" — trễ 1 lần sửa tin/trả lời nút không đáng chờ.
DROP_ON_429 = ("editMessageText", "editMessageReplyMarkup", "answerCallbackQuery", "deleteMessage")

_local = threading.local()


def session() -> requests.Session:
    """Session riêng của luồng hiện tại (requests.Session không an toàn đa luồng)."""
    s = getattr(_local, "session", None)
    if s is None:
        s = requests.Session()
        _local.session = s
    return s


class TelegramError(Exception):
    def __init__(self, method: str, description: str, code: int | None = None):
        super().__init__(f"{method}: {description}")
        self.method = method
        self.description = description
        self.code = code


class TelegramAPI:
    """Gọi Bot API. `transport` thay được trong test để KHÔNG gửi tin thật."""

    def __init__(
        self,
        token: str,
        transport: Callable[..., Any] | None = None,
        sleep: Callable[[float], None] = time.sleep,
        clock: Callable[[], float] = time.monotonic,
    ):
        self._token = token
        self.base = f"https://api.telegram.org/bot{token}/"
        self.file_base = f"https://api.telegram.org/file/bot{token}/"
        self._transport = transport or self._http
        self._sleep = sleep
        self._clock = clock
        self._send_lock = threading.Lock()
        self._last_send = float("-inf")  # lần gửi đầu không phải chờ
        # Telegram bắt chờ (429) → MỌI luồng gửi cùng chờ tới mốc này.
        self._send_blocked_until = float("-inf")

    def redact(self, text: object) -> str:
        """Che token trong mọi chuỗi lỗi — lỗi requests in nguyên URL /bot<TOKEN>/…"""
        s = str(text)
        return s.replace(self._token, "***") if self._token else s

    # ── vận chuyển HTTP thật ──
    def _http(self, method: str, data: dict | None, files: dict | None, read_timeout: float) -> dict:
        url = self.base + method
        payload = None
        if data is not None:
            # reply_markup… phải là JSON string khi gửi dạng form.
            payload = {k: (json.dumps(v, ensure_ascii=False) if isinstance(v, (dict, list)) else v)
                       for k, v in data.items() if v is not None}
        for fh in (files or {}).values():
            fh.seek(0)  # gửi lại sau 429 phải đọc file từ đầu
        resp =session().post(url, data=payload, files=files, timeout=(CONNECT_TIMEOUT, read_timeout))
        try:
            return resp.json()
        except ValueError:
            return {"ok": False, "error_code": resp.status_code, "description": resp.text[:200]}

    def _space_sends(self, method: str) -> None:
        if not method.startswith("send"):
            return
        with self._send_lock:
            now = self._clock()
            wait = max(self._last_send + SEND_GAP_SEC, self._send_blocked_until) - now
            if wait > 0:
                self._sleep(wait)
            self._last_send = self._clock()

    def call(self, method: str, data: dict | None = None, files: dict | None = None,
             read_timeout: float = 30) -> Any:
        attempts = 0
        while True:
            attempts += 1
            self._space_sends(method)
            try:
                res = self._transport(method, data, files, read_timeout)
            except (requests.ConnectionError, requests.ConnectTimeout) as e:
                # Chưa kết nối được = Telegram CHƯA nhận → gửi lại an toàn.
                # (ReadTimeout thì có thể đã nhận rồi → KHÔNG gửi lại, tránh trùng.)
                if method.startswith("send") and attempts < 3 and not isinstance(e, requests.ReadTimeout):
                    self._sleep(1.5 * attempts)
                    continue
                raise TelegramError(method, "lỗi mạng: " + self.redact(e)) from None
            except requests.RequestException as e:
                raise TelegramError(method, "lỗi mạng: " + self.redact(e)) from None
            if res.get("ok"):
                return res.get("result")
            code = res.get("error_code")
            desc = res.get("description", "")
            if code == 429:
                retry = float((res.get("parameters") or {}).get("retry_after", 1))
                if method in DROP_ON_429:
                    log.info("429 trên %s — bỏ qua", method)
                    return None
                if method.startswith("send") and attempts < 5:
                    with self._send_lock:
                        self._send_blocked_until = max(self._send_blocked_until, self._clock() + retry)
                    continue  # _space_sends sẽ chờ tới mốc
            # Sửa tin y nguyên nội dung cũ → Telegram báo lỗi, coi như xong.
            if code == 400 and "message is not modified" in desc:
                return None
            raise TelegramError(method, self.redact(desc), code)

    # ── tiện ích ──
    def send_message(self, chat_id: int, text: str, reply_markup: dict | None = None, **kw) -> dict:
        return self.call("sendMessage", {
            "chat_id": chat_id, "text": text[:4096], "parse_mode": "HTML",
            "disable_web_page_preview": True, "reply_markup": reply_markup, **kw})

    def edit_message(self, chat_id: int, message_id: int, text: str, reply_markup: dict | None = None) -> Any:
        return self.call("editMessageText", {
            "chat_id": chat_id, "message_id": message_id, "text": text[:4096], "parse_mode": "HTML",
            "disable_web_page_preview": True, "reply_markup": reply_markup})

    def answer_callback(self, callback_id: str, text: str = "", alert: bool = False) -> Any:
        return self.call("answerCallbackQuery", {
            "callback_query_id": callback_id, "text": text[:200], "show_alert": alert}, read_timeout=10)

    def delete_message(self, chat_id: int, message_id: int) -> Any:
        return self.call("deleteMessage", {"chat_id": chat_id, "message_id": message_id}, read_timeout=10)

    def get_updates(self, offset: int, timeout: int = 50) -> list:
        return self.call("getUpdates", {
            "offset": offset, "timeout": timeout,
            "allowed_updates": ["message", "callback_query"]}, read_timeout=timeout + 10) or []

    def download_file(self, file_id: str, max_bytes: int = 6 * 1024 * 1024) -> bytes:
        info = self.call("getFile", {"file_id": file_id})
        if not info or info.get("file_size", 0) > max_bytes:
            raise TelegramError("getFile", "file quá lớn hoặc không tải được")
        try:
            resp = session().get(self.file_base + info["file_path"], timeout=(CONNECT_TIMEOUT, 60))
            resp.raise_for_status()
        except requests.RequestException as e:
            raise TelegramError("getFile", "không tải được file: " + self.redact(e)) from None
        return resp.content

    def send_file(self, chat_id: int, path: str, caption: str = "", as_video: bool = False) -> Any:
        method = "sendVideo" if as_video else "sendDocument"
        field = "video" if as_video else "document"
        with open(path, "rb") as fh:
            return self.call(method, {"chat_id": chat_id, "caption": caption[:1024], "parse_mode": "HTML"},
                             files={field: fh}, read_timeout=300)
