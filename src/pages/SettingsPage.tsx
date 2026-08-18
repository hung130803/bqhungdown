import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSettingsStore } from "@/stores/useSettingsStore";
import * as cmd from "@/ipc/commands";
import type { Settings, Theme, Language, SiteCookie } from "@/types/models";
import { setLanguage } from "@/i18n";
import { UpdateSection } from "@/components/UpdateSection";

export function SettingsPage() {
  const { t } = useTranslation();
  const settings = useSettingsStore(s => s.settings);
  const update = useSettingsStore(s => s.update);

  // Deno (JS runtime) setup status — polled until ready so the user can see
  // when the video-link decoder is downloaded and active.
  const [deno, setDeno] = useState<string>("unknown");
  const [cleaning, setCleaning] = useState(false);
  const [cleanMsg, setCleanMsg] = useState<string | null>(null);

  // Nút "Sửa lỗi tải ngay" — chạy quy trình tự phục hồi (update yt-dlp nightly
  // + Deno + PO token) và hiện kết quả ngay dưới nút.
  const [fixing, setFixing] = useState(false);
  const [fixMsg, setFixMsg] = useState<string | null>(null);

  // Nút "Kiểm tra proxy" — test từng proxy đã nhập, hiện kết quả.
  const [testingProxy, setTestingProxy] = useState(false);
  const [proxyTestMsg, setProxyTestMsg] = useState<string | null>(null);
  const testProxyNow = async () => {
    if (testingProxy) return;
    const list = (settings?.proxies ?? []).map(p => p.trim()).filter(Boolean);
    if (list.length === 0) return;
    setTestingProxy(true);
    setProxyTestMsg("⏳ Đang kiểm tra…");
    const lines: string[] = [];
    for (let i = 0; i < list.length; i++) {
      try {
        const r = await cmd.testProxy(list[i]);
        lines.push(`Proxy ${i + 1}: ${r}`);
      } catch (e) {
        lines.push(`Proxy ${i + 1}: ${String(e)}`);
      }
      setProxyTestMsg(lines.join("\n"));
    }
    setTestingProxy(false);
  };
  const fixNow = async () => {
    if (fixing) return;
    setFixing(true);
    setFixMsg(null);
    try {
      const msg = await cmd.fixDownloadEngine();
      setFixMsg(`✓ Xong!\n${msg}`);
    } catch (e) {
      setFixMsg(`Lỗi khi sửa: ${String(e)}`);
    } finally {
      setFixing(false);
    }
  };

  // YouTube Data API — nhiều key, mỗi key có đèn xanh/đỏ. Key hết quota khi
  // tải sẽ tự nhảy sang key kế; ở đây để người dùng thấy key nào còn/hết.
  type KeyState = "idle" | "checking" | "ok" | "bad";
  const ytKeys = settings?.youtubeApiKeys ?? [];
  const [ytStatuses, setYtStatuses] = useState<Record<number, KeyState>>({});
  const [ytErrors, setYtErrors] = useState<Record<number, string>>({});
  const [newKey, setNewKey] = useState<string>("");
  const ytKeysJoined = ytKeys.join("|");

  const checkKeyAt = async (index: number, key: string) => {
    setYtStatuses(s => ({ ...s, [index]: "checking" }));
    const r = await cmd.validateYoutubeApiKey(key);
    setYtStatuses(s => ({ ...s, [index]: r.ok ? "ok" : "bad" }));
    setYtErrors(e => ({ ...e, [index]: r.ok ? "" : (r.error ?? "Key không hoạt động") }));
  };

  // Khi danh sách key đổi (thêm/xoá/load) → tự kiểm tra lại tất cả 1 lần.
  useEffect(() => {
    if (ytKeys.length === 0) return;
    let alive = true;
    (async () => {
      for (let i = 0; i < ytKeys.length; i++) {
        if (!alive) return;
        await checkKeyAt(i, ytKeys[i]);
      }
    })();
    return () => { alive = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ytKeysJoined]);

  const addKey = async () => {
    const k = newKey.trim();
    if (!k || ytKeys.includes(k)) { setNewKey(""); return; }
    await update({ youtubeApiKeys: [...ytKeys, k] });
    setNewKey("");
  };
  const removeKeyAt = async (index: number) => {
    await update({ youtubeApiKeys: ytKeys.filter((_, i) => i !== index) });
  };
  const checkAllKeys = async () => {
    for (let i = 0; i < ytKeys.length; i++) await checkKeyAt(i, ytKeys[i]);
  };
  const maskKey = (k: string) =>
    k.length <= 12 ? k : `${k.slice(0, 8)}…${k.slice(-4)}`;
  // Quota ƯỚC TÍNH đã tiêu hôm nay (theo app) cho từng key — tự làm mới 15s.
  const [ytUsage, setYtUsage] = useState<import("../ipc/commands").ApiUsageReport | null>(null);
  useEffect(() => {
    if (ytKeys.length === 0) { setYtUsage(null); return; }
    let alive = true;
    const load = () => cmd.youtubeApiUsage().then(u => { if (alive) setYtUsage(u); }).catch(() => {});
    void load();
    const id = setInterval(load, 15000);
    return () => { alive = false; clearInterval(id); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ytKeysJoined]);
  useEffect(() => {
    let alive = true;
    const tick = async () => {
      try {
        const s = await cmd.denoStatus();
        if (alive) setDeno(s);
        return s;
      } catch {
        return "unknown";
      }
    };
    void tick();
    const id = setInterval(async () => {
      const s = await tick();
      if (s === "ready") clearInterval(id);
    }, 3000);
    return () => { alive = false; clearInterval(id); };
  }, []);

  if (!settings) return null;

  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => {
    void update({ [k]: v } as Partial<Settings>);
    if (k === "language") setLanguage(v as Language);
  };

  const chooseFolder = async () => {
    const f = await cmd.pickFolder();
    if (f) await update({ defaultFolder: f });
  };

  const cleanJunk = async () => {
    const folder = settings?.defaultFolder;
    if (cleaning || !folder) return;
    setCleaning(true);
    setCleanMsg(null);
    try {
      const n = await cmd.cleanJunkFiles(folder);
      setCleanMsg(`✓ Đã xóa ${n} file rác.`);
    } catch {
      setCleanMsg("Lỗi khi dọn.");
    } finally {
      setCleaning(false);
    }
  };

  return (
    <div className="max-w-2xl mx-auto space-y-5">
      <h2 className="text-xl font-medium text-fg">{t("settings.title")}</h2>

      <UpdateSection />

      <Field label={t("settings.maxConcurrency")}>
        <div className="space-y-1">
          <input
            type="number"
            min={1}
            max={32}
            value={settings.maxConcurrency}
            onChange={e => {
              // Cho phép xoá rỗng trong khi đang gõ. Chỉ commit khi giá trị
              // thực sự là số ≥ 1; rỗng → giữ nguyên giá trị cũ tới khi user
              // gõ tiếp.
              const raw = e.target.value;
              if (raw === "") return;
              const n = parseInt(raw, 10);
              if (Number.isFinite(n) && n >= 1) set("maxConcurrency", n);
            }}
            className="w-24 px-3 py-2 rounded-md bg-surface border border-border text-fg"
          />
          {/* NÓI THẬT thay vì cấm: trần nới lên 32 vì bão-kết-nối đã được
              ngân sách conns_per_item chặn. Khuyên 3-6 nhưng để user tự quyết,
              kèm dấu hiệu để họ tự biết khi nào là quá nhiều. */}
          <p className="text-xs text-muted">
            Khuyên dùng <b>3–6</b> (tối đa 32) — anh cứ tự đặt, app không hạ
            xuống nữa. Video vượt số này <b>xếp hàng chờ</b>, kể cả video bấm
            Thử lại.
          </p>
          <p className="text-xs text-muted">
            Đặt cao <b>không chắc</b> nhanh hơn: YouTube bóp tốc độ từng kết nối
            và bóp theo IP khi thấy tải dồn dập. Dấu hiệu <b>quá nhiều</b>: mỗi
            video chỉ còn ~1 MB/s, hay gặp lỗi <i>“Tải bị treo”</i> / <i>“429”</i>.
            Gặp vậy thì hạ về 4–6.
          </p>
        </div>
      </Field>

      <Field label={t("settings.maxHeight")}>
        <div className="space-y-1">
          <select
            value={settings.maxHeight ?? 1080}
            onChange={e => set("maxHeight", parseInt(e.target.value, 10))}
            className="px-3 py-2 rounded-md bg-surface border border-border text-fg"
          >
            <option value={720}>720p</option>
            <option value={1080}>1080p (Full HD) — {t("settings.maxHeightRecommended")}</option>
            <option value={1440}>1440p (2K)</option>
            <option value={2160}>2160p (4K)</option>
            <option value={0}>{t("settings.maxHeightUnlimited")}</option>
          </select>
          <p className="text-xs text-muted">{t("settings.maxHeightHint")}</p>
        </div>
      </Field>

      <Field label={t("settings.defaultFolder")}>
        <div className="flex gap-2">
          <input readOnly value={settings.defaultFolder} className="flex-1 px-3 py-2 rounded-md bg-surface border border-border text-fg" />
          <button onClick={chooseFolder} className="px-3 py-2 rounded-md bg-surface-2 border border-border text-fg">{t("home.chooseFolder")}</button>
        </div>
      </Field>

      <div className="space-y-1 -mt-2">
        <div className="flex items-center gap-2">
          <button
            onClick={() => void cleanJunk()}
            disabled={cleaning || !settings.defaultFolder}
            className="px-3 py-2 rounded-md bg-surface-2 border border-border text-fg text-sm disabled:opacity-50"
          >
            {cleaning ? "Đang dọn…" : "🧹 Dọn file rác trong thư mục tải"}
          </button>
          {cleanMsg && <span className="text-xs text-success">{cleanMsg}</span>}
        </div>
        <p className="text-xs text-muted">
          App <b>tự động dọn 2 phút/lần</b> các file dở dang / 0 byte (icon trắng, không xài được) — nút này để dọn ngay.
          <b> Tuyệt đối không xóa video tải xong</b> (còn dùng được) và <b>không đụng video đang tải</b>.
        </p>
      </div>

      <Field label={t("settings.theme")}>
        <select value={settings.theme} onChange={e => set("theme", e.target.value as Theme)} className="px-3 py-2 rounded-md bg-surface border border-border text-fg">
          <option value="system">{t("settings.themeOptions.system")}</option>
          <option value="light">{t("settings.themeOptions.light")}</option>
          <option value="dark">{t("settings.themeOptions.dark")}</option>
        </select>
      </Field>

      <Field label={t("settings.language")}>
        <select value={settings.language} onChange={e => set("language", e.target.value as Language)} className="px-3 py-2 rounded-md bg-surface border border-border text-fg">
          <option value="vi">{t("settings.languageOptions.vi")}</option>
          <option value="en">{t("settings.languageOptions.en")}</option>
        </select>
      </Field>

      <Toggle
        label="Tạo thư mục riêng cho mỗi kênh khi tải"
        checked={settings.channelSubfolder ?? true}
        onChange={v => set("channelSubfolder", v)}
      />

      <Toggle
        label="Chạy ngầm khi đóng (bấm X xuống khay, vẫn tải tiếp)"
        checked={settings.minimizeToTray ?? true}
        onChange={v => set("minimizeToTray", v)}
      />

      <Field label="Bị giới hạn tốc độ → tự tải lại sau (phút)">
        <input
          type="number"
          min={1}
          max={120}
          value={settings.rateLimitCooldownMin ?? 10}
          onChange={e => {
            const n = parseInt(e.target.value, 10);
            if (Number.isFinite(n) && n >= 1) set("rateLimitCooldownMin", n);
          }}
          className="w-24 px-3 py-2 rounded-md bg-surface border border-border text-fg"
        />
      </Field>

      <Toggle label={t("settings.clipboardWatcher")} checked={settings.clipboardWatcher} onChange={v => set("clipboardWatcher", v)} />
      <Toggle label={t("settings.notifications")} checked={settings.notifications} onChange={v => set("notifications", v)} />
      <Toggle label={t("settings.aria2cEnabled")} checked={settings.aria2cEnabled} onChange={v => set("aria2cEnabled", v)} />
      <Toggle label={t("settings.skipDownloaded")} checked={settings.skipDownloaded} onChange={v => set("skipDownloaded", v)} />
      <p className="text-xs text-muted -mt-1">{t("settings.skipDownloadedHint")}</p>

      <Field label={t("settings.cookiesBrowser")}>
        <select
          value={settings.cookiesBrowser ?? ""}
          disabled={!!settings.cookiesFile}
          onChange={e => {
            const v = e.target.value;
            // Empty = clear; backend xử lý null = no cookies
            void update({ cookiesBrowser: v === "" ? null : v });
          }}
          className="px-3 py-2 rounded-md bg-surface border border-border text-fg disabled:opacity-50 disabled:cursor-not-allowed"
        >
          <option value="">{t("settings.cookiesNone")}</option>
          <option value="edge">Edge</option>
          <option value="chrome">Chrome</option>
          <option value="firefox">Firefox</option>
          <option value="brave">Brave</option>
          <option value="chromium">Chromium</option>
          <option value="vivaldi">Vivaldi</option>
          <option value="opera">Opera</option>
        </select>
      </Field>
      <p className="text-xs text-muted -mt-3">
        {settings.cookiesFile
          ? "Đang dùng file cookies.txt — option này bị tắt. Bỏ chọn file để dùng lại trình duyệt."
          : t("settings.cookiesHint")}
      </p>

      <Field label={t("settings.cookiesFile")}>
        <div className="flex gap-2">
          <input
            readOnly
            value={settings.cookiesFile ?? ""}
            placeholder={t("settings.cookiesFilePlaceholder")}
            className="flex-1 px-3 py-2 rounded-md bg-surface border border-border text-fg placeholder:text-muted"
          />
          <button
            onClick={async () => {
              const f = await cmd.pickFile();
              if (f) await update({ cookiesFile: f });
            }}
            className="px-3 py-2 rounded-md bg-surface-2 border border-border text-fg shrink-0"
          >
            {t("settings.chooseFile")}
          </button>
          {settings.cookiesFile && (
            <button
              onClick={() => void update({ cookiesFile: null })}
              className="px-3 py-2 rounded-md border border-border text-fg shrink-0"
              title={t("settings.cookiesFileClear")}
            >
              ✕
            </button>
          )}
        </div>
      </Field>
      <p className="text-xs text-muted -mt-3">{t("settings.cookiesFileHint")}</p>

      {/* ── Mỗi trang một ô cookie ────────────────────────────────────────
          Trước đây cả app dùng chung 1 file, muốn tải nhiều nền tảng thì phải
          tự gộp tay nhiều tên miền vào một file — trang nào hết hạn là phải
          gộp lại từ đầu. Giờ nạp riêng từng trang; trang nào để trống thì tự
          dùng ô chung ở trên. */}
      <div className="mt-4 rounded-md border border-border p-3">
        <div className="font-medium text-fg">Cookie riêng từng trang</div>
        <p className="text-xs text-muted mt-1 mb-3">
          Nạp cookie riêng cho từng nền tảng — app tự chọn đúng ô theo link, và khi
          cookie hỏng thì báo đúng tên trang. Trang nào để trống sẽ dùng ô chung ở trên.
        </p>
        <div className="flex flex-col gap-2">
          {SITE_COOKIE_SLOTS.map(s => (
            <SiteCookieRow
              key={s.key}
              siteKey={s.key}
              label={s.label}
              slot={settings.siteCookies?.[s.key]}
              onChange={next => {
                const map = { ...(settings.siteCookies ?? {}) };
                if (next) map[s.key] = next;
                else delete map[s.key];
                void update({ siteCookies: map });
              }}
            />
          ))}
        </div>
      </div>

      <Field label="Proxy (chống chặn khi tải số lượng lớn)">
        <textarea
          rows={4}
          value={(settings.proxies ?? []).join("\n")}
          onChange={e => set("proxies", e.target.value.split("\n") as unknown as Settings["proxies"])}
          placeholder={"Mỗi dòng 1 proxy. Dán dạng nào cũng được:\nip:port:user:pass\nhttp://user:pass@host:port\nsocks5://host:port"}
          className="w-full px-3 py-2 rounded-md bg-surface border border-border text-fg placeholder:text-muted font-mono text-xs"
          spellCheck={false}
        />
      </Field>
      <div className="flex items-center gap-2 -mt-2">
        <button
          onClick={() => void testProxyNow()}
          disabled={testingProxy || (settings.proxies ?? []).filter(p => p.trim()).length === 0}
          className="px-3 py-1.5 text-xs rounded-md bg-surface-2 border border-border text-fg disabled:opacity-50"
        >
          {testingProxy ? "⏳ Đang kiểm tra…" : "Kiểm tra proxy"}
        </button>
        {proxyTestMsg && (
          <span className="text-xs whitespace-pre-line text-muted flex-1">{proxyTestMsg}</span>
        )}
      </div>
      <p className="text-xs text-muted -mt-1">
        Dán proxy vào là <b>tự lưu ngay</b>, không cần bấm gì — lượt tải TIẾP THEO sẽ tự đi qua proxy
        (video đang tải dở thì bấm Thử lại để áp dụng). Bấm <b>Kiểm tra proxy</b> để xem proxy sống hay chết.
        <br />
        <b>Mở khoá site bị nhà mạng chặn</b> (vd bilibili.tv): thêm 1 proxy là tải được luôn, không cần
        đổi DNS hay VPN. Video khoá theo vùng thì dùng proxy ở nước ngoài. Để trống = không dùng proxy.
      </p>

      <Field label="YouTube API key — thêm nhiều key, hết quota tự nhảy key khác">
        <div className="space-y-2">
          {/* Danh sách key đã thêm, mỗi key 1 đèn xanh/đỏ */}
          {ytKeys.map((k, i) => {
            const st = ytStatuses[i] ?? "idle";
            const u = ytUsage?.keys[i];
            // % đã tiêu (ước tính) → thanh + màu cảnh báo khi gần cạn.
            const pct = u ? Math.min(100, Math.round((u.used / u.quota) * 100)) : 0;
            const barColor = pct >= 90 ? "bg-danger" : pct >= 70 ? "bg-warning" : "bg-success";
            return (
              <div key={`${k}-${i}`} className="flex items-center gap-2 px-3 py-2 rounded-md bg-surface border border-border">
                <span className="text-sm shrink-0 w-6 text-muted">#{i + 1}</span>
                <div className="flex-1 min-w-0">
                  <span className="font-mono text-xs text-fg truncate block" title={k}>{maskKey(k)}</span>
                  {u && (
                    <div className="flex items-center gap-1.5 mt-1" title={`Ước tính theo app: đã dùng ~${u.used.toLocaleString()} / ${u.quota.toLocaleString()} đơn vị hôm nay. YouTube không cho biết số thật — đây là số app tự đếm.`}>
                      <div className="h-1.5 flex-1 max-w-[120px] rounded bg-surface-2 overflow-hidden">
                        <div className={`h-full ${barColor}`} style={{ width: `${pct}%` }} />
                      </div>
                      <span className="text-[11px] text-muted shrink-0">
                        còn ~{u.remaining.toLocaleString()}
                      </span>
                    </div>
                  )}
                </div>
                <span className="text-xs shrink-0">
                  {st === "ok" && <span className="text-success font-medium">🟢 OK</span>}
                  {st === "checking" && <span className="text-warning">⏳…</span>}
                  {st === "bad" && (
                    <span className="text-danger font-medium" title={ytErrors[i] ?? ""}>
                      🔴 {(ytErrors[i] ?? "").includes("hết") || (ytErrors[i] ?? "").includes("quota") ? "Hết quota" : "Lỗi"}
                    </span>
                  )}
                  {st === "idle" && <span className="text-muted">—</span>}
                </span>
                <button
                  onClick={() => void checkKeyAt(i, k)}
                  className="px-2 py-1 rounded border border-border text-fg text-xs shrink-0"
                  title="Kiểm tra lại key này"
                >
                  Kiểm tra
                </button>
                <button
                  onClick={() => void removeKeyAt(i)}
                  className="px-2 py-1 rounded border border-border text-fg text-xs shrink-0"
                  title="Xoá key này"
                >
                  ✕
                </button>
              </div>
            );
          })}
          {/* Ô thêm key mới */}
          <div className="flex gap-2 items-stretch">
            <input
              type="text"
              value={newKey}
              onChange={e => setNewKey(e.target.value)}
              onKeyDown={e => { if (e.key === "Enter") void addKey(); }}
              placeholder="Dán key mới dạng AIzaSy... rồi bấm Thêm"
              className="flex-1 px-3 py-2 rounded-md bg-surface border border-border text-fg placeholder:text-muted font-mono text-xs"
              spellCheck={false}
              autoComplete="off"
            />
            <button
              onClick={() => void addKey()}
              disabled={!newKey.trim()}
              className="px-3 py-2 rounded-md bg-surface-2 border border-border text-fg shrink-0 disabled:opacity-50"
            >
              + Thêm key
            </button>
            {ytKeys.length > 0 && (
              <button
                onClick={() => void checkAllKeys()}
                className="px-3 py-2 rounded-md border border-border text-fg shrink-0"
                title="Kiểm tra lại tất cả key"
              >
                Kiểm tra tất cả
              </button>
            )}
          </div>
        </div>
      </Field>
      <p className="text-xs text-muted -mt-3">
        Lấy key miễn phí ở <b>console.cloud.google.com</b> → bật "YouTube Data API v3" → tạo API key.
        Thêm <b>nhiều key</b> để khi 1 key hết 10.000 lượt/ngày, app <b>tự nhảy sang key kế tiếp</b> (key
        hết sẽ hiện <span className="text-danger">🔴 Hết quota</span>). Để trống = dùng cách cũ (dò từng video, chậm).
      </p>
      {ytKeys.length > 0 && (
        <p className="text-xs text-muted -mt-2">
          Thanh <b>"còn ~…"</b> là <b>ước tính theo app</b> (app tự đếm số lượt đã gọi hôm nay) — YouTube
          không cho biết số quota thật, chỉ Google Cloud Console mới thấy chính xác. Số reset mỗi ngày
          {ytUsage?.day ? ` (mốc ${ytUsage.day} giờ Thái Bình Dương)` : ""}. Nếu key còn dùng ở nơi khác thì số thật đã tiêu sẽ cao hơn.
        </p>
      )}

      {/* ── Nút cứu hộ: YouTube đổi luật → bấm 1 nút là tự vá ─────────────── */}
      <div className="rounded-lg border border-border bg-surface p-4 space-y-2">
        <div className="flex items-center justify-between gap-3">
          <div>
            <div className="text-sm font-medium text-fg">🛠️ Tải bị lỗi / bị chặn?</div>
            <div className="text-xs text-muted">
              Bấm nút này là app tự vá: cập nhật bộ tải yt-dlp bản mới nhất (nightly),
              kiểm tra bộ giải mã Deno và khởi động lại gói chống bot (PO token).
              Fix được ~90% trường hợp YouTube vừa "đổi luật". Mất khoảng 10–30 giây.
            </div>
          </div>
          <button
            onClick={() => void fixNow()}
            disabled={fixing}
            className="px-4 py-2 rounded-md bg-accent text-accent-fg text-sm font-medium shrink-0 disabled:opacity-60"
          >
            {fixing ? "⏳ Đang sửa…" : "Sửa lỗi tải ngay"}
          </button>
        </div>
        {fixMsg && (
          <pre className="text-xs whitespace-pre-wrap font-sans text-muted border-t border-border pt-2 m-0">
            {fixMsg}
          </pre>
        )}
      </div>

      <Toggle
        label="Bật PO Token (giảm chặn bot, không cần cookie)"
        checked={settings.poTokenEnabled ?? false}
        onChange={v => set("poTokenEnabled", v)}
      />
      <p className="text-xs text-muted -mt-3">
        Lần đầu bật, app tải gói chống bot (~46MB) rồi chạy ngầm. Giúp đỡ bị "đòi robot" mà không cần cookie.
        Lưu ý: không đổi IP — tải cực nhiều vẫn nên dùng proxy. Lỗi gì thì tải vẫn chạy bình thường.
      </p>

      <div className="flex items-center justify-between gap-3 py-2 border-t border-border mt-2 pt-4">
        <div>
          <div className="text-sm font-medium text-fg">Bộ giải mã video (Deno)</div>
          <div className="text-xs text-muted">Bắt buộc để tải YouTube (giải câu đố JavaScript lấy link). App tự tải ~40MB lần đầu.</div>
        </div>
        <div className="shrink-0 text-sm flex items-center gap-2">
          {deno === "ready" && <span className="text-success font-medium">✅ Đã sẵn sàng</span>}
          {deno === "downloading" && <span className="text-warning">⏳ Đang tải…</span>}
          {(deno === "failed" || deno === "unknown") && (
            <>
              <span className="text-muted">{deno === "failed" ? "❌ Chưa có" : "⏳ Đang kiểm tra…"}</span>
              <button
                onClick={() => { void cmd.retryDeno(); setDeno("downloading"); }}
                className="px-2.5 py-1 rounded-md bg-surface-2 border border-border text-fg text-xs"
              >
                Tải lại
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

/**
 * Các trang có ô cookie riêng. `key` phải khớp ĐÚNG tên extractor mà Rust trả
 * về (`extractors::match_host`) — sai một chữ là app không tìm thấy ô, link
 * lặng lẽ rơi về ô chung.
 */
const SITE_COOKIE_SLOTS: { key: string; label: string }[] = [
  { key: "youtube", label: "YouTube" },
  { key: "tiktok", label: "TikTok" },
  { key: "douyin", label: "Douyin" },
  { key: "facebook", label: "Facebook" },
  { key: "instagram", label: "Instagram" },
  { key: "bilibili", label: "Bilibili" },
  { key: "twitter", label: "X (Twitter)" },
];

const COOKIE_BROWSERS = ["edge", "chrome", "firefox", "brave", "chromium", "vivaldi", "opera"];

/** Một dòng = một trang: chọn file cookies.txt riêng, hoặc lấy từ trình duyệt. */
function SiteCookieRow({
  siteKey,
  label,
  slot,
  onChange,
}: {
  siteKey: string;
  label: string;
  slot?: SiteCookie;
  onChange: (next: SiteCookie | null) => void;
}) {
  const file = slot?.file ?? "";
  const browser = slot?.browser ?? "";
  const isSet = !!file || !!browser;
  return (
    <div className="flex items-center gap-2">
      <span className="text-sm text-fg w-24 shrink-0" title={siteKey}>
        {label}
      </span>
      <input
        readOnly
        value={file}
        placeholder={browser ? `(trình duyệt: ${browser})` : "dùng ô chung"}
        className="flex-1 min-w-0 px-2 py-1.5 text-xs rounded-md bg-surface border border-border text-fg placeholder:text-muted"
      />
      <button
        onClick={async () => {
          const f = await cmd.pickFile();
          // Chọn file thì bỏ luôn lựa chọn trình duyệt — một ô một nguồn,
          // tránh cảnh "đặt file rồi mà vẫn đi lấy cookie trình duyệt".
          if (f) onChange({ file: f, browser: null });
        }}
        className="px-2 py-1.5 text-xs rounded-md bg-surface-2 border border-border text-fg shrink-0"
      >
        Chọn file
      </button>
      <select
        value={browser}
        disabled={!!file}
        onChange={e => {
          const v = e.target.value;
          if (!v) onChange(file ? { file, browser: null } : null);
          else onChange({ file: null, browser: v });
        }}
        className="px-2 py-1.5 text-xs rounded-md bg-surface border border-border text-fg shrink-0 disabled:opacity-50"
        title={file ? "Đang dùng file — bỏ file để chọn trình duyệt" : "Lấy cookie từ trình duyệt"}
      >
        <option value="">— trình duyệt —</option>
        {COOKIE_BROWSERS.map(b => (
          <option key={b} value={b}>
            {b}
          </option>
        ))}
      </select>
      <button
        onClick={() => onChange(null)}
        disabled={!isSet}
        className="px-2 py-1.5 text-xs rounded-md border border-border text-fg shrink-0 disabled:opacity-30"
        title={`Xoá ô ${label} — link ${label} sẽ dùng lại ô chung`}
      >
        ✕
      </button>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="block space-y-2">
      <span className="text-sm font-medium text-fg">{label}</span>
      {children}
    </label>
  );
}

function Toggle({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="flex items-center justify-between gap-3 py-2">
      <span className="text-sm text-fg">{label}</span>
      <input type="checkbox" checked={checked} onChange={e => onChange(e.target.checked)} className="h-4 w-4" />
    </label>
  );
}
