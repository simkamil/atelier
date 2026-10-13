// SPDX-License-Identifier: AGPL-3.0-or-later

const STORAGE_KEY = "atelier.theme";
const THEMES = new Set(["light", "dark", "system"]);

function readLocalStorage() {
  try { return globalThis.localStorage; }
  catch { return null; }
}

function readMatchMedia(matchMedia) {
  if (typeof matchMedia !== "function") return null;
  try { return matchMedia("(prefers-color-scheme: dark)"); }
  catch { return null; }
}

/**
 * Resolve the shell's theme preference into a concrete palette on `root`.
 * `onChange` receives a frozen `{ theme, resolvedTheme }` state after changes.
 * Labels and controls belong to the caller and its localization catalogue.
 */
export function createThemeController({
  root = globalThis.document?.documentElement,
  storage,
  matchMedia = globalThis.matchMedia?.bind(globalThis),
  onChange = () => {},
} = {}) {
  if (!root?.dataset || typeof root.dataset !== "object") {
    throw new TypeError("A root element with dataset is required.");
  }
  if (typeof onChange !== "function") throw new TypeError("onChange must be a function.");

  const preferenceStorage = storage === undefined ? readLocalStorage() : storage;
  const mediaFactory = matchMedia === globalThis.matchMedia && typeof matchMedia === "function"
    ? matchMedia.bind(globalThis) : matchMedia;
  const mediaQuery = readMatchMedia(mediaFactory);
  let systemPrefersDark = Boolean(mediaQuery?.matches);
  let preference = "system";
  try {
    const stored = preferenceStorage?.getItem(STORAGE_KEY);
    if (THEMES.has(stored)) preference = stored;
  } catch { /* Private mode and unavailable storage use the in-memory preference. */ }

  let disposed = false;
  let resolvedTheme = "light";
  const resolve = value => value === "system" ? (systemPrefersDark ? "dark" : "light") : value;

  function publish(notify) {
    const previousPreference = root.dataset.themePreference;
    const previousTheme = root.dataset.theme;
    resolvedTheme = resolve(preference);
    root.dataset.themePreference = preference;
    root.dataset.theme = resolvedTheme;
    if (notify && (previousPreference !== preference || previousTheme !== resolvedTheme)) {
      try { onChange(Object.freeze({ theme: preference, resolvedTheme })); }
      catch { /* A caller callback cannot interrupt palette application. */ }
    }
  }

  function handleSystemChange(event) {
    const next = Boolean(event?.matches ?? mediaQuery?.matches);
    if (next === systemPrefersDark) return;
    systemPrefersDark = next;
    if (preference === "system") publish(true);
  }

  let removeMediaListener = () => {};
  if (mediaQuery?.addEventListener) {
    mediaQuery.addEventListener("change", handleSystemChange);
    removeMediaListener = () => mediaQuery.removeEventListener?.("change", handleSystemChange);
  } else if (mediaQuery?.addListener) {
    mediaQuery.addListener(handleSystemChange);
    removeMediaListener = () => mediaQuery.removeListener?.(handleSystemChange);
  }

  publish(false);

  return Object.freeze({
    getTheme() { return preference; },
    getResolvedTheme() { return resolvedTheme; },
    setTheme(theme) {
      if (disposed) throw new TypeError("Theme controller has been disposed.");
      if (!THEMES.has(theme)) throw new TypeError("Theme must be light, dark, or system.");
      const changed = theme !== preference;
      preference = theme;
      try { preferenceStorage?.setItem(STORAGE_KEY, theme); }
      catch { /* The theme remains active for this session if storage is unavailable. */ }
      publish(changed);
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      removeMediaListener();
    },
  });
}
