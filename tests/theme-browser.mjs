// SPDX-License-Identifier: AGPL-3.0-or-later
import { createThemeController } from "/theme.mjs";

class TestMediaQuery {
  matches;
  listeners = new Set();
  constructor(matches) { this.matches = matches; }
  addEventListener(type, listener) { if (type === "change") this.listeners.add(listener); }
  removeEventListener(type, listener) { if (type === "change") this.listeners.delete(listener); }
  setMatches(matches) {
    if (matches === this.matches) return;
    this.matches = matches;
    for (const listener of this.listeners) listener({ matches });
  }
}

export async function run({ assert, fixture }) {
  if (typeof assert !== "function" || !fixture?.append) throw new TypeError("A browser assert callback and visible DOM fixture are required.");
  let sheet = [...document.querySelectorAll('link[rel="stylesheet"]')]
    .find(link => new URL(link.href, location.href).pathname === "/theme.css");
  if (!sheet) {
    sheet = document.createElement("link");
    sheet.rel = "stylesheet";
    sheet.href = "/theme.css";
    const loaded = new Promise((resolve, reject) => {
      sheet.addEventListener("load", resolve, { once: true });
      sheet.addEventListener("error", () => reject(new Error("Theme stylesheet failed to load.")), { once: true });
    });
    document.head.append(sheet);
    await loaded;
  }

  const root = document.documentElement;
  const previousTheme = root.getAttribute("data-theme");
  const previousPreference = root.getAttribute("data-theme-preference");
  const probe = document.createElement("div");
  probe.setAttribute("aria-hidden", "true");
  probe.textContent = "Atelier theme probe";
  Object.assign(probe.style, {
    display: "block", color: "var(--chrome-ink)", backgroundColor: "var(--chrome-background)",
    border: "1px solid var(--chrome-border)", padding: "4px", margin: "4px",
  });
  const canvas = document.createElement("canvas");
  canvas.setAttribute("aria-hidden", "true");
  canvas.width = 1;
  canvas.height = 1;
  const context = canvas.getContext("2d");
  context.fillStyle = "#fff";
  context.fillRect(0, 0, 1, 1);
  const documentPixel = [...context.getImageData(0, 0, 1, 1).data];
  fixture.append(probe, canvas);

  const media = new TestMediaQuery(false);
  const controller = createThemeController({ root, storage: null, matchMedia: () => media });
  const isForcedColors = matchMedia("(forced-colors: active)").matches;
  try {
    assert(controller.getTheme() === "system", "theme preference defaults to system");
    assert(controller.getResolvedTheme() === "light", "system uses the injected current preference");
    const light = getComputedStyle(probe).color;
    if (isForcedColors) {
      assert(getComputedStyle(root).getPropertyValue("--chrome-ink").trim() === "CanvasText",
        "forced-colors maps the light shell ink token to CanvasText");
    } else assert(light === "rgb(23, 42, 42)", "real DOM applies the Atelier light ink token");
    const lightSurface = getComputedStyle(probe).backgroundColor;
    if (isForcedColors) {
      assert(getComputedStyle(root).getPropertyValue("--chrome-background").trim() === "Canvas",
        "forced-colors maps the light shell surface token to Canvas");
    } else assert(lightSurface === "rgb(242, 245, 241)", "real DOM applies the light chrome surface token");

    controller.setTheme("dark");
    const dark = getComputedStyle(probe).color;
    assert(controller.getResolvedTheme() === "dark", "dark preference resolves immediately");
    if (isForcedColors) assert(getComputedStyle(root).getPropertyValue("--chrome-ink").trim() === "CanvasText",
      "forced-colors preserves system ink colors when dark is selected");
    else {
      assert(dark === "rgb(238, 244, 239)", "dark preference updates real DOM text immediately");
      assert(getComputedStyle(probe).backgroundColor === "rgb(23, 31, 27)", "dark palette updates shell surfaces");
    }

    controller.setTheme("system");
    media.setMatches(true);
    assert(controller.getTheme() === "system" && controller.getResolvedTheme() === "dark",
      "system preference follows a live media change");
    media.setMatches(false);
    assert(controller.getResolvedTheme() === "light", "live media change restores the light system palette");

    if (isForcedColors) {
      const inkToken = getComputedStyle(root).getPropertyValue("--chrome-ink").trim();
      const accentToken = getComputedStyle(root).getPropertyValue("--chrome-accent").trim();
      assert(inkToken === "CanvasText", "forced-colors maps shell ink to CanvasText");
      assert(accentToken === "Highlight", "forced-colors maps the shell accent to Highlight");
      assert(getComputedStyle(probe).color !== "rgb(23, 42, 42)", "forced-colors overrides real DOM text colors");
    } else {
      const forcedRule = [...document.styleSheets].some(styleSheet => {
        try { return [...styleSheet.cssRules].some(rule => String(rule.cssText).includes("forced-colors: active")); }
        catch { return false; }
      });
      assert(forcedRule, "theme stylesheet declares a forced-colors palette for the high-contrast run");
    }

    const canvasBefore = getComputedStyle(canvas);
    const canvasBackground = canvasBefore.backgroundColor;
    const canvasFilter = canvasBefore.filter;
    controller.setTheme("dark");
    const canvasAfter = getComputedStyle(canvas);
    assert(canvasAfter.backgroundColor === canvasBackground && canvasAfter.filter === canvasFilter,
      "theme changes do not recolor or filter document Canvas elements");
    assert([...context.getImageData(0, 0, 1, 1).data].every((value, index) => value === documentPixel[index]),
      "theme changes leave Canvas document pixels byte-for-byte unchanged");
  } finally {
    controller.dispose();
    probe.remove();
    canvas.remove();
    if (previousTheme === null) root.removeAttribute("data-theme");
    else root.setAttribute("data-theme", previousTheme);
    if (previousPreference === null) root.removeAttribute("data-theme-preference");
    else root.setAttribute("data-theme-preference", previousPreference);
  }
}
