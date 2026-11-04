"""Định dạng tin nhắn — TOÀN hàm thuần (không gọi mạng) để test dễ."""

from __future__ import annotations

import html
import re
import time
from datetime import datetime, timezone

STATE_ICON = {
    "queued": "⏳", "downloading": "⬇️", "paused": "⏸", "completed": "✅",
    "failed": "❌", "cancelled": "🚫", "skipped": "⏭",
}
STATE_NAME = {
    "queued": "Đang chờ", "downloading": "Đang tải", "paused": "Tạm dừng", "completed": "Xong",
    "failed": "Lỗi", "cancelled": "Đã huỷ", "skipped": "Bỏ qua",
}


def esc(s: object) -> str:
    return html.escape(str(s if s is not None else ""), quote=False)


def short(s: str | None, n: int = 60) -> str:
    s = (s or "").strip().replace("\n", " ")
    return s if len(s) <= n else s[: n - 1] + "…"


def size(b: float | int | None) -> str:
    if not b:
        return "0 B"
    b = float(b)
    for unit in ("B", "KB", "MB", "GB"):
        if b < 1024 or unit == "GB":
            return f"{b:.0f} {unit}" if unit == "B" else f"{b:.1f} {unit}"
        b /= 1024
    return f"{b:.1f} GB"


def speed(bps: float | None) -> str:
    return f"{size(bps)}/s" if bps else "—"


def eta(sec: int | None) -> str:
    if not sec:
        return "—"
    m, s = divmod(int(sec), 60)
    h, m = divmod(m, 60)
    return f"{h}g{m:02d}p" if h else f"{m}p{s:02d}s"


def bar(pct: float | None, width: int = 10) -> str:
    """Thanh tiến độ bằng ô màu — khối '█' bị Telegram vẽ thành vạch đen đặc khó nhìn."""
    if pct is None:
        return "⬜" * width
    full = max(0, min(width, round(pct / 100 * width)))
    return "🟩" * full + "⬜" * (width - full)


def ago(iso: str | None, now: float | None = None) -> str:
    if not iso:
        return "chưa"
    try:
        t = parse_time(iso)
    except ValueError:
        return "?"
    d = int((now if now is not None else time.time()) - t)
    if d < 60:
        return "vừa xong"
    if d < 3600:
        return f"{d // 60} phút trước"
    if d < 86400:
        return f"{d // 3600} giờ trước"
    return f"{d // 86400} ngày trước"


def parse_time(iso: str) -> float:
    """ISO của chrono (có thể 9 chữ số lẻ giây — Python 3.10 không đọc được) → epoch."""
    s = iso.replace("Z", "+00:00")
    m = re.match(r"^(.*?\.\d{6})\d+(.*)$", s)
    if m:
        s = m.group(1) + m.group(2)
    return datetime.fromisoformat(s).timestamp()


def days_left(epoch: int | None, now: float | None = None) -> float | None:
    if not epoch:
        return None
    return (epoch - (now if now is not None else time.time())) / 86400


def page_count(total: int, per_page: int) -> int:
    return max(1, -(-total // per_page))


def title_of(it: dict) -> str:
    """Tên video; chưa lấy được tên (title vẫn là link) thì ghi rõ thay vì hiện link thô."""
    t = (it.get("title") or "").strip()
    if not t or t.startswith(("http://", "https://")):
        return "(đang lấy tên video…)"
    return t


def first_line(s: str | None, n: int = 160) -> str:
    return short((s or "").split("\n")[0], n)


def progress_line(it: dict) -> str:
    """Dòng tiến độ — chỉ hiện số liệu ĐANG có (không in thanh trống / '—')."""
    st = it.get("state")
    if st == "downloading":
        pct = it.get("percent")
        if pct is None:
            return "⬇️ Đang chuẩn bị tải…"
        parts = [f"{bar(pct)} {pct:.0f}%"]
        if it.get("speedBps"):
            parts.append(speed(it["speedBps"]))
        if it.get("etaSec"):
            parts.append(f"còn {eta(it['etaSec'])}")
        return " · ".join(parts)
    if st == "queued":
        err = it.get("error")
        return "⏳ " + (first_line(err, 140) if err else "Đang chờ tới lượt")
    if st == "failed":
        return "❌ " + first_line(it.get("error"), 160)
    if st == "skipped":
        return "⏭ " + (first_line(it.get("error"), 140) or "Bỏ qua — video đã tải trước đó")
    return STATE_ICON.get(st, "•") + " " + STATE_NAME.get(st, st or "")


def item_detail(it: dict) -> str:
    st = it.get("state")
    lines = [f"<b>{esc(title_of(it))}</b>",
             f"Trạng thái: {STATE_ICON.get(st, '')} <b>{STATE_NAME.get(st, st)}</b>"
             f" · mã <code>{esc(it.get('shortId'))}</code>"]
    if st in ("downloading", "queued"):
        lines.append(esc(progress_line(it)))
    if it.get("error") and st != "queued":
        lines.append("\n<i>" + esc(short(it["error"], 700)) + "</i>")
    if st == "completed" and it.get("outputPath"):
        lines.append(f"\n💾 Đã lưu trên máy tính:\n<code>{esc(it['outputPath'])}</code>")
    elif it.get("saveFolder"):
        lines.append(f"📁 Lưu vào: <code>{esc(it['saveFolder'])}</code>")
    lines.append(f"🔗 {esc(it.get('url'))}")
    return "\n".join(lines)


def tracked_text(items: list[dict], folder: str | None, audio: bool) -> str:
    """Tin 'đã thêm' tự cập nhật đến khi mọi mục xong/lỗi."""
    done = sum(1 for it in items if it.get("state") == "completed")
    fail = sum(1 for it in items if it.get("state") in ("failed", "cancelled"))
    left = len(items) - done - fail - sum(1 for it in items if it.get("state") == "skipped")
    kind = "âm thanh" if audio else "video"
    if left > 0:
        head = f"⬇️ <b>Đang tải {len(items)} {kind}</b> — xong {done}/{len(items)}"
    else:
        head = f"✅ <b>Xong</b> — tải được {done}/{len(items)} {kind}" + (f" · ❌ lỗi {fail}" if fail else "")
    lines = [head]
    for n, it in enumerate(items[:10], 1):
        lines.append(f"{n}. {esc(short(title_of(it), 60))}\n    {esc(progress_line(it))}")
    if len(items) > 10:
        lines.append(f"… và {len(items) - 10} mục nữa")
    if folder:
        lines.append(f"\n💾 Lưu trên máy tính: <code>{esc(folder)}</code>")
    if left == 0 and done:
        lines.append("Bấm <b>📤 Gửi về điện thoại</b> nếu muốn xem/lưu trên điện thoại (không bấm thì chỉ nằm trên máy tính).")
    return "\n".join(lines)


def watch_result(action: str, res: dict | None, err: BaseException | None) -> str:
    """Câu báo kết quả bấm 'Quét ngay' / 'Tải thêm 1 video'."""
    if err is not None:
        return f"❌ <b>Không làm được:</b> {esc(err)}"
    res = res or {}
    if "count" not in res:  # app bản cũ: chỉ biết đã gửi lệnh
        return "✅ Đã gửi lệnh cho app (bản app này chưa báo kết quả — cập nhật app để thấy kết quả)."
    n = int(res.get("count") or 0)
    ch = res.get("channel") or {}
    if n > 0:
        pick = f"\n{esc(ch['lastPick'])}" if action == "more" and ch.get("lastPick") else ""
        what = "video mới" if action == "check" else "video"
        return f"✅ <b>Đã thêm {n} {what}</b> vào hàng đợi tải.{pick}"
    if ch.get("lastError"):
        return "🔴 <b>Quét kênh bị lỗi</b> — xem lý do bên dưới."
    if action == "more":
        return ("ℹ️ <b>Kênh đã HẾT video chưa tải</b> — mọi video trên kênh đều đã làm/đã tải. "
                "Chờ kênh đăng video mới (app tự quét theo lịch).")
    return "ℹ️ <b>Không có video mới</b> kể từ lần quét trước."


# "paused" = chưa xong (người dùng tạm dừng) — đợt tải chưa được coi là hoàn tất.
ACTIVE_STATES = ("queued", "downloading", "paused")


def batch_text(items: list[dict], started: str, finished: bool) -> str:
    """1 tin cho CẢ ĐỢT tải (quét kênh / chạy nhóm) — tự cập nhật, không spam."""
    n = len(items)
    c = {k: sum(1 for it in items if it.get("state") == k)
         for k in ("completed", "downloading", "queued", "failed", "skipped", "cancelled", "paused")}
    done = c["completed"] + c["failed"] + c["skipped"] + c["cancelled"]
    pct = 100 * done / n if n else 100
    head = (f"✅ <b>Xong đợt tải</b> (bắt đầu {started})" if finished
            else f"📦 <b>Đợt tải</b> (bắt đầu {started}) — <b>{c['completed']}/{n}</b> xong")
    lines = [head, f"{bar(pct)} {pct:.0f}%"]
    stats = [f"✅ Xong {c['completed']}"]
    if c["downloading"]:
        stats.append(f"⬇️ Đang tải {c['downloading']}")
    if c["queued"]:
        stats.append(f"⏳ Chờ {c['queued']}")
    if c["paused"]:
        stats.append(f"⏸ Dừng {c['paused']}")
    if c["failed"]:
        stats.append(f"❌ Lỗi {c['failed']}")
    if c["skipped"]:
        stats.append(f"⏭ Bỏ qua {c['skipped']}")
    lines.append(" · ".join(stats))
    downloading = [it for it in items if it.get("state") == "downloading"]
    if downloading and not finished:
        lines.append("\n<b>Đang tải:</b>")
        for it in downloading[:6]:
            lines.append(f"• {esc(short(title_of(it), 42))}\n   {esc(progress_line(it))}")
        if len(downloading) > 6:
            lines.append(f"… và {len(downloading) - 6} video nữa")
    failed = [it for it in items if it.get("state") == "failed"]
    if failed:
        lines.append("\n<b>Lỗi:</b>")
        for it in failed[:5]:
            lines.append(f"• {esc(short(title_of(it), 40))} — <i>{esc(first_line(it.get('error'), 80))}</i>")
        if len(failed) > 5:
            lines.append(f"… và {len(failed) - 5} lỗi nữa")
    if finished and c["completed"]:
        lines.append("\n💾 Video đã lưu trên máy tính (thư mục từng kênh). "
                     "Muốn xem trên điện thoại: bấm <b>📤 Gửi video về điện thoại</b>.")
    elif not finished:
        lines.append(f"\n<i>Tự cập nhật lúc {time.strftime('%H:%M:%S')}</i>")
    return "\n".join(lines)


def _norm_dir(p) -> str:
    return str(p or "").replace("/", "\\").rstrip("\\").lower()


def batch_report(items: list[dict], channels: list[dict], started: str, t0: float | None,
                 now: float | None = None) -> str:
    """BÁO CÁO TỔNG khi đợt tải xong: tổng số + từng NHÓM (video xong/lỗi, kênh đã
    đủ hạn mức hôm nay). Video gán vào nhóm theo thư mục lưu = thư mục kênh."""
    now = now or time.time()
    by_dir = {_norm_dir(c.get("destDir")): c for c in channels if c.get("destDir")}
    c = {k: sum(1 for it in items if it.get("state") == k) for k in ("completed", "failed", "skipped", "cancelled")}
    mins = int((now - t0) // 60) if t0 else None
    dur = (f"{mins // 60} giờ {mins % 60} phút" if mins and mins >= 60 else f"{mins} phút") if mins is not None else ""
    lines = [f"🏁 <b>ĐÃ XONG ĐỢT TẢI</b> — {len(items)} video",
             f"⏱ {started} → {time.strftime('%H:%M', time.localtime(now))}" + (f" ({dur})" if dur else ""),
             f"✅ Xong <b>{c['completed']}</b>" + (f" · ❌ Lỗi <b>{c['failed']}</b>" if c["failed"] else "")
             + (f" · ⏭ Bỏ qua {c['skipped']}" if c["skipped"] else "")
             + (f" · 🚫 Huỷ {c['cancelled']}" if c["cancelled"] else "")]
    groups: dict[str, dict[str, int]] = {}
    for it in items:
        ch = by_dir.get(_norm_dir(it.get("saveFolder")))
        g = (ch.get("group") or "Chưa nhóm") if ch else "Tải lẻ (không thuộc kênh theo dõi)"
        d = groups.setdefault(g, {"completed": 0, "failed": 0, "skipped": 0, "cancelled": 0})
        if it.get("state") in d:
            d[it["state"]] += 1
    if groups:
        lines.append("\n<b>Theo nhóm:</b>")
        for g, d in sorted(groups.items(), key=lambda kv: -sum(kv[1].values())):
            line = f"📁 <b>{esc(g)}</b>: ✅ {d['completed']}"
            if d["failed"]:
                line += f" · ❌ {d['failed']}"
            if d["skipped"]:
                line += f" · ⏭ {d['skipped']}"
            on = [ch for ch in channels if (ch.get("group") or "Chưa nhóm") == g and ch.get("enabled")]
            if on:
                full = sum(1 for ch in on if today_count(ch) >= daily_limit(ch))
                line += f"\n    Hôm nay: <b>{full}/{len(on)}</b> kênh đã đủ video"
                if full < len(on):
                    line += f" · ⏳ {len(on) - full} kênh chưa đủ"
            lines.append(line)
    failed = [it for it in items if it.get("state") == "failed"]
    if failed:
        lines.append("\n<b>Lỗi:</b>")
        for it in failed[:5]:
            lines.append(f"• {esc(short(title_of(it), 40))} — <i>{esc(first_line(it.get('error'), 80))}</i>")
        if len(failed) > 5:
            lines.append(f"… và {len(failed) - 5} lỗi nữa")
    if c["completed"]:
        lines.append("\n💾 Video đã lưu trên máy tính (thư mục từng kênh).")
    return clip_message("\n".join(lines))


def clip_message(text: str, limit: int = 3900) -> str:
    """Telegram từ chối tin > 4096 ký tự (mất luôn tin) → cắt gọn theo dòng."""
    if len(text) <= limit:
        return text
    return text[:limit].rsplit("\n", 1)[0] + "\n…"


def today_count(c: dict, today: str | None = None) -> int:
    """Số video ĐÃ TẢI HÔM NAY. App giữ `dripCount` của NGÀY `dripDate` — khác
    hôm nay thì là số của ngày cũ (từng hiện '1/1' của ngày 27 làm tưởng đã tải)."""
    today = today or time.strftime("%Y-%m-%d")
    return int(c.get("dripCount") or 0) if c.get("dripDate") == today else 0


def daily_limit(c: dict) -> int:
    """Hạn mức/ngày như app thật dùng (watcher kẹp 1..3) — file cấu hình lạ
    (0, 10, chữ) không làm bot hiện số khác với app."""
    try:
        return max(1, min(3, int(c.get("dailyLimit") or 1)))
    except (TypeError, ValueError):
        return 1


def day_status(c: dict, today: str | None = None) -> str:
    """Chữ rõ nghĩa cho hạn mức ngày: 'đủ 1/1 hôm nay' / 'chưa tải 0/1'."""
    n, lim = today_count(c, today), daily_limit(c)
    if c.get("downloading"):
        return f"⬇️ đang tải {n}/{lim}"
    if n >= lim:
        return f"✅ đủ {n}/{lim} hôm nay"
    return f"chưa tải {n}/{lim}" if n == 0 else f"đã tải {n}/{lim}"


# Trạng thái kênh: khoá → (biểu tượng, nhãn ngắn). Mỗi lỗi một nhãn RIÊNG —
# không gộp mọi thứ thành "lỗi" (anh Hùng 31/10/2026: "báo cho chuẩn").
STATUS_META = {
    "full": ("✅", "đủ hôm nay"),
    "dl": ("⬇️", "đang tải"),
    "wait": ("⏳", "đã quét, chưa có video tải"),
    "notrun": ("💤", "chưa quét hôm nay"),
    "empty": ("📭", "hết video"),
    "dead": ("❌", "kênh không còn"),
    "blocked": ("🤖", "YouTube chặn tạm"),
    "cookie": ("🍪", "lỗi cookie"),
    "net": ("📡", "lỗi mạng"),
    "private": ("🔒", "riêng tư"),
    "err": ("🔴", "lỗi khác"),
    "off": ("⏸", "tắt theo dõi"),
}
STATUS_ADVICE = {
    "empty": "Kênh còn nhưng không còn video chưa làm/không có video ở mục theo dõi — mở kênh xem; "
             "chỉ còn Shorts thì đổi loại, trống thật thì tắt theo dõi.",
    "dead": "YouTube báo 404 (kênh bị xoá/khoá/đổi tên @) — mở link kiểm tra, mất thật thì tắt theo dõi.",
    "blocked": "YouTube chặn tạm thời, KHÔNG phải kênh hỏng — app tự quét lại lượt sau.",
    "cookie": "Vào 🍪 Cookie → 🧪 Kiểm tra; đăng nhập lại trên Firefox nếu đỏ.",
    "net": "Kiểm tra mạng/proxy trên máy tính.",
    "private": "Kênh để riêng tư/chỉ thành viên — không tải được nếu không có quyền.",
    "err": "Bấm tên kênh trong 📺 Kênh để xem dòng lỗi chi tiết.",
    "notrun": "Kênh chưa được quét hôm nay — bấm ▶ Quét nhóm (bot hoặc app), hoặc bật tự quét trong Cài đặt.",
}


def local_day(iso: str | None) -> str | None:
    """Ngày (giờ máy, YYYY-MM-DD) của mốc ISO UTC app trả về."""
    if not iso:
        return None
    try:
        return time.strftime("%Y-%m-%d", time.localtime(parse_time(iso)))
    except (ValueError, OverflowError, OSError):
        return None


def channel_error_kind(err: str | None) -> str | None:
    """Loại lỗi QUÉT KÊNH theo câu app báo (error.rs / channel_fetcher)."""
    if not err:
        return None
    e = err.lower()
    if err.startswith("❌ Kênh KHÔNG CÒN") or "http error 404" in e or "requested entity was not found" in e:
        return "dead"
    if err.startswith("📭") or "does not have a" in e or "không có video nào trên kênh" in e:
        return "empty"
    if err.startswith(("🤖", "⏳")) or "not a bot" in e or "http error 429" in e or "yêu cầu xác minh" in e:
        return "blocked"
    if err.startswith(("🍪", "🔞")):
        return "cookie"
    if err.startswith("📡") or "lỗi mạng" in e or "timed out" in e:
        return "net"
    if err.startswith("🔒"):
        return "private"
    return "err"


def channel_status(c: dict, today: str | None = None) -> tuple[str, str, str]:
    """(biểu tượng, nhãn có số, khoá) — 1 trạng thái ĐÚNG cho mỗi kênh."""
    if not c.get("enabled"):
        e, l = STATUS_META["off"]
        return e, l, "off"
    kind = channel_error_kind(c.get("lastError"))
    if kind:
        e, l = STATUS_META[kind]
        return e, l, kind
    n, lim = today_count(c, today), daily_limit(c)
    if c.get("downloading"):
        return "⬇️", f"đang tải {n}/{lim}", "dl"
    if n >= lim:
        return "✅", f"đủ {n}/{lim} hôm nay", "full"
    if c.get("sourceEmpty"):
        return "📭", f"hết video chưa làm {n}/{lim}", "empty"
    today = today or time.strftime("%Y-%m-%d")
    checked = local_day(c.get("lastChecked"))
    if checked != today:
        when = f" (lần cuối {checked[8:10]}/{checked[5:7]})" if checked else " (chưa quét lần nào)"
        return "💤", f"chưa quét hôm nay{when}", "notrun"
    return "⏳", f"đã quét, chưa có video tải {n}/{lim}", "wait"


def channel_button(c: dict) -> str:
    icon, label, _ = channel_status(c)
    name = short(c.get("targetName") or c.get("title") or c.get("url"), 24)
    return f"{icon} {name} · {label}"


def channel_detail(c: dict) -> str:
    name = esc(c.get("targetName") or c.get("title") or c.get("url"))
    lines = [
        f"<b>📺 {name}</b>" + (f" [{esc(c['group'])}]" if c.get("group") else ""),
        f"Theo dõi: {'🟢 đang bật' if c.get('enabled') else '⏸ đang tắt'}"
        f" · tự tải: {'có' if c.get('autoDownload') else 'không'}",
        "Tình trạng: {} {}".format(*channel_status(c)[:2])
        + (f"\n👉 <i>{STATUS_ADVICE[channel_status(c)[2]]}</i>" if channel_status(c)[2] in STATUS_ADVICE else ""),
        f"Hôm nay: {day_status(c)} (hạn mức {daily_limit(c)} video/ngày, sang ngày mới tự về 0)",
        f"Lần quét gần nhất: {ago(c.get('lastChecked'))}"
        + (f" · {c['lastNewCount']} video mới" if c.get("lastNewCount") else ""),
        f"Đã làm: {c.get('done', 0)} video"
        + (" · ⚠️ <b>kênh đã hết video chưa tải</b>" if c.get("sourceEmpty") else ""),
    ]
    if c.get("lastError"):
        lines.append("\n🔴 <b>Lỗi lần quét trước:</b>\n<i>" + esc(short(c["lastError"], 600)) + "</i>")
    lines.append(f"\n🔗 {esc(c.get('url'))}")
    return "\n".join(lines)


def item_line(it: dict, with_id: bool = True) -> str:
    icon = STATE_ICON.get(it.get("state"), "•")
    title = esc(short(title_of(it), 55))
    sid = f" <code>{esc(it.get('shortId'))}</code>" if with_id else ""
    if it.get("state") == "downloading":
        return f"{icon}{sid} {title}\n    {esc(progress_line(it))}"
    return f"{icon}{sid} {title}"


def status_text(st: dict) -> str:
    c = st.get("counts", {})
    s = st.get("settings", {})
    w = st.get("watch", {})
    lines = [
        f"<b>📊 BQHungDown {esc(st.get('version', ''))}</b>",
        f"⬇️ Đang tải: <b>{c.get('downloading', 0)}</b> · ⏳ Chờ: {c.get('queued', 0)}"
        f" · ⏸ Dừng: {c.get('paused', 0)}",
        f"✅ Xong: {c.get('completed', 0)} · ❌ Lỗi: <b>{c.get('failed', 0)}</b>"
        f" · ⏭ Bỏ qua: {c.get('skipped', 0)}",
        f"🚀 Tốc độ tổng: {speed(st.get('speedBps'))}",
        "",
        f"📺 Kênh theo dõi: {w.get('enabled', 0)}/{w.get('channels', 0)} bật"
        + (f" · ⚠️ {w.get('withError')} kênh lỗi" if w.get("withError") else ""),
        f"🤖 Tự quét: {'BẬT' if s.get('watchAutoEnabled') else 'TẮT'} (mỗi {s.get('watchIntervalMin')} phút)"
        f" · Luồng tải: {s.get('maxConcurrency')}",
    ]
    active = st.get("active") or []
    if active:
        lines += ["", "<b>Đang tải:</b>"] + [item_line(it) for it in active[:8]]
        if len(active) > 8:
            lines.append(f"… và {len(active) - 8} mục nữa")
    return "\n".join(lines)


def queue_text(data: dict, state: str, page: int, per_page: int) -> str:
    items = data.get("items", [])
    total = data.get("total", len(items))
    name = {"all": "Tất cả", "active": "Đang chạy"}.get(state, STATE_NAME.get(state, state))
    head = f"<b>📋 Hàng đợi — {esc(name)}</b> ({total} mục)"
    chunk = items[page * per_page:(page + 1) * per_page]
    if not chunk:
        return head + "\n\nKhông có mục nào."
    body = []
    for it in chunk:
        line = item_line(it)
        if it.get("state") == "failed" and it.get("error"):
            line += "\n    <i>" + esc(short(it["error"].split("\n")[0], 110)) + "</i>"
        body.append(line)
    pages = max(1, -(-min(total, len(items)) // per_page))
    return head + f" · trang {page + 1}/{pages}\n\n" + "\n".join(body)


def channel_line(c: dict) -> str:
    icon = "🟢" if c.get("enabled") else "⚪"
    if c.get("lastError"):
        icon = "🔴"
    name = esc(short(c.get("targetName") or c.get("title") or c.get("url"), 40))
    grp = f" [{esc(c['group'])}]" if c.get("group") else ""
    return (f"{icon} <b>{name}</b>{grp}\n    {esc(day_status(c))}"
            f" · quét {ago(c.get('lastChecked'))}")


def firefox_block(check, now_wall: float | None = None) -> list[str]:
    """Khối YouTube-từ-Firefox: trạng thái THẬT từ lần kiểm tra gần nhất."""
    lines = ["<b>▶️ YouTube</b> — 🦊 <b>TỰ ĐỘNG lấy cookie từ Firefox</b> mỗi lần tải"]
    if check is None:
        lines.append("⚪ Chưa kiểm tra — bấm <b>🧪 Kiểm tra ngay</b> (mất 10–30 giây).")
        return lines
    when = time.strftime("%H:%M %d/%m", time.localtime(check.at))
    if check.status == "ok":
        lines += ["🟢 <b>ĐANG HOẠT ĐỘNG</b> — Firefox đã đăng nhập, tải được cả video 18+.",
                  f"<i>Kiểm tra lúc {when} · bot tự kiểm tra mỗi 6 giờ, hỏng là báo ngay.</i>",
                  "✅ Anh <b>không cần làm gì</b>. Đừng đăng xuất YouTube trên Firefox là được."]
    else:
        lines += [f"🔴 <b>CHƯA DÙNG ĐƯỢC</b> — {esc(check.detail)}",
                  f"<i>Kiểm tra lúc {when}.</i>",
                  "👉 Bấm <b>🦊 Mở Firefox</b> → đăng nhập YouTube → bấm <b>🧪 Kiểm tra ngay</b>."]
    return lines


def cookies_text(data: dict, now: float | None = None, check=None) -> str:
    lines = ["<b>🍪 Cookie</b>", ""]
    sites = data.get("sites", [])
    yt = next((s for s in sites if s.get("site") == "youtube"), None)
    if yt and yt.get("browser") == "firefox":
        lines += firefox_block(check) + ["", "<b>Trang khác</b> (chỉ cần khi tải video phải đăng nhập):"]
    for s in sites:
        label = esc(s.get("label"))
        if s.get("browser"):
            if s is yt and s.get("browser") == "firefox":
                continue
            if s.get("browser") == "firefox":
                lines.append(f"🦊 {label}: tự lấy từ Firefox (đăng nhập {label} trên Firefox nếu cần)")
            else:
                lines.append(f"🌐 {label}: lấy từ trình duyệt {esc(s['browser'])}")
            continue
        if not s.get("file"):
            lines.append(f"▫️ {label}: chưa đặt (dùng ô chung)")
            continue
        if not s.get("exists"):
            lines.append(f"🔴 {label}: file cookie đã bị xoá khỏi máy — gửi file mới nếu cần")
            continue
        dl = days_left(s.get("earliestExpiry"), now)
        if s.get("cookies") == 0:
            state = "🔴 không có cookie đúng trang"
        elif dl is not None and dl < 0:
            state = "🔴 đã hết hạn"
        elif dl is not None and dl < 3:
            state = f"🟠 sắp hết hạn ({dl:.1f} ngày)"
        else:
            state = "🟢 ổn" + (f" (còn {dl:.0f} ngày)" if dl is not None else "")
        lines.append(f"{state} — {label} · {s.get('cookies')} cookie")
    lines += ["", "<i>Trang khác: gửi file <b>cookies.txt</b> vào đây (ghi tên trang ở chú thích,"
              " vd <code>douyin</code>, hoặc để bot tự nhận).</i>"]
    return "\n".join(lines)


SITE_LABELS = {"youtube": "YouTube", "tiktok": "TikTok", "douyin": "Douyin", "facebook": "Facebook",
               "instagram": "Instagram", "bilibili": "Bilibili", "twitter": "X (Twitter)"}


def site_label(site: str) -> str:
    return SITE_LABELS.get(site, site)


def guess_cookie_site(text: str, sites: dict[str, tuple[str, ...]]) -> str | None:
    """Trang chiếm đa số cookie trong file. Hoà/không có → None."""
    counts = {k: 0 for k in sites}
    for raw in text.splitlines():
        line = raw.strip()
        if line.startswith("#HttpOnly_"):
            line = line[len("#HttpOnly_"):]
        if not line or line.startswith("#"):
            continue
        host = line.split("\t", 1)[0].lstrip(".").lower()
        for key, domains in sites.items():
            if any(host == d or host.endswith("." + d) for d in domains):
                counts[key] += 1
    best = sorted(counts.items(), key=lambda kv: kv[1], reverse=True)
    if not best or best[0][1] == 0 or (len(best) > 1 and best[0][1] == best[1][1]):
        return None
    return best[0][0]


# ───────────────────────── Lịch tải (dây chuyền, Đợt 1b) ─────────────────────────

def nhom_ten(g: str | None) -> str:
    return g or "Chưa phân nhóm"


def gio_hien(iso: str | None, now: float | None = None) -> str:
    """"06:00 hôm nay" / "06:00 ngày mai" / "06:00 ngày 07/11" (giờ của máy chạy app)."""
    if not iso:
        return "—"
    try:
        t = datetime.fromisoformat(iso.replace("Z", "+00:00"))
    except ValueError:
        return iso
    hom_nay = datetime.fromtimestamp(now if now is not None else time.time(), t.tzinfo).date()
    d = (t.date() - hom_nay).days
    ngay = "hôm nay" if d == 0 else "ngày mai" if d == 1 else f"ngày {t:%d/%m}"
    return f"{t:%H:%M} {ngay}"


def lich_text(d: dict, now: float | None = None) -> str:
    """Tin trạng thái Lịch tải (bot TỰ SỬA tin này mỗi phút khi có thay đổi)."""
    bat = bool(d.get("lichTaiEnabled"))
    lines = [f"<b>🗓 Lịch tải: {'BẬT' if bat else 'TẮT'}</b> · bắt đầu <b>{esc(d.get('gio'))}</b>"
             f" · hạn chót mỗi đợt <b>{esc(d.get('hanChotGio'))} giờ</b>"]
    nhom = d.get("nhom") or []
    da = set(d.get("nhomDaQuet") or [])
    if nhom:
        lines.append("📋 Thứ tự nhóm: " + " → ".join(("✅" if g in da else "") + esc(nhom_ten(g)) for g in nhom))
    else:
        lines.append("📋 Chưa chọn nhóm nào — chưa bật được (thêm nhóm: <code>/lich nhom A, B</code>).")
    if d.get("dangChay"):
        dung = " — <b>đang dừng</b> (chờ nhóm đang quét xong)" if d.get("dangDung") else ""
        lines.append(f"⚙️ Đang làm: <b>{esc(d['dangChay'])}</b>{dung}")
    else:
        lines.append("⚙️ Đang làm: không")
    lines.append("⏰ Lần chạy tới: " + (esc(gio_hien(d.get("lanToi"), now)) if d.get("lanToi") else "— (Lịch tắt)"))
    if d.get("boQuaHomNay"):
        lines.append("⏭ Hôm nay: đã bỏ qua")
    if d.get("lyDoHoan"):
        lines.append("ℹ️ Lý do hoãn gần nhất: " + esc(d["lyDoHoan"]))
    mo = [x for x in d.get("dots") or [] if x.get("trangThai") == "dang_tai"]
    if mo:
        lines.append("📦 Đợt đang mở: " + "; ".join(
            f"{esc(x.get('dotId'))} ({esc(x.get('nhom'))}: {x.get('xong', 0)}/{x.get('video', 0)} xong)" for x in mo))
    else:
        lines.append("📦 Đợt đang mở: không")
    lines.append(f"🐢 Video trễ hôm nay: {d.get('tre', 0)}")
    for v in d.get("vanDe") or []:
        lines.append("⚠️ " + esc(v.get("text")))
    return "\n".join(lines)


def lich_markup(d: dict, tok: str) -> dict:
    """Nút của bảng Lịch tải. `tok` = mã bảng: bấm nút trên bảng CŨ → báo hết hạn."""
    p = f"lt:{tok}:"
    bat = bool(d.get("lichTaiEnabled"))
    rows = [
        [("⏸ Tắt Lịch" if bat else "▶ Bật Lịch", p + ("off" if bat else "on")), ("▶ Chạy ngay", p + "chay")],
        [("⏹ Dừng lượt…", p + "dung?"), ("⏭ Bỏ qua hôm nay", p + "boqua")],
        [("🕐 −30 phút", p + "gio:-30"), ("🕐 +30 phút", p + "gio:30")],
        [("⏳ Hạn −1 giờ", p + "han:-1"), ("⏳ Hạn +1 giờ", p + "han:1")],
    ]
    # Nút ↑ mang MÃ TÊN nhóm (không phải chỉ số): bảng cũ / bấm lặp vẫn áp đúng
    # nhóm đó, bấm 2 lần = lên 2 bậc chứ không đổi ngược lại.
    up = [(f"↑ {short(nhom_ten(g), 18)}", p + f"up:{ma_nhom(g)}") for i, g in enumerate(d.get("nhom") or []) if i > 0]
    rows += [up[i:i + 2] for i in range(0, len(up), 2)]
    rows.append([("🔄 Làm mới", p + "r"), ("🏠 Menu", "m")])
    return {"inline_keyboard": [[{"text": t, "callback_data": c} for t, c in r] for r in rows if r]}


def ma_nhom(g: str) -> str:
    """Mã ngắn của tên nhóm cho callback_data (giới hạn 64 byte của Telegram)."""
    import hashlib
    return hashlib.sha1((g or "").encode("utf-8")).hexdigest()[:8]


def doi_gio(gio: str, phut: int) -> str:
    """"06:00" + 30 phút → "06:30" (vòng qua nửa đêm)."""
    h, m = (int(x) for x in gio.split(":"))
    t = (h * 60 + m + phut) % 1440
    return f"{t // 60:02d}:{t % 60:02d}"
