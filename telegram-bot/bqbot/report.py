"""BÁO CÁO NGÀY — tổng hợp chi tiết, đúng nghĩa từng phần:
video đã tải / lỗi / bỏ qua HÔM NAY, từng NHÓM kênh (đủ hạn mức, chờ lượt, hết
video, kênh mất, YouTube chặn tạm…), danh sách kênh cần xem và tình trạng cookie.

Dùng chung cho: tin tự gửi CUỐI NGÀY (monitor) và nút 📋 Báo cáo ngày (xem bất
cứ lúc nào). Hàm `daily_report` thuần dữ liệu → test được không cần app.
"""

from __future__ import annotations

import time
from datetime import date, datetime

from . import fmt
from .app_client import AppClient, AppError, AppOffline

# Thứ tự + nhãn các loại trạng thái kênh trong báo cáo nhóm.
STATUS_ORDER = ("full", "dl", "wait", "notrun", "empty", "dead", "blocked", "cookie", "net", "private", "err", "off")
TELEGRAM_LIMIT = 3900


def _local_date(iso: str | None) -> date | None:
    if not iso:
        return None
    try:
        return datetime.fromisoformat(iso.replace("Z", "+00:00")).astimezone().date()
    except ValueError:
        return None


def _name(c: dict) -> str:
    return c.get("targetName") or c.get("title") or c.get("url") or "?"


def daily_report(channels: list[dict], history: list[dict], queue: list[dict],
                 cookie=None, today: date | None = None, now: float | None = None,
                 auto_scan: bool | None = None) -> str:
    today = today or date.today()
    today_s = today.strftime("%Y-%m-%d")
    done = [h for h in history if _local_date(h.get("finishedAt")) == today]
    q_today = [it for it in queue if _local_date(it.get("finishedAt") or it.get("createdAt")) == today]
    failed = [it for it in q_today if it.get("state") == "failed"]
    skipped = [it for it in q_today if it.get("state") == "skipped"]
    active = [it for it in queue if it.get("state") in fmt.ACTIVE_STATES]

    by_dir = {fmt._norm_dir(c.get("destDir")): c for c in channels if c.get("destDir")}
    per_chan: dict[str, int] = {}
    for h in done:
        c = by_dir.get(fmt._norm_dir(h.get("saveFolder")))
        if c:
            per_chan[c["id"]] = per_chan.get(c["id"], 0) + 1

    stamp = time.strftime("%H:%M", time.localtime(now or time.time()))
    lines = [f"📋 <b>BÁO CÁO NGÀY {today.strftime('%d/%m')}</b> <i>(lúc {stamp})</i>",
             f"✅ Đã tải hôm nay: <b>{len(done)}</b> video"
             + (f" · ❌ Lỗi <b>{len(failed)}</b>" if failed else "")
             + (f" · ⏭ Bỏ qua {len(skipped)}" if skipped else "")
             + (f" · ⬇️ Đang chạy {len(active)}" if active else "")]
    on = [c for c in channels if c.get("enabled")]
    lines.append(f"📺 Kênh theo dõi: {len(channels)} (đang bật {len(on)})")
    if auto_scan is False:
        lines.append("⚙️ Tự quét kênh đang <b>TẮT</b> — kênh chỉ quét khi anh bấm chạy nhóm.")

    # ── từng nhóm ──
    groups: dict[str, list[dict]] = {}
    for c in channels:
        groups.setdefault(c.get("group") or "Chưa nhóm", []).append(c)
    issues: dict[str, list[dict]] = {}
    for g in sorted(groups, key=lambda x: (x == "Chưa nhóm", x)):
        chans = groups[g]
        counts: dict[str, int] = {}
        for c in chans:
            emoji, label, key = fmt.channel_status(c, today_s)
            counts[key] = counts.get(key, 0) + 1
            if key in ("dead", "empty", "blocked", "cookie", "net", "private", "err"):
                issues.setdefault(key, []).append(c)
        vids = sum(per_chan.get(c["id"], 0) for c in chans)
        n_on = sum(1 for c in chans if c.get("enabled"))
        lines.append(f"\n📁 <b>{fmt.esc(g)}</b> — {n_on} kênh bật · tải hôm nay <b>{vids}</b> video")
        parts = [f"{fmt.STATUS_META[k][0]} {counts[k]} {fmt.STATUS_META[k][1]}" for k in STATUS_ORDER if counts.get(k)]
        lines.append("    " + " · ".join(parts) if parts else "    (không có kênh)")
        if counts.get("notrun"):
            days = [fmt.local_day(c.get("lastChecked")) for c in chans if c.get("enabled")]
            last = max((d for d in days if d), default=None)
            lines.append("    <i>💤 Nhóm chưa chạy hôm nay"
                         + (f" — lần quét gần nhất {last[8:10]}/{last[5:7]}" if last else "") + "</i>")

    # ── cần anh xem ──
    if issues:
        lines.append("\n⚠️ <b>Kênh cần xem</b> (lý do đúng từng kênh):")
        for key in STATUS_ORDER:
            cs = issues.get(key)
            if not cs:
                continue
            emoji, label = fmt.STATUS_META[key]
            names = ", ".join(fmt.esc(fmt.short(_name(c), 28)) for c in cs[:8])
            more = f" … +{len(cs) - 8}" if len(cs) > 8 else ""
            lines.append(f"{emoji} <b>{label}</b> ({len(cs)}): {names}{more}")
            lines.append(f"    <i>👉 {fmt.STATUS_ADVICE[key]}</i>")
    else:
        lines.append("\n✅ Không có kênh nào lỗi.")

    if failed:
        lines.append(f"\n❌ <b>Video tải lỗi hôm nay</b> ({len(failed)}):")
        for it in failed[:6]:
            lines.append(f"• {fmt.esc(fmt.short(fmt.title_of(it), 36))} — "
                         f"<i>{fmt.esc(fmt.first_line(it.get('error'), 70))}</i>")
        if len(failed) > 6:
            lines.append(f"… và {len(failed) - 6} video nữa")

    if cookie is not None:
        lines.append("\n🍪 Cookie YouTube (Firefox): " + ("🟢 hoạt động" if cookie.ok else f"🔴 {fmt.esc(cookie.detail)}"))

    text = "\n".join(lines)
    if len(text) > TELEGRAM_LIMIT:
        text = text[:TELEGRAM_LIMIT].rsplit("\n", 1)[0] + "\n… (xem tiếp ở 📺 Kênh)"
    return text


def report_markup(groups: list[str]) -> dict:
    from .handlers import kb  # tránh vòng import lúc nạp module
    rows = [[("🔄 Làm mới báo cáo", "rp")]]
    grow = [(fmt.short(g or "Chưa nhóm", 14), f"w:g{i}:0") for i, g in enumerate(groups[:9])]
    rows += [grow[i:i + 3] for i in range(0, len(grow), 3)]
    rows.append([("🔴 Kênh cần xem", "w:e:0"), ("❌ Video lỗi", "q:failed:0")])
    rows.append([("🏠 Menu", "m")])
    return kb(rows)


def build(app: AppClient, cookie=None) -> tuple[str, dict]:
    """Lấy dữ liệu MỚI từ app rồi dựng báo cáo. AppOffline/AppError để người gọi xử."""
    app.cache.clear("watch")
    channels = app.watch().get("items", [])
    history = app.get("/api/history?limit=3000").get("items", [])
    queue = app.queue("all", limit=2000).get("items", [])
    try:
        st = app.status()
    except (AppOffline, AppError):
        st = {}
    groups = list(st.get("watch", {}).get("groups") or [])
    auto = st.get("settings", {}).get("watchAutoEnabled")
    return daily_report(channels, history, queue, cookie, auto_scan=auto), report_markup(groups)
