import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { BrowserRouter, NavLink, Routes, Route, useNavigate } from "react-router-dom";
import { ThemeProvider } from "@/components/ThemeProvider";
import { Logo } from "@/components/Logo";
import { UpdateBanner } from "@/components/UpdateBanner";
import { useSettingsStore } from "@/stores/useSettingsStore";
import { useQueueStore } from "@/stores/useQueueStore";
import { useClipboardStore } from "@/stores/useClipboardStore";
import { setupI18n, setLanguage, detectInitialLanguage } from "@/i18n";
import * as cmd from "@/ipc/commands";
import {
  onDownloadProgress,
  onDownloadState,
  onClipboardDetected,
  onNotificationClicked,
  onQueueUpdated,
} from "@/ipc/events";

import { PasteUrlPage } from "@/pages/PasteUrlPage";
import { QueuePage } from "@/pages/QueuePage";
import { HistoryPage } from "@/pages/HistoryPage";
import { SettingsPage } from "@/pages/SettingsPage";

export default function App() {
  return (
    <BrowserRouter>
      <Bootstrap>
        <Shell />
      </Bootstrap>
    </BrowserRouter>
  );
}

function Bootstrap({ children }: { children: React.ReactNode }) {
  const hydrate = useSettingsStore((s) => s.hydrate);
  const setQueueAll = useQueueStore((s) => s.setAll);
  const applyProgress = useQueueStore((s) => s.applyProgress);
  const applyState = useQueueStore((s) => s.applyState);
  const setClipboard = useClipboardStore((s) => s.setDetected);
  const isDismissed = useClipboardStore((s) => s.isDismissed);
  const navigate = useNavigate();
  const [ready, setReady] = useState(false);

  useEffect(() => {
    let unlistens: Array<() => void> = [];
    (async () => {
      setupI18n(detectInitialLanguage());
      try {
        const boot = await cmd.appBootstrap();
        hydrate(boot.settings);
        setQueueAll(boot.queue);
        if (boot.settings.language) setLanguage(boot.settings.language);
      } catch (e) {
        console.error("[bootstrap] app_bootstrap failed:", e);
      }

      unlistens.push(await onDownloadProgress((p) => applyProgress(p.shortId, p.progress)));
      unlistens.push(await onDownloadState((p) => applyState(p.shortId, p.state, p.errorMessage, p.outputPath)));
      unlistens.push(await onQueueUpdated((p) => setQueueAll(p.items)));
      unlistens.push(
        await onClipboardDetected((p) => {
          if (!isDismissed(p.url)) setClipboard(p.url, p.extractor);
        }),
      );
      unlistens.push(
        await onNotificationClicked((p) => {
          navigate(`/queue?focus=${encodeURIComponent(p.shortId)}`);
        }),
      );

      setReady(true);
    })();
    return () => {
      unlistens.forEach((fn) => fn());
    };
  }, [hydrate, setQueueAll, applyProgress, applyState, setClipboard, isDismissed, navigate]);

  if (!ready) {
    return (
      <div className="h-full grid place-items-center text-muted">
        <div className="flex flex-col items-center gap-3">
          <Logo size={56} />
          <span className="text-sm">Đang khởi tạo…</span>
        </div>
      </div>
    );
  }
  return <>{children}</>;
}

function Shell() {
  const settings = useSettingsStore((s) => s.settings);
  return (
    <ThemeProvider theme={settings?.theme ?? "system"}>
      <div className="min-h-full flex flex-col">
        <UpdateBanner />
        <Header />
        <main className="flex-1 px-6 py-6 max-w-5xl w-full mx-auto">
          <Routes>
            <Route path="/" element={<PasteUrlPage />} />
            <Route path="/queue" element={<QueuePage />} />
            <Route path="/history" element={<HistoryPage />} />
            <Route path="/settings" element={<SettingsPage />} />
          </Routes>
        </main>
      </div>
    </ThemeProvider>
  );
}

function Header() {
  const { t } = useTranslation();
  const queueCount = useQueueStore((s) => s.items.filter((i) => i.state === "downloading" || i.state === "queued" || i.state === "paused").length);

  const linkClass = ({ isActive }: { isActive: boolean }) =>
    `relative px-3.5 py-2 rounded-lg text-sm font-medium transition-colors ${
      isActive
        ? "bg-accent text-accent-fg shadow-sm"
        : "text-fg/80 hover:text-fg hover:bg-surface-2"
    }`;

  return (
    <header className="sticky top-0 z-20 border-b border-border bg-surface/95 backdrop-blur supports-[backdrop-filter]:bg-surface/80">
      <div className="max-w-5xl mx-auto px-6 py-3 flex items-center justify-between gap-4">
        <div className="flex items-center gap-3 min-w-0">
          <Logo size={36} />
          <div className="min-w-0">
            <div className="font-semibold text-fg leading-tight">BQHungDown</div>
            <div className="text-xs text-muted leading-tight">{t("app.tagline")}</div>
          </div>
        </div>

        <nav className="flex gap-1 items-center">
          <NavLink to="/" end className={linkClass}>
            {t("nav.home")}
          </NavLink>
          <NavLink to="/queue" className={linkClass}>
            <span className="inline-flex items-center gap-2">
              {t("nav.queue")}
              {queueCount > 0 && (
                <span className="inline-flex items-center justify-center min-w-[18px] h-[18px] px-1 text-[10px] font-bold rounded-full bg-accent text-accent-fg">
                  {queueCount}
                </span>
              )}
            </span>
          </NavLink>
          <NavLink to="/history" className={linkClass}>
            {t("nav.history")}
          </NavLink>
          <NavLink to="/settings" className={linkClass}>
            {t("nav.settings")}
          </NavLink>
        </nav>
      </div>
    </header>
  );
}
