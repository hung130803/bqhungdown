"""Bộ điều phối: nhận update, chia việc nhẹ/nặng, chạy nền, đánh thức tức thì.

- 1 luồng RIÊNG chỉ nghe getUpdates (long polling) → đẩy vào hàng + set Event.
- Vòng chính chờ `threading.Event` (không sleep cố định) và CHỈ ĐỊNH TUYẾN —
  không bao giờ tự gọi mạng (một lần gửi tin dính 429 chờ 30 s cũng không làm
  nút khác quay vòng).
- Lệnh NHẸ (chỉ soạn chữ) → luồng trả lời nhanh, chạy ngay.
- Lệnh NẶNG (gọi app…) → ThreadPool tối đa 3 luồng. Cùng 1 đối tượng (`key`,
  vd 1 mục tải / 1 kênh / 1 chat) thì xếp hàng làm LẦN LƯỢT — việc đang chờ
  khoá KHÔNG chiếm luồng của pool.
- Bấm nút: answerCallbackQuery "⏳ Đang làm..." NGAY ở luồng riêng rồi mới
  làm việc thật.
- Việc chờ lâu (sửa bộ tải, thêm kênh, gửi file) chạy ở pool RIÊNG, gửi đi
  rồi KHÔNG chờ; vòng chính kiểm tra future nào xong thì báo kết quả. Nhờ
  pool riêng, việc dài không làm nghẽn lệnh thường hay nhịp theo dõi.
- Nhịp theo dõi (`tick`) có luồng riêng, không chồng lượt.
- Mọi lệnh mất quá 3 giây (tính cả thời gian xếp hàng) được ghi log.
"""

from __future__ import annotations

import collections
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
        # Trả lời nút + lệnh nhẹ + báo kết quả việc dài — không xếp sau việc nặng.
        self.fast = ThreadPoolExecutor(max_workers=2, thread_name_prefix="fast")
        self.jobs_pool = ThreadPoolExecutor(max_workers=3, thread_name_prefix="job")
        self.tick_pool = ThreadPoolExecutor(max_workers=1, thread_name_prefix="tick")
        self._tick_running = threading.Event()
        # Hàng chờ theo key: key có mặt = đang có việc chạy; deque = việc chờ sau.
        self._keyed: dict[str, collections.deque] = {}
        self._keyed_guard = threading.Lock()
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
            except Exception as e:  # noqa: BLE001  mạng chập chờn — chờ tăng dần, tối đa 30s
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

    # ── chạy có đo giờ ──
    def _timed(self, name: str, fn: Callable[[], Any], since: float | None = None) -> Any:
        t0 = self.clock() if since is None else since
        try:
            return fn()
        except Exception:  # noqa: BLE001
            log.exception("Lệnh %s lỗi", name)
            return None
        finally:
            dt = self.clock() - t0
            if dt > SLOW_SEC:
                log.warning("Lệnh chậm %.1fs: %s", dt, name)

    def _submit(self, executor: ThreadPoolExecutor, fn: Callable[[], Any]) -> Future | None:
        try:
            return executor.submit(fn)
        except RuntimeError:  # đã shutdown
            return None

    # ── điều phối ──
    def run_task(self, task: Task | None) -> Future | None:
        if task is None:
            return None
        queued_at = self.clock()
        if not task.heavy:
            return self._submit(self.fast, lambda: self._timed(task.name, task.fn, queued_at))
        if not task.key:
            return self._submit(self.pool, lambda: self._timed(task.name, task.fn, queued_at))
        done: Future = Future()

        def work():
            try:
                done.set_result(self._timed(task.name, task.fn, queued_at))
            finally:
                self._next_for_key(task.key)

        with self._keyed_guard:
            waiting = self._keyed.get(task.key)
            if waiting is not None:  # key đang bận → xếp hàng, KHÔNG chiếm luồng
                waiting.append(work)
                return done
            self._keyed[task.key] = collections.deque()
        if self._submit(self.pool, work) is None:
            with self._keyed_guard:
                self._keyed.pop(task.key, None)
        return done

    def _next_for_key(self, key: str) -> None:
        with self._keyed_guard:
            waiting = self._keyed.get(key)
            if not waiting:
                self._keyed.pop(key, None)  # hết việc → bỏ key (không phình bộ nhớ)
                return
            nxt = waiting.popleft()
        if self._submit(self.pool, nxt) is None:
            with self._keyed_guard:
                self._keyed.pop(key, None)

    def handle_update(self, u: dict) -> Future | None:
        if self.stop.is_set():
            return None
        try:
            cb = u.get("callback_query")
            if cb:
                # Trả lời nút NGAY (luồng riêng) rồi mới làm việc thật.
                self._submit(self.fast, lambda: self._answer(cb.get("id")))
                return self.run_task(self.router.route_callback(cb))
            msg = u.get("message")
            if msg:
                return self.run_task(self.router.route_message(msg))
        except Exception:  # noqa: BLE001  1 update hỏng không được làm chết vòng chính
            log.exception("Xử lý update lỗi")
        return None

    def _answer(self, callback_id: str | None) -> None:
        if not callback_id:
            return
        try:
            self.tg.answer_callback(callback_id, "⏳ Đang làm...")
        except Exception as e:  # noqa: BLE001
            log.info("answerCallbackQuery lỗi: %s", e)

    # ── việc chờ lâu: gửi rồi đi, vòng chính kiểm tra cái nào xong ──
    def submit_job(self, name: str, fn: Callable[[], Any],
                   on_done: Callable[[Any, BaseException | None], None]) -> None:
        fut = self._submit(self.jobs_pool, fn)
        if fut is None:
            return
        with self._jobs_guard:
            self._jobs.append(_Job(fut, on_done, name))
        fut.add_done_callback(lambda _f: self.wake.set())

    def collect_jobs(self) -> int:
        with self._jobs_guard:
            done = [j for j in self._jobs if j.future.done()]
            self._jobs = [j for j in self._jobs if not j.future.done()]
        for j in done:
            if j.future.cancelled():  # bot đang tắt
                continue
            err = j.future.exception()
            res = None if err else j.future.result()
            dt = self.clock() - j.started
            if dt > SLOW_SEC:
                log.info("Việc dài %s xong sau %.1fs", j.name, dt)
            # Báo kết quả = gửi tin → chạy ở luồng nhanh, không chặn vòng chính.
            self._submit(self.fast, lambda j=j, res=res, err=err:
                         self._timed("báo kết quả " + j.name, lambda: j.on_done(res, err)))
        return len(done)

    def pending_jobs(self) -> int:
        with self._jobs_guard:
            return len(self._jobs)

    def drain(self) -> int:
        n = 0
        while not self.stop.is_set():
            try:
                u = self.updates.get_nowait()
            except queue.Empty:
                break
            self.handle_update(u)
            n += 1
        return n

    def _start_tick(self, tick: Callable[[], None]) -> None:
        if self._tick_running.is_set():  # lượt trước chưa xong → bỏ lượt này
            return
        self._tick_running.set()

        def run():
            try:
                self._timed("tick", tick)
            finally:
                self._tick_running.clear()

        if self._submit(self.tick_pool, run) is None:
            self._tick_running.clear()

    def run(self, tick: Callable[[], None] | None = None, tick_every: float = 5.0) -> None:
        next_tick = self.clock()
        while not self.stop.is_set():
            timeout = max(0.0, next_tick - self.clock()) if tick else None
            self.wake.wait(timeout)
            self.wake.clear()
            if self.stop.is_set():
                break
            self.drain()
            self.collect_jobs()
            if tick and self.clock() >= next_tick:
                next_tick = self.clock() + tick_every
                self._start_tick(tick)

    def shutdown(self) -> None:
        self.stop.set()
        self.wake.set()
        for ex in (self.pool, self.fast, self.jobs_pool, self.tick_pool):
            ex.shutdown(wait=False, cancel_futures=True)
