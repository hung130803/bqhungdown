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


def bar(pct: float | None, width: int = 12) -> str:
    if pct is None:
        return "░" * width
    full = max(0, min(width, round(pct / 100 * width)))
    return "█" * full + "░" * (width - full)


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


def channel_button(c: dict) -> str:
    icon = "🔴" if c.get("lastError") else ("🟢" if c.get("enabled") else "⏸")
    name = short(c.get("targetName") or c.get("title") or c.get("url"), 26)
    return f"{icon} {name} · {c.get('dripCount', 0)}/{c.get('dailyLimit', 1)}"


def channel_detail(c: dict) -> str:
    name = esc(c.get("targetName") or c.get("title") or c.get("url"))
    lines = [
        f"<b>📺 {name}</b>" + (f" [{esc(c['group'])}]" if c.get("group") else ""),
        f"Theo dõi: {'🟢 đang bật' if c.get('enabled') else '⏸ đang tắt'}"
        f" · tự tải: {'có' if c.get('autoDownload') else 'không'}",
        f"Hôm nay: {c.get('dripCount', 0)}/{c.get('dailyLimit', 1)} video"
        + (f" · đang tải {c['downloading']}" if c.get("downloading") else ""),
        f"Lần quét gần nhất: {ago(c.get('lastChecked'))}"
        + (f" · {c['lastNewCount']} video mới" if c.get("lastNewCount") else ""),
        f"Đã làm: {c.get('done', 0)} video",
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
    extra = f" · ⬇️{c['downloading']}" if c.get("downloading") else ""
    return (f"{icon} <b>{name}</b>{grp}\n    {c.get('dripCount', 0)}/{c.get('dailyLimit', 1)} hôm nay"
            f"{extra} · quét {ago(c.get('lastChecked'))}")


def cookies_text(data: dict, now: float | None = None) -> str:
    lines = ["<b>🍪 Cookie theo trang</b>"]
    for s in data.get("sites", []):
        label = esc(s.get("label"))
        if s.get("browser"):
            lines.append(f"🌐 {label}: lấy từ trình duyệt {esc(s['browser'])}")
            continue
        if not s.get("file"):
            lines.append(f"▫️ {label}: chưa đặt (dùng ô chung)")
            continue
        if not s.get("exists"):
            lines.append(f"🔴 {label}: file không còn trên máy")
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
    lines += ["", "Đổi cookie: gửi file <b>cookies.txt</b> vào đây (ghi tên trang ở chú thích,"
              " vd <code>youtube</code>, hoặc để bot tự nhận)."]
    return "\n".join(lines)


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
