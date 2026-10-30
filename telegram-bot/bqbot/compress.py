"""Nén video thành bản nhẹ (< giới hạn 50 MB của Bot API) để XEM trên điện thoại.

File gốc trên máy tính GIỮ NGUYÊN; bản nén nằm ở thư mục tạm và xoá sau khi gửi.
Dùng chính ffmpeg đi kèm app (đường dẫn lấy từ /api/status), không cài thêm gì.
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

AUDIO_KBPS = 96
# Dưới mức này hình quá vỡ — báo không gửi được thay vì gửi bản không xem nổi.
MIN_VIDEO_KBPS = 150
CREATE_NO_WINDOW = 0x08000000 if os.name == "nt" else 0


class CompressError(Exception):
    pass


def find_ffmpeg(from_app: str | None) -> str | None:
    """ffmpeg của app → bản trong thư mục dự án → PATH."""
    if from_app and Path(from_app).is_file():
        return from_app
    repo_bin = Path(__file__).resolve().parents[2] / "src-tauri" / "binaries"
    for p in sorted(repo_bin.glob("ffmpeg*.exe")) if repo_bin.is_dir() else []:
        return str(p)
    return shutil.which("ffmpeg")


def parse_duration(ffmpeg_stderr: str) -> float | None:
    m = re.search(r"Duration:\s*(\d+):(\d+):(\d+(?:\.\d+)?)", ffmpeg_stderr)
    if not m:
        return None
    h, mi, s = m.groups()
    return int(h) * 3600 + int(mi) * 60 + float(s)


def plan(duration_s: float, target_mb: float) -> tuple[int, int] | None:
    """(video_kbps, chiều cao) để file ra ~target_mb. None = video quá dài."""
    total_kbps = target_mb * 8192 / max(duration_s, 1)
    video_kbps = int(total_kbps - AUDIO_KBPS)
    if video_kbps < MIN_VIDEO_KBPS:
        return None
    height = 720 if video_kbps >= 1500 else 480 if video_kbps >= 500 else 360
    return video_kbps, height


def compress(src: str, ffmpeg: str, target_mb: float = 45, timeout: int = 1800) -> str:
    """Trả về đường dẫn file nén (thư mục tạm — nơi gọi tự xoá)."""
    probe = subprocess.run([ffmpeg, "-hide_banner", "-i", src], capture_output=True, text=True,
                           encoding="utf-8", errors="replace", creationflags=CREATE_NO_WINDOW)
    duration = parse_duration(probe.stderr)
    if not duration:
        raise CompressError("không đọc được thời lượng video")
    p = plan(duration, target_mb)
    if p is None:
        raise CompressError(f"video dài {duration / 60:.0f} phút — nén xuống dưới 50 MB sẽ vỡ hình, "
                            "Telegram không cho bot gửi file lớn hơn")
    vk, h = p
    out = os.path.join(tempfile.mkdtemp(prefix="bqbot-"), Path(src).stem[:80] + " (xem thu).mp4")
    cmd = [ffmpeg, "-hide_banner", "-loglevel", "error", "-y", "-i", src,
           "-vf", f"scale=-2:'min({h},ih)'", "-c:v", "libx264", "-preset", "veryfast",
           "-b:v", f"{vk}k", "-maxrate", f"{vk}k", "-bufsize", f"{vk * 2}k",
           "-c:a", "aac", "-b:a", f"{AUDIO_KBPS}k", "-movflags", "+faststart", out]
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace",
                           timeout=timeout, creationflags=CREATE_NO_WINDOW)
    except subprocess.TimeoutExpired:
        cleanup(out)
        raise CompressError("nén quá lâu, đã huỷ") from None
    if r.returncode != 0 or not os.path.isfile(out):
        cleanup(out)
        raise CompressError("ffmpeg lỗi: " + (r.stderr or "").strip()[-200:])
    return out


def cleanup(path: str) -> None:
    try:
        shutil.rmtree(os.path.dirname(path), ignore_errors=True)
    except OSError:
        pass
