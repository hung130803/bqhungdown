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
      <p className="text-xs text-muted -mt-3">
        Dán nhiều proxy (mỗi dòng 1 cái) — app tự xoay vòng và tự đổi proxy khi bị YouTube chặn.
        Nên dùng proxy <b>dân cư (residential)</b>; proxy datacenter thường bị chặn. Để trống = không dùng.
      </p>

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
