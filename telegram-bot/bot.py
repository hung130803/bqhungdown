"""Khởi chạy bot Telegram điều khiển BQHungDown.

    python bot.py            # chạy bot
    python bot.py --check    # chỉ kiểm tra kết nối app + Telegram rồi thoát
"""

from __future__ import annotations

import logging
import logging.handlers
import os
import signal
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from bqbot import config as config_mod  # noqa: E402
from bqbot.app_client import AppClient, AppError, AppOffline  # noqa: E402
from bqbot.dispatcher import Dispatcher  # noqa: E402
from bqbot.handlers import BOT_COMMANDS, Router  # noqa: E402
from bqbot.monitor import Monitor  # noqa: E402
from bqbot.net import TelegramAPI, TelegramError  # noqa: E402


class RedactToken(logging.Filter):
    """Che token bot trong MỌI dòng log (kể cả traceback của thư viện)."""

    def __init__(self, token: str):
        super().__init__()
        self.token = token

    def filter(self, record: logging.LogRecord) -> bool:
        if self.token:
            msg = record.getMessage()
            if self.token in msg:
                record.msg, record.args = msg.replace(self.token, "***"), ()
            if record.exc_info:
                text = logging.Formatter().formatException(record.exc_info)
                record.exc_info, record.exc_text = None, text.replace(self.token, "***")
        return True


def setup_logging(token: str = "") -> None:
    logs = HERE / "logs"
    logs.mkdir(exist_ok=True)
    fmt = logging.Formatter("%(asctime)s %(levelname)s %(name)s: %(message)s")
    fh = logging.handlers.RotatingFileHandler(logs / "bot.log", maxBytes=2_000_000, backupCount=3, encoding="utf-8")
    sh = logging.StreamHandler()
    for h in (fh, sh):
        h.setFormatter(fmt)
        h.addFilter(RedactToken(token))
    logging.basicConfig(level=logging.INFO, handlers=[fh, sh], force=True)
    logging.getLogger("urllib3").setLevel(logging.WARNING)


def main() -> int:
    try:
        cfg = config_mod.load(HERE / "config.json")
    except config_mod.ConfigError as e:
        setup_logging()
        logging.getLogger("bqbot").error("%s", e)
        return config_mod.CONFIG_EXIT_CODE
    setup_logging(cfg.telegram_token)
    log = logging.getLogger("bqbot")
    tg = TelegramAPI(cfg.telegram_token)
    app = AppClient(Path(cfg.app_config_path) if cfg.app_config_path else None)

    try:
        me = tg.call("getMe")
    except TelegramError as e:
        if e.code in (401, 404):
            log.error("Token bot sai (Telegram báo %s) — sửa telegram_token trong config.json", e.code)
            return config_mod.CONFIG_EXIT_CODE
        log.error("Không kết nối được Telegram: %s", e)
        return 1
    log.info("Bot @%s sẵn sàng", me.get("username"))
    try:
        log.info("App BQHungDown %s đang chạy", app.ping().get("version"))
    except (AppOffline, AppError) as e:
        log.warning("Chưa kết nối được app: %s (bot vẫn chạy, sẽ tự nối lại)", e)
    if "--check" in sys.argv:
        return 0
    if not cfg.allowed_chat_ids:
        log.warning("allowed_chat_ids đang trống — nhắn bất kỳ cho bot để lấy mã chat rồi điền vào config.json")

    for method, data in (
        # Token từng gắn webhook thì getUpdates bị 409 mãi — gỡ đi (giữ tin đang chờ).
        ("deleteWebhook", {"drop_pending_updates": False}),
        ("setMyCommands", {"commands": [{"command": c, "description": d} for c, d in BOT_COMMANDS]}),
    ):
        try:
            tg.call(method, data)
        except TelegramError as e:
            log.info("%s lỗi: %s", method, e)

    monitor = Monitor(tg, app, cfg)
    router = Router(tg, app, cfg, monitor)
    disp = Dispatcher(tg, router, workers=cfg.workers)
    router.dispatcher = disp

    def stop(*_a):
        log.info("Đang tắt bot…")
        disp.shutdown()

    signal.signal(signal.SIGINT, stop)
    signal.signal(signal.SIGTERM, stop)
    disp.start_polling()
    disp.run(tick=monitor.tick, tick_every=cfg.poll_interval_sec)
    log.info("Bot đã dừng.")
    logging.shutdown()
    # Không chờ việc dài đang dở (gửi file 49 MB, sửa bộ tải…) — Python mặc
    # định join mọi luồng pool khi thoát, Ctrl+C có thể treo tới 10 phút.
    os._exit(0)


if __name__ == "__main__":
    sys.exit(main())
