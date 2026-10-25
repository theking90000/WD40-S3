import assert from "node:assert/strict";
import test from "node:test";
import {
  languageModeStorageKey, languageStorageKey, persistLanguageMode, readLanguagePreference,
} from "./preference.ts";

test("automatic detection stays enabled until a manual language is selected", (t) => {
  const previousWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  const storage = new Map<string, string>();
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: {
      localStorage: {
        getItem: (key: string) => storage.get(key) ?? null,
        setItem: (key: string, value: string) => storage.set(key, value),
        removeItem: (key: string) => storage.delete(key),
      },
    },
  });
  t.after(() => {
    if (previousWindow) Object.defineProperty(globalThis, "window", previousWindow);
    else Reflect.deleteProperty(globalThis, "window");
  });

  assert.equal(readLanguagePreference(), "auto");
  storage.set(languageStorageKey, "fr");
  assert.equal(readLanguagePreference(), "auto", "a cached detection is not a manual choice");

  persistLanguageMode("fr");
  assert.equal(readLanguagePreference(), "fr");
  storage.set(languageStorageKey, "en");
  assert.equal(readLanguagePreference(), "en", "the detector owns the selected language cache");

  persistLanguageMode("auto");
  assert.equal(readLanguagePreference(), "auto");
  assert.equal(storage.get(languageStorageKey), "en", "returning to automatic keeps the detector cache");
  assert.equal(storage.has(languageModeStorageKey), false);

  storage.set(languageModeStorageKey, "manual");
  for (const invalid of ["de", "auto", "fr-BE", "", "invalid"]) {
    storage.set(languageStorageKey, invalid);
    assert.equal(readLanguagePreference(), "auto");
  }
  storage.delete(languageStorageKey);
  assert.equal(readLanguagePreference(), "auto");
  storage.set(languageStorageKey, "fr");
  storage.set(languageModeStorageKey, "invalid");
  assert.equal(readLanguagePreference(), "auto");
});

test("language preferences tolerate unavailable storage and prerendering", (t) => {
  const previousWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  t.after(() => {
    if (previousWindow) Object.defineProperty(globalThis, "window", previousWindow);
    else Reflect.deleteProperty(globalThis, "window");
  });
  Reflect.deleteProperty(globalThis, "window");
  assert.equal(readLanguagePreference(), "auto");
  assert.doesNotThrow(() => persistLanguageMode("fr"));

  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: {
      get localStorage() { throw new Error("Storage blocked"); },
    },
  });
  assert.equal(readLanguagePreference(), "auto");
  assert.doesNotThrow(() => persistLanguageMode("en"));
  assert.doesNotThrow(() => persistLanguageMode("auto"));
});
