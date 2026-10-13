// SPDX-License-Identifier: AGPL-3.0-or-later
import assert from "node:assert/strict";
import { createThemeController } from "../web/theme.mjs";

class Root {
  dataset = {};
}

class Storage {
  values = new Map();
  failRead = false;
  failWrite = false;
  getItem(key) {
    if (this.failRead) throw new Error("storage unavailable");
    return this.values.get(key) ?? null;
  }
  setItem(key, value) {
    if (this.failWrite) throw new Error("storage unavailable");
    this.values.set(key, value);
  }
}

class MediaQuery {
  matches;
  listeners = new Set();
  constructor(matches) { this.matches = matches; }
  addEventListener(type, listener) { if (type === "change") this.listeners.add(listener); }
  removeEventListener(type, listener) { if (type === "change") this.listeners.delete(listener); }
  setMatches(matches) {
    if (this.matches === matches) return;
    this.matches = matches;
    for (const listener of this.listeners) listener({ matches });
  }
}

const media = new MediaQuery(true);
const storage = new Storage();
const root = new Root();
const changes = [];
const controller = createThemeController({
  root, storage, matchMedia: () => media,
  onChange: state => changes.push(state),
});
assert.equal(controller.getTheme(), "system");
assert.equal(controller.getResolvedTheme(), "dark", "system resolves the current OS preference");
assert.equal(root.dataset.themePreference, "system");
assert.equal(root.dataset.theme, "dark");
assert.equal(changes.length, 0, "initialization does not emit a change");

media.setMatches(false);
assert.equal(controller.getResolvedTheme(), "light", "system follows a live OS change");
assert.deepEqual(changes.at(-1), { theme: "system", resolvedTheme: "light" });
controller.setTheme("dark");
assert.equal(controller.getTheme(), "dark");
assert.equal(controller.getResolvedTheme(), "dark");
assert.equal(storage.values.get("atelier.theme"), "dark");
const explicitChanges = changes.length;
media.setMatches(true);
assert.equal(controller.getResolvedTheme(), "dark", "explicit theme ignores OS palette changes");
assert.equal(changes.length, explicitChanges, "irrelevant OS changes do not notify");
controller.setTheme("system");
assert.equal(controller.getResolvedTheme(), "dark");
assert.equal(storage.values.get("atelier.theme"), "system");
media.setMatches(false);
assert.equal(root.dataset.theme, "light");
assert.equal(changes.at(-1).resolvedTheme, "light");
assert.throws(() => controller.setTheme("sepia"), TypeError);
controller.dispose();
controller.dispose();
assert.equal(media.listeners.size, 0, "dispose removes the media listener idempotently");
const disposedTheme = controller.getResolvedTheme();
media.setMatches(true);
assert.equal(controller.getResolvedTheme(), disposedTheme, "disposed controllers ignore late events");
assert.throws(() => controller.setTheme("light"), TypeError);

const invalidStorage = new Storage();
invalidStorage.values.set("atelier.theme", "unknown");
const invalidController = createThemeController({ root: new Root(), storage: invalidStorage,
  matchMedia: () => new MediaQuery(false) });
assert.equal(invalidController.getTheme(), "system", "invalid persisted preference falls back to system");
assert.equal(invalidController.getResolvedTheme(), "light");
invalidController.dispose();

const restoredStorage = new Storage();
restoredStorage.values.set("atelier.theme", "dark");
const restoredController = createThemeController({ root: new Root(), storage: restoredStorage,
  matchMedia: () => new MediaQuery(false) });
assert.equal(restoredController.getTheme(), "dark", "a valid persisted preference is restored");
assert.equal(restoredController.getResolvedTheme(), "dark");
restoredController.dispose();

const failingStorage = new Storage();
failingStorage.failRead = true;
const failureController = createThemeController({ root: new Root(), storage: failingStorage,
  matchMedia: () => new MediaQuery(false) });
assert.equal(failureController.getTheme(), "system", "storage read errors use the default preference");
failingStorage.failWrite = true;
failureController.setTheme("dark");
assert.equal(failureController.getResolvedTheme(), "dark", "storage write errors do not block the palette");
failureController.dispose();

const legacyMedia = {
  matches: false,
  listener: null,
  addListener(listener) { this.listener = listener; },
  removeListener(listener) { if (this.listener === listener) this.listener = null; },
};
const legacyController = createThemeController({ root: new Root(), storage: null,
  matchMedia: () => legacyMedia });
legacyController.setTheme("system");
legacyMedia.matches = true;
legacyMedia.listener({ matches: true });
assert.equal(legacyController.getResolvedTheme(), "dark", "legacy media query listeners are supported");
legacyController.dispose();
assert.equal(legacyMedia.listener, null);

const badCallbackController = createThemeController({ root: new Root(), storage: null,
  matchMedia: () => new MediaQuery(false), onChange() { throw new Error("caller failure"); } });
badCallbackController.setTheme("dark");
assert.equal(badCallbackController.getResolvedTheme(), "dark", "callback failure cannot interrupt theme state");
badCallbackController.dispose();

console.log(JSON.stringify({ status: "passed", checks: [
  "preference and resolved theme are distinct and published to root data attributes",
  "live system preference updates while explicit palettes ignore OS changes",
  "preferences restore and persist under atelier.theme; invalid/storage-failing values fall back safely",
  "modern and legacy media listeners are removed on idempotent disposal",
  "invalid selections and throwing callbacks do not corrupt controller state",
] }));
