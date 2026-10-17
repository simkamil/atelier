// SPDX-License-Identifier: AGPL-3.0-or-later
import { createAboutDialog } from "../web/about-dialog.mjs";

const key = (target, name) => target.dispatchEvent(new KeyboardEvent("keydown", {
  key: name, bubbles: true, cancelable: true,
}));
const click = target => target.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
const element = (tag, text) => {
  const node = document.createElement(tag);
  if (text !== undefined) node.textContent = text;
  return node;
};

const labels = {
  dialogTitle: "À propos d’Atelier",
  close: "Fermer À propos",
  tabList: "Informations et licences",
  overviewTab: "Informations",
  agplTab: "Licence AGPL",
  fontLicensePrefix: "Licence de police",
  unicodeTab: "Licence Unicode",
  version: "Version",
  buildDate: "Date de build",
  sourceUrl: "Code source",
  agplHeading: "GNU Affero General Public License",
  unicodeHeading: "Unicode License V3",
};
const metadata = {
  version: "0.1.0-test+é",
  buildDate: "2026-10-13T10:12:13Z",
  sourceUrl: "https://example.invalid/source?select=me",
};
const longLicense = Array.from({ length: 90 }, (_, index) => `AGPL section ${index + 1}`).join("\n");
const licenseTexts = {
  agpl: `AGPL line one\r\nAGPL line two\n${longLicense}\n<svg onload=\"window.__licenseInjection = true\">literal</svg>`,
  fonts: [
    { name: "Carlito", text: "Carlito full notice\nCopyright Carlito\n" },
    { name: "Caladea", text: "Caladea full notice\nCopyright Caladea\n" },
    { name: "Liberation", text: "Liberation full notice\nCopyright Liberation\n" },
  ],
  unicode: "Unicode full notice\r\nCopyright Unicode\n",
};

export async function run({ assert = (condition, message) => { if (!condition) throw new Error(message); }, fixture } = {}) {
  const supplied = typeof fixture === "function" ? await fixture() : fixture;
  const host = supplied instanceof Element ? supplied : document.createElement("div");
  if (!supplied) document.body.append(host);
  const checks = [];
  const verify = (condition, message) => { assert(condition, message); checks.push(message); };
  let controller;
  const originalFetch = window.fetch;
  let fetchCount = 0;
  window.fetch = (...args) => { fetchCount++; return originalFetch(...args); };
  try {
    const opener = element("button", "Open about"); opener.type = "button"; host.append(opener); opener.focus();
    controller = createAboutDialog({ labels, metadata, licenseTexts });
    host.append(controller.element);
    controller.open({ returnFocusTo: opener });
    verify(controller.element.open && document.activeElement.textContent === labels.overviewTab, "dialog opens on the information tab with an accessible initial focus");
    const dialogRect = controller.element.getBoundingClientRect();
    verify(dialogRect.left >= 0 && dialogRect.right <= innerWidth && dialogRect.top >= 0 && dialogRect.bottom <= innerHeight, "dialog remains inside the available viewport");

    const textFor = value => [...controller.element.querySelectorAll(".about-dialog-metadata-value code")].find(node => node.textContent === value);
    verify(textFor(metadata.version)?.textContent === metadata.version && textFor(metadata.buildDate)?.textContent === metadata.buildDate && textFor(metadata.sourceUrl)?.textContent === metadata.sourceUrl, "version, build date, and source address are displayed exactly and remain selectable text");
    const plainTime = controller.element.querySelector(".about-dialog-metadata-value time");
    verify(plainTime === null, "a build date without an ISO day is shown as plain text without a time element");
    const isoController = createAboutDialog({ labels, metadata: { ...metadata, buildDate: "13 octobre 2026", buildDateIso: "2026-10-13" }, licenseTexts });
    const isoTime = isoController.element.querySelector(".about-dialog-metadata-value code time");
    verify(isoTime?.getAttribute("datetime") === "2026-10-13" && isoTime.textContent === "13 octobre 2026", "build date is rendered as a semantic time element with the ISO day and the formatted visible text");
    verify(!controller.element.querySelector("a[href]") && !controller.element.querySelector("[data-about-external-link]"), "source address is displayed without creating navigation");

    const tabs = [...controller.element.querySelectorAll('[role="tab"]')];
    const panels = [...controller.element.querySelectorAll('[role="tabpanel"]')];
    verify(tabs.length === 6 && tabs.every(tab => tab.getAttribute("aria-label") || tab.textContent.trim()) && tabs.every(tab => tab.hasAttribute("aria-controls")), "each notice has a named tab connected to its panel");
    const panelFor = tab => controller.element.querySelector(`#${tab.getAttribute("aria-controls")}`);
    const agplTab = tabs.find(tab => tab.textContent === labels.agplTab);
    click(agplTab);
    const agplText = panelFor(agplTab).querySelector("pre");
    verify(agplText.textContent === licenseTexts.agpl && agplText.getAttribute("aria-label") === labels.agplTab, "AGPL notice preserves every supplied character and has an accessible name");
    verify(getComputedStyle(agplText).overflowY === "auto" && agplText.scrollHeight > agplText.clientHeight, "long license content scrolls inside its bounded panel");
    verify(!agplText.querySelector("svg") && !window.__licenseInjection, "license text resembling markup stays literal and cannot execute");
    agplTab.focus(); key(agplTab, "Tab");
    verify(document.activeElement === agplText, "Tab moves from the selected notice tab into its scrollable text panel");
    key(agplText, "Tab");
    verify(document.activeElement === controller.element.querySelector(".ui-dialog-close"), "Tab remains trapped inside the modal dialog");

    for (const font of licenseTexts.fonts) {
      const tab = tabs.find(candidate => candidate.textContent === `${labels.fontLicensePrefix} ${font.name}`);
      click(tab);
      verify(tab.getAttribute("aria-selected") === "true" && panelFor(tab).querySelector("pre").textContent === font.text, `${font.name} notice is fully available through its selected tab`);
    }
    const unicodeTab = tabs.find(tab => tab.textContent === labels.unicodeTab);
    click(unicodeTab);
    verify(panelFor(unicodeTab).querySelector("pre").textContent === licenseTexts.unicode, "Unicode notice preserves all supplied characters");

    const overviewTab = tabs.find(tab => tab.textContent === labels.overviewTab);
    overviewTab.focus(); key(overviewTab, "ArrowRight");
    verify(agplTab.getAttribute("aria-selected") === "true" && document.activeElement === agplTab, "arrow keys move focus and activate the adjacent tab");
    key(agplTab, "End");
    verify(unicodeTab.getAttribute("aria-selected") === "true" && document.activeElement === unicodeTab, "End selects and focuses the last notice tab");
    key(unicodeTab, "Escape");
    verify(!controller.element.open && document.activeElement === opener, "Escape closes the modal and restores the opener focus");
    controller.open({ returnFocusTo: opener });
    verify(controller.element.open && document.activeElement === overviewTab, "the dialog can be opened again before disposal");
    controller.close();
    verify(!controller.element.open && document.activeElement === opener, "the returned close method closes the modal and restores the opener");
    controller.open({ returnFocusTo: opener });
    verify(controller.element.open, "the dialog can be reopened after a normal close");
    controller.dispose();
    const selectedBeforeClick = unicodeTab.getAttribute("aria-selected");
    click(agplTab);
    controller.open({ returnFocusTo: opener });
    verify(!controller.element.isConnected && !controller.element.open && unicodeTab.getAttribute("aria-selected") === selectedBeforeClick, "disposal removes the dialog and tabs listeners and prevents reopening");
    verify(fetchCount === 0, "creating, opening, and browsing notices performs no fetch");

    let rejectedMissingMetadata = false;
    try { createAboutDialog({ labels, metadata: { version: metadata.version, sourceUrl: metadata.sourceUrl }, licenseTexts }); }
    catch (error) { rejectedMissingMetadata = error instanceof TypeError; }
    verify(rejectedMissingMetadata, "missing build metadata is rejected rather than invented");
    return checks;
  } finally {
    window.fetch = originalFetch;
    controller?.dispose();
    if (!supplied) host.remove();
  }
}
