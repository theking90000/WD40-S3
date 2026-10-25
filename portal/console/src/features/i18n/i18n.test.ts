import assert from "node:assert/strict";
import test from "node:test";
import { applyLanguagePreference, createPortalI18n } from "./i18n.ts";
import { languageStorageKey } from "./preference.ts";

test("browser detection, manual choices and automatic caching respect supported languages", async (t) => {
  const previousWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  const previousNavigator = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  const storage = new Map<string, string>([[languageStorageKey, "en"]]);
  let languages = ["fr-BE", "en"];
  let storageBlocked = false;
  const checkStorage = () => { if (storageBlocked) throw new Error("Storage blocked"); };
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: {
      location: { pathname: "/en/", search: "?lng=en", hash: "#lng=en", hostname: "en.example.com" },
      localStorage: {
        getItem: (key: string) => { checkStorage(); return storage.get(key) ?? null; },
        setItem: (key: string, value: string) => { checkStorage(); storage.set(key, value); },
        removeItem: (key: string) => { checkStorage(); storage.delete(key); },
      },
    },
  });
  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    value: { get languages() { return languages; }, get language() { return languages[0]; } },
  });
  t.after(() => {
    if (previousWindow) Object.defineProperty(globalThis, "window", previousWindow);
    else Reflect.deleteProperty(globalThis, "window");
    if (previousNavigator) Object.defineProperty(globalThis, "navigator", previousNavigator);
    else Reflect.deleteProperty(globalThis, "navigator");
  });

  const instance = createPortalI18n();
  assert.equal(instance.isInitialized, true);
  assert.equal(instance.resolvedLanguage, "fr");
  assert.equal(storage.get(languageStorageKey), "en", "initialization must not overwrite the saved choice");
  assert.equal(instance.t($ => $.language.label), "Langue");

  await applyLanguagePreference(instance, "auto");
  assert.equal(instance.resolvedLanguage, "fr", "regional French wins over later English and URL hints");
  assert.equal(storage.get(languageStorageKey), "fr-BE");

  for (const [browserLanguages, expected] of [
    [["nl-BE", "fr-FR"], "fr"],
    [["fr-CA", "en-US"], "fr"],
    [["en-GB", "fr-BE"], "en"],
    [["de-DE"], "en"],
    [[], "en"],
  ] as const) {
    languages = [...browserLanguages];
    await applyLanguagePreference(instance, "auto");
    assert.equal(instance.resolvedLanguage, expected);
    assert.equal(storage.get(languageStorageKey), instance.language);
  }

  languages = ["en-US"];
  await applyLanguagePreference(instance, "fr");
  assert.equal(instance.resolvedLanguage, "fr", "manual French overrides the browser");
  assert.equal(storage.get(languageStorageKey), "fr");
  languages = ["fr-BE"];
  await applyLanguagePreference(instance, "en");
  assert.equal(instance.t($ => $.language.label), "Language");
  assert.equal(storage.get(languageStorageKey), "en");
  await applyLanguagePreference(instance, "auto");
  assert.equal(instance.resolvedLanguage, "fr", "automatic mode ignores a previous manual language cache");

  storageBlocked = true;
  await applyLanguagePreference(instance, "en");
  assert.equal(instance.resolvedLanguage, "en", "failed persistence must not prevent an in-memory choice");
  await applyLanguagePreference(instance, "auto");
  assert.equal(instance.resolvedLanguage, "fr");

  const secondInstance = createPortalI18n();
  assert.equal(secondInstance.resolvedLanguage, "fr", "another prerender starts in French");
  await applyLanguagePreference(instance, "en");
  instance.removeResourceBundle("en", "common");
  assert.equal(instance.t($ => $.language.label), "Langue", "missing English messages fall back to French");
  assert.equal(secondInstance.resolvedLanguage, "fr", "instances do not share the active language");
});
