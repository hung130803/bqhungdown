"""Đọc cấu hình bot từ `telegram-bot/config.json` (+ biến môi trường)."""

from __future__ import annotations

import json
import os
from dataclasses import dataclass, field
from pathlib import Path


# Mã thoát riêng cho lỗi cấu hình — run-bot.bat thấy mã này thì DỪNG, không
# tự chạy lại vô hạn.
CONFIG_EXIT_CODE = 3


class ConfigError(Exception):
    pass


@dataclass
class Config:
    telegram_token: str
    allowed_chat_ids: list[int]
    notify_chat_ids: list[int] = field(default_factory=list)
    app_config_path: str | None = None
    poll_interval_sec: float = 5
    digest_interval_sec: float = 20
    board_interval_sec: float = 10
    notify_completed: bool = True
    notify_failed: bool = True
    notify_new: bool = True
    # Bot API thường chỉ gửi được file ≤ 50 MB.
    max_send_file_mb: int = 49
    # Đường dẫn app để nút "Mở app" bấm từ điện thoại; trống = tự tìm bản cài.
    app_exe: str | None = None
    workers: int = 3


def load(path: Path) -> Config:
    raw: dict = {}
    if path.exists():
        # utf-8-sig: Notepad hay lưu kèm BOM — json.loads sẽ hỏng nếu không bỏ.
        try:
            raw = json.loads(path.read_text(encoding="utf-8-sig"))
        except ValueError as e:
            raise ConfigError(f"{path} sai định dạng JSON: {e}") from None
    token = os.environ.get("BQBOT_TOKEN") or raw.get("telegram_token") or ""
    if not token or token.startswith("<"):
        raise ConfigError(f"Thiếu telegram_token trong {path} (lấy từ @BotFather) hoặc biến BQBOT_TOKEN.")
    allowed = [int(x) for x in raw.get("allowed_chat_ids") or []]
    known = set(Config.__dataclass_fields__)
    extra = {k: v for k, v in raw.items() if k in known and k not in ("telegram_token", "allowed_chat_ids")}
    cfg = Config(telegram_token=token, allowed_chat_ids=allowed, **extra)
    cfg.notify_chat_ids = [int(x) for x in (cfg.notify_chat_ids or allowed)]
    cfg.workers = max(1, min(3, int(cfg.workers)))
    return cfg
