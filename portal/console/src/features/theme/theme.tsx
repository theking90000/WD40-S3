import { createContext, useCallback, useContext, useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";

export type ThemePreference = "light" | "dark" | "system";
export type Theme = "light" | "dark";

const storageKey = `wd40.portal.theme:${import.meta.env.BASE_URL}`;
const systemQuery = "(prefers-color-scheme: dark)";

export const themeScript = `(() => {
  let preference = "system";
  try {
    const stored = window.localStorage.getItem(${JSON.stringify(storageKey)});
    if (stored === "light" || stored === "dark") preference = stored;
  } catch {}
  document.documentElement.dataset.theme = preference === "system"
    ? (window.matchMedia(${JSON.stringify(systemQuery)}).matches ? "dark" : "light")
    : preference;
})();`;

function readPreference(): ThemePreference {
  try {
    const stored = window.localStorage.getItem(storageKey);
    if (stored === "light" || stored === "dark") return stored;
  } catch {
    // Keep the system default when browser storage is unavailable.
  }
  return "system";
}

function resolveTheme(preference: ThemePreference): Theme {
  return preference === "system"
    ? (window.matchMedia(systemQuery).matches ? "dark" : "light")
    : preference;
}

interface ThemeContextValue {
  preference: ThemePreference;
  theme: Theme;
  setPreference: (preference: ThemePreference) => void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

export function ThemeProvider({ children }: { children: ReactNode }) {
  const preferenceRef = useRef<ThemePreference>("system");
  const [state, setState] = useState<{ preference: ThemePreference; theme: Theme }>({
    preference: "system",
    theme: "light",
  });

  const applyPreference = useCallback((preference: ThemePreference) => {
    const theme = resolveTheme(preference);
    preferenceRef.current = preference;
    document.documentElement.dataset.theme = theme;
    setState({ preference, theme });
  }, []);

  const setPreference = useCallback((preference: ThemePreference) => {
    applyPreference(preference);
    try {
      window.localStorage.setItem(storageKey, preference);
    } catch {
      // Apply the preference for this page even when it cannot be persisted.
    }
  }, [applyPreference]);

  useEffect(() => {
    const media = window.matchMedia(systemQuery);
    function systemChanged() {
      if (preferenceRef.current === "system") applyPreference("system");
    }
    function storageChanged(event: StorageEvent) {
      if (event.key === storageKey || event.key === null) applyPreference(readPreference());
    }

    applyPreference(readPreference());
    media.addEventListener("change", systemChanged);
    window.addEventListener("storage", storageChanged);
    return () => {
      media.removeEventListener("change", systemChanged);
      window.removeEventListener("storage", storageChanged);
    };
  }, [applyPreference]);

  return (
    <ThemeContext.Provider value={{ ...state, setPreference }}>
      {children}
    </ThemeContext.Provider>
  );
}

export function useTheme() {
  const context = useContext(ThemeContext);
  if (!context) throw new Error("useTheme must be used within ThemeProvider");
  return context;
}
