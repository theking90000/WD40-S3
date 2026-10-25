import { createContext, useCallback, useContext, useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { I18nextProvider } from "react-i18next";
import { applyLanguagePreference, createPortalI18n } from "./i18n.ts";
import {
  isLanguage, languageModeStorageKey, languageStorageKey, persistLanguageMode, readLanguagePreference,
} from "./preference.ts";
import type { Language, LanguagePreference } from "./preference.ts";

interface LanguageContextValue {
  preference: LanguagePreference;
  language: Language;
  setPreference: (preference: LanguagePreference) => void;
}

const LanguageContext = createContext<LanguageContextValue | null>(null);

export function LanguageProvider({ children }: { children: ReactNode }) {
  const [instance] = useState(createPortalI18n);
  const [preference, setPreferenceState] = useState<LanguagePreference>("auto");
  const [language, setLanguage] = useState<Language>("fr");
  const preferenceRef = useRef<LanguagePreference>("auto");

  const setPreference = useCallback((next: LanguagePreference) => {
    preferenceRef.current = next;
    void applyLanguagePreference(instance, next);
    persistLanguageMode(next);
    setPreferenceState(next);
  }, [instance]);

  useEffect(() => {
    function languageChanged() {
      const resolved = instance.resolvedLanguage;
      const next = isLanguage(resolved) ? resolved : "fr";
      document.documentElement.lang = next;
      setLanguage(next);
    }
    function browserLanguageChanged() {
      if (preferenceRef.current === "auto") setPreference("auto");
    }
    function storageChanged(event: StorageEvent) {
      if (event.key !== null && event.key !== languageStorageKey && event.key !== languageModeStorageKey) return;
      try {
        if (event.storageArea && event.storageArea !== window.localStorage) return;
      } catch {
        return;
      }
      const next = readLanguagePreference();
      if (next !== preferenceRef.current) setPreference(next);
    }

    instance.on("languageChanged", languageChanged);
    // Keep French for prerendering and the first client render; detect only after hydration.
    setPreference(readLanguagePreference());
    window.addEventListener("languagechange", browserLanguageChanged);
    window.addEventListener("storage", storageChanged);
    return () => {
      instance.off("languageChanged", languageChanged);
      window.removeEventListener("languagechange", browserLanguageChanged);
      window.removeEventListener("storage", storageChanged);
    };
  }, [instance, setPreference]);

  return (
    <I18nextProvider i18n={instance}>
      <LanguageContext.Provider value={{ preference, language, setPreference }}>
        {children}
      </LanguageContext.Provider>
    </I18nextProvider>
  );
}

export function useLanguage() {
  const context = useContext(LanguageContext);
  if (!context) throw new Error("useLanguage must be used within LanguageProvider");
  return context;
}
