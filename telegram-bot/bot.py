"""Khởi chạy bot Telegram điều khiển BQHungDown.

    python bot.py            # chạy bot
    python bot.py --check    # chỉ kiểm tra kết nối app + Telegram rồi thoát
"""

from __future__ import annotations

import logging
import logging.handlers
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
from bqbot.net import TelegramAPI  # noqa: E402


def setup_logging() -> None:
    logs = HERE / "logs"
    logs.mkdir(exist_ok=True)
    fmt = logging.Formatter("%(asctime)s %(levelname)s %(name)s: %(message)s")
    fh = logging.handlers.RotatingFileHandler(logs / "bot.log", maxBytes=2_000_000, backupCount=3, encoding="utf-8")
    fh.setFormatter(fmt)
    sh = logging.StreamHandler()
    sh.setFormatter(fmt)
    logging.basicConfig(level=logging.INFO, handlers=[fh, sh])
    logging.getLogger("urllib3").setLevel(logging.WARNING)


def main() -> int:
    setup_logging()
    log = logging.getLogger("bqbot")
    cfg = config_mod.load(HERE / "config.json")
    tg = TelegramAPI(cfg.telegram_token)
    app = AppClient(Path(cfg.app_config_path) if cfg.app_config_path else None)

    me = tg.call("getMe")
    log.info("Bot @%s sẵn sàng", me.get("username"))
    try:
        log.info("App BQHungDown %s đang chạy", app.ping().get("version"))
    except (AppOffline, AppError) as e:
        log.warning("Chưa kết nối được app: %s (bot vẫn chạy, sẽ tự nối lại)", e)
    if "--check" in sys.argv:
        return 0
    if not cfg.allowed_chat_ids:
        log.warning("allowed_chat_ids đang trống — nhắn /id cho bot để lấy mã chat rồi điền vào config.json")

    try:
        tg.call("setMyCommands", {"commands": [{"command": c, "description": d} for c, d in BOT_COMMANDS]})
    except Exception as e:  # noqa: BLE001
        log.info("setMyCommands lỗi: %s", e)

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
    return 0


if __name__ == "__main__":
    sys.exit(main())
