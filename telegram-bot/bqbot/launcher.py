"""Bot tự đăng ký với app + chỉ chạy 1 bản.

- `register()`: ghi `<APPDATA>/com.prodown.app/telegram_bot.json` (thư mục bot,
  pythonw.exe). App đọc file này mỗi lần mở để TỰ BẬT bot chạy ngầm
  (`src-tauri/src/bot_launcher.rs`). Tắt tự bật: sửa "enabled": false.
- `acquire_single_instance()`: khoá file `logs/bot.lock` — bản thứ hai thoát
  ngay với mã ALREADY_RUNNING (2 bot cùng token → Telegram báo lỗi 409).
"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

ALREADY_RUNNING = 4
REG_NAME = "telegram_bot.json"


def registration_path(app_config_dir: Path | None = None) -> Path:
    if app_config_dir is None:
        base = os.environ.get("APPDATA") or str(Path.home() / ".config")
        app_config_dir = Path(base) / "com.prodown.app"
    return app_config_dir / REG_NAME


def pythonw_for(python_exe: str) -> str:
    """pythonw.exe cạnh python.exe (chạy không cửa sổ). Không có thì dùng chính nó."""
    p = Path(python_exe)
    cand = p.with_name("pythonw.exe") if os.name == "nt" else p
    return str(cand if cand.is_file() else p)


def register(bot_dir: Path, app_config_dir: Path | None = None, python_exe: str | None = None) -> Path | None:
    """Ghi/cập nhật đăng ký. GIỮ nguyên "enabled" người dùng đã tắt."""
    path = registration_path(app_config_dir)
    try:
        old = json.loads(path.read_text(encoding="utf-8-sig")) if path.exists() else {}
    except (OSError, ValueError):
        old = {}
    data = {
        "enabled": bool(old.get("enabled", True)),
        "dir": str(bot_dir),
        "python": pythonw_for(python_exe or sys.executable),
        "script": "bot.py",
    }
    if data == old:
        return path
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        tmp = path.with_suffix(".json.tmp")
        tmp.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")
        os.replace(tmp, path)
        return path
    except OSError:
        return None  # app chưa từng chạy / không ghi được — bot vẫn chạy bình thường


def find_app(configured: str | None = None) -> str | None:
    """Đường dẫn app để bot MỞ APP từ điện thoại.

    Có `app_exe` trong config thì dùng đúng nó. Không thì chọn bản MỚI NHẤT (file
    sửa gần nhất) giữa bản cài và bản build — bản cài cũ chưa cập nhật không có
    cổng điều khiển, mở nó thì bot không điều khiển được.
    """
    if configured:
        return configured if Path(configured).is_file() else None
    cands = []
    local = os.environ.get("LOCALAPPDATA")
    if local:
        cands.append(Path(local) / "BQHungDown" / "bqhungdown.exe")
    cands.append(Path(__file__).resolve().parents[2] / "src-tauri" / "target" / "release" / "bqhungdown.exe")
    found = [p for p in cands if p.is_file()]
    if not found:
        return None
    return str(max(found, key=lambda p: p.stat().st_mtime))


def open_app(exe: str) -> None:
    """Mở app TÁCH RỜI khỏi bot (bot tắt/khởi động lại không kéo app theo)."""
    import subprocess
    flags = 0
    if os.name == "nt":
        flags = 0x00000008 | 0x00000200  # DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
    subprocess.Popen([exe], cwd=str(Path(exe).parent), creationflags=flags, close_fds=True,
                     stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def force_kill_app() -> bool:
    """Buộc tắt app (chỉ dùng khi app treo / bản cũ không có lệnh đóng êm)."""
    import subprocess
    if os.name != "nt":
        return False
    r = subprocess.run(["taskkill", "/IM", "bqhungdown.exe", "/F", "/T"], capture_output=True,
                       creationflags=0x08000000)
    return r.returncode == 0


class SingleInstance:
    """Giữ khoá suốt đời tiến trình; tiến trình chết là hệ điều hành tự nhả."""

    def __init__(self, lock_path: Path):
        self.lock_path = lock_path
        self._fh = None

    def acquire(self) -> bool:
        self.lock_path.parent.mkdir(parents=True, exist_ok=True)
        fh = open(self.lock_path, "a+")
        try:
            if os.name == "nt":
                import msvcrt
                fh.seek(0)
                msvcrt.locking(fh.fileno(), msvcrt.LK_NBLCK, 1)
            else:
                import fcntl
                fcntl.flock(fh.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError:
            fh.close()
            return False
        self._fh = fh
        return True

    def release(self) -> None:
        if self._fh is not None:
            try:
                self._fh.close()
            finally:
                self._fh = None
