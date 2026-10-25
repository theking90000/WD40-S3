export const supportedLanguages = ["fr", "en"] as const;
export type Language = typeof supportedLanguages[number];
export type LanguagePreference = "auto" | Language;

const baseUrl = import.meta.env?.BASE_URL ?? "/";
export const languageStorageKey = `wd40.portal.language:${baseUrl}`;
export const languageModeStorageKey = `wd40.portal.languageMode:${baseUrl}`;

export function isLanguage(value: unknown): value is Language {
  return value === "fr" || value === "en";
}

export function isLanguagePreference(value: unknown): value is LanguagePreference {
  return value === "auto" || isLanguage(value);
}

export function readLanguagePreference(): LanguagePreference {
  try {
    if (window.localStorage.getItem(languageModeStorageKey) === "manual") {
      const language = window.localStorage.getItem(languageStorageKey);
      if (isLanguage(language)) return language;
    }
  } catch {
    // Use browser detection when storage is unavailable.
  }
  return "auto";
}

// The detector owns the cached language; the application only persists the mode.
export function persistLanguageMode(preference: LanguagePreference) {
  try {
    if (preference === "auto") window.localStorage.removeItem(languageModeStorageKey);
    else window.localStorage.setItem(languageModeStorageKey, "manual");
  } catch {
    // The preference still applies in the current tab.
  }
}
