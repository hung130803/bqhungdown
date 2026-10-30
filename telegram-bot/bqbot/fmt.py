"""Định dạng tin nhắn — TOÀN hàm thuần (không gọi mạng) để test dễ."""

from __future__ import annotations

import html
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
        t = datetime.fromisoformat(iso.replace("Z", "+00:00")).timestamp()
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


def days_left(epoch: int | None, now: float | None = None) -> float | None:
    if not epoch:
        return None
    return (epoch - (now if now is not None else time.time())) / 86400


def item_line(it: dict, with_id: bool = True) -> str:
    icon = STATE_ICON.get(it.get("state"), "•")
    title = esc(short(it.get("title") or it.get("url"), 55))
    sid = f" <code>{esc(it.get('shortId'))}</code>" if with_id else ""
    if it.get("state") == "downloading":
        pct = it.get("percent")
        p = f"{pct:.0f}%" if pct is not None else "…"
        return (f"{icon}{sid} {title}\n    {bar(pct)} {p} · {speed(it.get('speedBps'))}"
                f" · còn {eta(it.get('etaSec'))}")
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
