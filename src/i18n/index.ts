import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import vi from "./vi.json";
import en from "./en.json";
import type { Language } from "@/types/models";

const STORAGE_KEY = "prodown:lang";

export function setupI18n(initialLang: Language = "vi") {
  if (!i18n.isInitialized) {
    void i18n.use(initReactI18next).init({
      resources: { vi: { translation: vi }, en: { translation: en } },
      lng: initialLang,
      fallbackLng: "vi",
      interpolation: { escapeValue: false },
      returnNull: false,
    });
  } else {
    void i18n.changeLanguage(initialLang);
  }
}

export function setLanguage(lang: Language) {
  void i18n.changeLanguage(lang);
  try { localStorage.setItem(STORAGE_KEY, lang); } catch { /* ignore */ }
}

export function detectInitialLanguage(): Language {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    if (v === "vi" || v === "en") return v;
  } catch { /* ignore */ }
  return "vi";
}

export default i18n;
