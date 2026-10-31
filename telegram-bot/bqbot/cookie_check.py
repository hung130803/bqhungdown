"""Kiểm tra THẬT cookie YouTube lấy từ Firefox: chạy chính bộ tải yt-dlp của app
với `--cookies-from-browser firefox` trên 1 video giới hạn tuổi (18+), không tải
file. Lấy được thông tin video = Firefox còn đăng nhập, app tải được video 18+.

Bot KHÔNG tự đọc kho cookie của Firefox — việc đó do yt-dlp làm, y như lúc app
tải thật. Kết quả lưu lại để màn hình Cookie hiện ngay, khỏi chạy lại mỗi lần.
"""

from __future__ import annotations

import os
import subprocess
import threading
import time
from dataclasses import dataclass
from pathlib import Path

# Video công khai nhưng GIỚI HẠN TUỔI — chỉ xem được khi đã đăng nhập tài khoản 18+.
AGE_TEST_URL = "https://www.youtube.com/watch?v=mye1epXXk8c"
CHECK_TIMEOUT = 90

OK, LOGGED_OUT, NO_FIREFOX, NO_TOOL, ERROR = "ok", "logged_out", "no_firefox", "no_tool", "error"


@dataclass
class CheckResult:
    status: str
    at: float
    detail: str = ""

    @property
    def ok(self) -> bool:
        return self.status == OK


def find_ytdlp() -> str | None:
    """Bộ tải app đang dùng (bản tự cập nhật trong %APPDATA%), không có thì bản đóng gói."""
    appdata = os.environ.get("APPDATA")
    cands = []
    if appdata:
        cands.append(Path(appdata) / "com.prodown.app" / "bin" / "yt-dlp.exe")
    root = Path(__file__).resolve().parents[2]
    cands.append(root / "src-tauri" / "binaries" / "yt-dlp-x86_64-pc-windows-msvc.exe")
    return next((str(p) for p in cands if p.is_file()), None)


def classify(returncode: int, out: str) -> tuple[str, str]:
    """Đọc kết quả yt-dlp → (trạng thái, câu giải thích ngắn)."""
    low = out.lower()
    if returncode == 0 and "|" in out:
        return OK, "Firefox đang đăng nhập YouTube — tải được cả video 18+."
    if "could not find firefox" in low or "firefox cookies database" in low:
        return NO_FIREFOX, "Không thấy Firefox (hoặc chưa mở Firefox lần nào) trên máy tính."
    if "not a bot" in low or "http error 429" in low:
        return ERROR, "YouTube đang chặn tạm (kiểm tra robot) — không phải lỗi cookie, lát nữa tự kiểm tra lại."
    if ("sign in to confirm your age" in low or "no longer valid" in low
            or "sign in to confirm" in low or "login required" in low):
        return LOGGED_OUT, "Firefox đã bị đăng xuất YouTube (hoặc phiên hết hạn)."
    tail = out.strip().splitlines()[-1] if out.strip() else f"mã thoát {returncode}"
    return ERROR, tail[:200]


def run_check(ytdlp: str | None = None, runner=subprocess.run) -> CheckResult:
    exe = ytdlp or find_ytdlp()
    now = time.time()
    if not exe:
        return CheckResult(NO_TOOL, now, "Không thấy bộ tải yt-dlp của app.")
    cmd = [exe, "--no-update", "--cookies-from-browser", "firefox", "--skip-download",
           "--ignore-no-formats-error", "--no-warnings", "--print", "%(id)s|%(age_limit)s", AGE_TEST_URL]
    try:
        r = runner(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace",
                   timeout=CHECK_TIMEOUT, creationflags=0x08000000 if os.name == "nt" else 0)
    except subprocess.TimeoutExpired:
        return CheckResult(ERROR, now, "Kiểm tra quá lâu (mạng chậm?) — thử lại sau.")
    except OSError as e:
        return CheckResult(NO_TOOL, now, f"Không chạy được bộ tải: {e}")
    status, detail = classify(r.returncode, (r.stdout or "") + "\n" + (r.stderr or ""))
    return CheckResult(status, now, detail)


class CookieChecker:
    """Giữ kết quả lần kiểm tra gần nhất; chỉ 1 lượt kiểm tra chạy cùng lúc."""

    def __init__(self, check=run_check):
        self._check = check
        self._lock = threading.Lock()
        self._busy = threading.Lock()
        self.last: CheckResult | None = None

    def check_now(self) -> CheckResult | None:
        """Chạy kiểm tra (chặn tới khi xong). Đang có lượt khác chạy → trả None."""
        if not self._busy.acquire(blocking=False):
            return None
        try:
            res = self._check()
            with self._lock:
                self.last = res
            return res
        finally:
            self._busy.release()

    def wait_result(self, timeout: float = 120) -> CheckResult | None:
        """Kiểm tra ngay; nếu đang có lượt khác chạy thì ĐỢI lượt đó xong rồi
        trả kết quả của nó (không chạy chồng thêm 1 lượt nữa)."""
        res = self.check_now()
        if res is not None:
            return res
        if self._busy.acquire(timeout=timeout):
            self._busy.release()
        return self.latest()

    def latest(self) -> CheckResult | None:
        with self._lock:
            return self.last
