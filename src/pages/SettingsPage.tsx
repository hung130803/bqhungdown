import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSettingsStore } from "@/stores/useSettingsStore";
import * as cmd from "@/ipc/commands";
import type { Settings, Theme, Language } from "@/types/models";
import { setLanguage } from "@/i18n";

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

      <Field label={t("settings.maxConcurrency")}>
        <input
          type="number"
          min={1}
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
            return (
              <div key={`${k}-${i}`} className="flex items-center gap-2 px-3 py-2 rounded-md bg-surface border border-border">
                <span className="text-sm shrink-0 w-6 text-muted">#{i + 1}</span>
                <span className="font-mono text-xs text-fg flex-1 truncate" title={k}>{maskKey(k)}</span>
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
