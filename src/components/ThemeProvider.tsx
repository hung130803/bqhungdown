import { useEffect } from "react";
import type { Theme } from "@/types/models";

export interface ThemeProviderProps {
  theme: Theme;
  children: React.ReactNode;
}

export function applyTheme(theme: Theme) {
  const root = document.documentElement;
  if (theme === "system") {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    root.setAttribute("data-theme", mq.matches ? "dark" : "light");
  } else {
    root.setAttribute("data-theme", theme);
  }
}

export function ThemeProvider({ theme, children }: ThemeProviderProps) {
  useEffect(() => {
    applyTheme(theme);
    if (theme !== "system") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const handler = () => applyTheme("system");
    try { mq.addEventListener("change", handler); }
    catch { mq.addListener(handler); }
    return () => {
      try { mq.removeEventListener("change", handler); }
      catch { mq.removeListener(handler); }
    };
  }, [theme]);
  return <>{children}</>;
}
