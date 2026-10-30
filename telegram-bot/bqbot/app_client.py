"""Gọi cổng điều khiển cục bộ của app BQHungDown (`src-tauri/src/control_api.rs`).

Cổng + khoá tự đọc từ `%APPDATA%/com.prodown.app/control_api.json` (app tự
sinh lần đầu chạy). Muốn thêm lệnh mới: thêm route bên Rust rồi thêm 1 hàm
mỏng ở đây — bot KHÔNG tự xử lý nghiệp vụ.
"""

from __future__ import annotations

import json
import os
import threading
import time
from pathlib import Path
from typing import Any, Callable

import requests

from . import net  # noqa: F401  (nạp để ép IPv4 + dùng session theo luồng)
from .net import CONNECT_TIMEOUT, session


def default_config_path() -> Path:
    base = os.environ.get("APPDATA") or str(Path.home() / ".config")
    return Path(base) / "com.prodown.app" / "control_api.json"


class AppOffline(Exception):
    """App chưa mở (hoặc cổng điều khiển chưa chạy)."""


class AppError(Exception):
    """App trả lỗi nghiệp vụ (thông báo tiếng Việt từ Rust)."""


class TTLCache:
    """Cache đơn giản theo khoá, hết hạn sau `ttl` giây. An toàn đa luồng."""

    def __init__(self, clock: Callable[[], float] = time.monotonic):
        self._data: dict[str, tuple[float, Any]] = {}
        self._lock = threading.Lock()
        self._clock = clock

    def get(self, key: str, ttl: float, load: Callable[[], Any]) -> Any:
        now = self._clock()
        with self._lock:
            hit = self._data.get(key)
            if hit and now - hit[0] < ttl:
                return hit[1]
        value = load()
        with self._lock:
            self._data[key] = (self._clock(), value)
        return value

    def clear(self, prefix: str = "") -> None:
        with self._lock:
            for k in [k for k in self._data if k.startswith(prefix)]:
                del self._data[k]


class AppClient:
    def __init__(self, config_path: Path | None = None, base_url: str | None = None,
                 token: str | None = None, transport: Callable[..., Any] | None = None):
        self.config_path = config_path or default_config_path()
        self._base_url = base_url
        self._token = token
        self._transport = transport or self._http
        self.cache = TTLCache()

    # ── cấu hình ──
    def _load_conn(self) -> tuple[str, str]:
        if self._base_url and self._token:
            return self._base_url, self._token
        try:
            cfg = json.loads(self.config_path.read_text(encoding="utf-8"))
        except (OSError, ValueError) as e:
            raise AppOffline(f"Chưa thấy {self.config_path} — mở app BQHungDown (bản có cổng điều khiển) trước.") from e
        port = int(cfg.get("port") or 47321)
        return f"http://127.0.0.1:{port}", str(cfg.get("token") or "")

    def _http(self, method: str, path: str, body: Any, timeout: float) -> tuple[int, dict]:
        base, token = self._load_conn()
        try:
            resp = session().request(method, base + path, json=body,
                                     headers={"Authorization": f"Bearer {token}"},
                                     timeout=(CONNECT_TIMEOUT, timeout))
        except requests.ConnectionError as e:
            raise AppOffline("App BQHungDown đang tắt hoặc chưa mở cổng điều khiển.") from e
        except requests.Timeout as e:
            raise AppError("App phản hồi quá lâu — thử lại sau.") from e
        except requests.RequestException as e:
            raise AppError(f"Lỗi kết nối app: {type(e).__name__}") from e
        try:
            return resp.status_code, resp.json()
        except ValueError:
            return resp.status_code, {"ok": False, "error": resp.text[:200]}

    def request(self, method: str, path: str, body: Any = None, timeout: float = 30) -> dict:
        status, data = self._transport(method, path, body, timeout)
        if status == 401:
            raise AppError("Sai khoá điều khiển — xoá control_api.json rồi mở lại app để sinh khoá mới.")
        if status != 200 or not data.get("ok", False):
            raise AppError(data.get("error") or f"Lỗi {status}")
        return data

    def get(self, path: str, timeout: float = 15) -> dict:
        return self.request("GET", path, None, timeout)

    def post(self, path: str, body: Any = None, timeout: float = 30) -> dict:
        data = self.request("POST", path, body or {}, timeout)
        # Mọi thao tác ghi làm dữ liệu cũ trong cache hết đúng.
        self.cache.clear()
        return data

    # ── đọc (có cache ngắn cho dữ liệu ít đổi) ──
    def ping(self) -> dict:
        return self.get("/api/ping", timeout=5)

    def status(self) -> dict:
        return self.get("/api/status")

    def queue(self, state: str = "all", limit: int = 200) -> dict:
        return self.get(f"/api/queue?state={state}&limit={limit}")

    def watch(self, group: str | None = None, errors_only: bool = False) -> dict:
        q = []
        if group is not None:
            q.append("group=" + requests.utils.quote(group))
        if errors_only:
            q.append("errors=1")
        path = "/api/watch" + ("?" + "&".join(q) if q else "")
        return self.cache.get("watch:" + path, 15, lambda: self.get(path))

    def cookies(self) -> dict:
        return self.cache.get("cookies", 30, lambda: self.get("/api/cookies"))

    def history(self, limit: int = 10, q: str | None = None) -> dict:
        path = f"/api/history?limit={limit}"
        if q:
            path += "&q=" + requests.utils.quote(q)
        return self.cache.get("history:" + path, 15, lambda: self.get(path))

    # ── ghi ──
    def queue_action(self, short_id: str, action: str) -> dict:
        return self.post(f"/api/queue/{short_id}/{action}")

    def bulk(self, action: str) -> dict:
        return self.post(f"/api/bulk/{action}")

    def enqueue(self, urls: list[str], audio: bool = False, folder: str | None = None) -> dict:
        # Mỗi link có thể phải phân giải (Douyin/TikTok) vài trăm ms — cho dư.
        return self.post("/api/enqueue", {"urls": urls, "audio": audio, "folder": folder},
                         timeout=30 + 5 * len(urls))

    def watch_check(self, group: str | None = None) -> dict:
        return self.post("/api/watch/check", {"group": group})

    def watch_action(self, channel_id: str, action: str, body: dict | None = None) -> dict:
        # Quét / lấy thêm phải chờ app quét xong kênh (có thể vài phút với kênh lớn).
        timeout = 900 if action in ("check", "more") else 30
        return self.post(f"/api/watch/{channel_id}/{action}", body or {}, timeout=timeout)

    def watch_add(self, url: str, tab: str = "all") -> dict:
        return self.post("/api/watch/add", {"url": url, "tab": tab}, timeout=180)

    def set_cookie(self, site: str, content: str) -> dict:
        return self.post(f"/api/cookies/{site}", {"content": content})

    def settings(self, **patch: Any) -> dict:
        return self.post("/api/settings", patch)

    def fix_engine(self) -> dict:
        return self.post("/api/engine/fix", timeout=600)
