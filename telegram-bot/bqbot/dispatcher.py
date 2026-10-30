"""Bộ điều phối: nhận update, chia việc nhẹ/nặng, chạy nền, đánh thức tức thì.

- 1 luồng RIÊNG chỉ nghe getUpdates (long polling) → đẩy vào hàng + set Event.
- Vòng chính chờ `threading.Event` (không sleep cố định): có update/việc xong
  là thức ngay; không có gì thì thức theo nhịp `tick` (theo dõi tiến trình).
- Lệnh NHẸ (chỉ soạn chữ) chạy ngay ở vòng chính; lệnh NẶNG (gọi app, tải
  file…) đẩy sang ThreadPool tối đa 3 luồng. Cùng 1 đối tượng (khoá `key`, vd
  1 mục tải / 1 kênh) thì làm LẦN LƯỢT.
- Bấm nút: answerCallbackQuery "⏳ Đang làm..." NGAY ở luồng nền riêng rồi mới
  làm việc thật — nút không bao giờ quay vòng chờ việc nặng.
- Việc chờ lâu (sửa bộ tải, thêm kênh…) gửi đi rồi KHÔNG chờ: `submit_job`
  trả về ngay; vòng chính đi kiểm tra future nào xong thì báo kết quả.
- Mọi lệnh xử lý quá 3 giây được ghi log.
"""

from __future__ import annotations

import logging
import queue
import threading
import time
from concurrent.futures import Future, ThreadPoolExecutor
from dataclasses import dataclass, field
from typing import Any, Callable

log = logging.getLogger("bqbot.dispatch")

SLOW_SEC = 3.0


@dataclass
class Task:
    fn: Callable[[], Any]
    name: str
    heavy: bool = False
    key: str | None = None  # cùng key → làm lần lượt


@dataclass
class _Job:
    future: Future
    on_done: Callable[[Any, BaseException | None], None]
    name: str
    started: float = field(default_factory=time.monotonic)


class Dispatcher:
    def __init__(self, tg, router, workers: int = 3, clock: Callable[[], float] = time.monotonic):
        self.tg = tg
        self.router = router  # có route_message(msg) / route_callback(cb) -> Task | None
        self.clock = clock
        self.wake = threading.Event()
        self.stop = threading.Event()
        self.updates: "queue.Queue[dict]" = queue.Queue()
        self.pool = ThreadPoolExecutor(max_workers=workers, thread_name_prefix="heavy")
        # Luồng riêng cho answerCallbackQuery — không bao giờ xếp sau việc nặng.
        self.fast = ThreadPoolExecutor(max_workers=2, thread_name_prefix="answer")
        self._locks: dict[str, threading.Lock] = {}
        self._locks_guard = threading.Lock()
        self._jobs: list[_Job] = []
        self._jobs_guard = threading.Lock()
        self.offset = 0

    # ── nhận update ──
    def poll_forever(self) -> None:
        backoff = 1.0
        while not self.stop.is_set():
            try:
                ups = self.tg.get_updates(self.offset, timeout=50)
                backoff = 1.0
            except Exception as e:  # mạng chập chờn — chờ tăng dần, tối đa 30s
                log.warning("getUpdates lỗi: %s", e)
                self.stop.wait(backoff)
                backoff = min(backoff * 2, 30)
                continue
            for u in ups:
                self.offset = max(self.offset, int(u.get("update_id", 0)) + 1)
                self.updates.put(u)
            if ups:
                self.wake.set()

    def start_polling(self) -> threading.Thread:
        t = threading.Thread(target=self.poll_forever, name="poll", daemon=True)
        t.start()
        return t

    # ── điều phối ──
    def lock_for(self, key: str) -> threading.Lock:
        with self._locks_guard:
            lk = self._locks.get(key)
            if lk is None:
                lk = self._locks[key] = threading.Lock()
            return lk

    def _timed(self, name: str, fn: Callable[[], Any]) -> Any:
        t0 = self.clock()
        try:
            return fn()
        except Exception:
            log.exception("Lệnh %s lỗi", name)
            return None
        finally:
            dt = self.clock() - t0
            if dt > SLOW_SEC:
                log.warning("Lệnh chậm %.1fs: %s", dt, name)

    def run_task(self, task: Task | None) -> Future | None:
        if task is None:
            return None
        if not task.heavy:
            self._timed(task.name, task.fn)
            return None

        def work():
            if task.key:
                with self.lock_for(task.key):
                    return self._timed(task.name, task.fn)
            return self._timed(task.name, task.fn)

        return self.pool.submit(work)

    def handle_update(self, u: dict) -> Future | None:
        cb = u.get("callback_query")
        if cb:
            # Trả lời nút NGAY (luồng nền riêng) rồi mới làm việc thật.
            self.fast.submit(self._answer, cb.get("id"))
            return self.run_task(self.router.route_callback(cb))
        msg = u.get("message")
        if msg:
            return self.run_task(self.router.route_message(msg))
        return None

    def _answer(self, callback_id: str | None) -> None:
        if not callback_id:
            return
        try:
            self.tg.answer_callback(callback_id, "⏳ Đang làm...")
        except Exception as e:
            log.info("answerCallbackQuery lỗi: %s", e)

    # ── việc chờ lâu: gửi rồi đi, vòng chính kiểm tra cái nào xong ──
    def submit_job(self, name: str, fn: Callable[[], Any],
                   on_done: Callable[[Any, BaseException | None], None]) -> None:
        fut = self.pool.submit(fn)
        with self._jobs_guard:
            self._jobs.append(_Job(fut, on_done, name))
        fut.add_done_callback(lambda _f: self.wake.set())

    def collect_jobs(self) -> int:
        with self._jobs_guard:
            done = [j for j in self._jobs if j.future.done()]
            self._jobs = [j for j in self._jobs if not j.future.done()]
        for j in done:
            err = j.future.exception()
            res = None if err else j.future.result()
            dt = self.clock() - j.started
            if dt > SLOW_SEC:
                log.info("Việc dài %s xong sau %.1fs", j.name, dt)
            self._timed("báo kết quả " + j.name, lambda j=j, res=res, err=err: j.on_done(res, err))
        return len(done)

    def pending_jobs(self) -> int:
        with self._jobs_guard:
            return len(self._jobs)

    def drain(self) -> int:
        n = 0
        while True:
            try:
                u = self.updates.get_nowait()
            except queue.Empty:
                return n
            self.handle_update(u)
            n += 1

    def run(self, tick: Callable[[], None] | None = None, tick_every: float = 5.0) -> None:
        next_tick = self.clock()
        while not self.stop.is_set():
            timeout = max(0.0, next_tick - self.clock()) if tick else None
            self.wake.wait(timeout)
            self.wake.clear()
            self.drain()
            self.collect_jobs()
            if tick and self.clock() >= next_tick:
                next_tick = self.clock() + tick_every
                # Tick chạy nền, KHÔNG chồng lượt (lượt trước chưa xong thì bỏ).
                lk = self.lock_for("__tick__")
                if lk.acquire(blocking=False):
                    def run_tick(lk=lk):
                        try:
                            self._timed("tick", tick)
                        finally:
                            lk.release()
                    self.pool.submit(run_tick)

    def shutdown(self) -> None:
        self.stop.set()
        self.wake.set()
        self.pool.shutdown(wait=False, cancel_futures=True)
        self.fast.shutdown(wait=False, cancel_futures=True)
