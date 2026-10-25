import { createInstance } from "i18next";
import type { i18n } from "i18next";
import LanguageDetector from "i18next-browser-languagedetector";
import type { CustomDetector } from "i18next-browser-languagedetector";
import { initReactI18next } from "react-i18next";
import french from "../../locales/fr/common.ts";
import english from "../../locales/en/common.ts";
import { languageStorageKey, supportedLanguages } from "./preference.ts";
import type { LanguagePreference } from "./preference.ts";

export function createPortalI18n() {
  const instance = createInstance();
  instance.use(LanguageDetector).use(initReactI18next);
  // Inline resources and synchronous initialization keep prerendering and hydration identical.
  void instance.init({
    lng: "fr",
    supportedLngs: [...supportedLanguages],
    load: "languageOnly",
    cleanCode: true,
    fallbackLng: ["en", "fr"],
    defaultNS: "common",
    ns: ["common"],
    resources: { fr: { common: french }, en: { common: english } },
    enableSelector: true,
    initAsync: false,
    interpolation: { escapeValue: false },
    react: { useSuspense: false },
    // Do not overwrite an existing browser preference with the prerendered French language.
    detection: { order: [], caches: [], lookupLocalStorage: languageStorageKey },
  });
  return instance;
}

export function applyLanguagePreference(instance: i18n, preference: LanguagePreference) {
  const detector = instance.services.languageDetector as LanguageDetector;
  detector.init(instance.services, {
    order: preference === "auto" ? ["navigator"] : ["localStorage", "navigator"],
    lookupLocalStorage: languageStorageKey,
    caches: ["localStorage"],
  });

  // Storage may fail after the detector's initial availability probe, for example at quota.
  const storage = detector.detectors.localStorage as CustomDetector;
  detector.addDetector({
    ...storage,
    lookup(options) {
      try { return storage.lookup(options); } catch { return undefined; }
    },
    cacheUserLanguage(language, options) {
      try { storage.cacheUserLanguage?.(language, options); } catch { /* Keep the in-memory choice. */ }
    },
  });

  return instance.changeLanguage(preference === "auto" ? undefined : preference);
}
