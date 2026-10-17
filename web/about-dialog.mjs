// SPDX-License-Identifier: AGPL-3.0-or-later
import { attachTabs, createDialog } from "/ui-components.mjs";

const element = (tag, className, text) => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = String(text);
  return node;
};

const requiredText = (value, name) => {
  if (typeof value !== "string" || value.length === 0) {
    throw new TypeError(`${name} must be a non-empty string.`);
  }
  return value;
};

function validateOptions({ labels, metadata, licenseTexts } = {}) {
  if (!labels || typeof labels !== "object") throw new TypeError("labels are required.");
  if (!metadata || typeof metadata !== "object") throw new TypeError("metadata are required.");
  if (!licenseTexts || typeof licenseTexts !== "object") throw new TypeError("licenseTexts are required.");

  for (const key of ["dialogTitle", "close", "tabList", "overviewTab", "agplTab", "fontLicensePrefix", "unicodeTab", "version", "buildDate", "sourceUrl", "agplHeading", "unicodeHeading"]) {
    requiredText(labels[key], `labels.${key}`);
  }
  for (const key of ["version", "buildDate", "sourceUrl"]) requiredText(metadata[key], `metadata.${key}`);
  requiredText(licenseTexts.agpl, "licenseTexts.agpl");
  requiredText(licenseTexts.unicode, "licenseTexts.unicode");
  if (!Array.isArray(licenseTexts.fonts) || licenseTexts.fonts.length !== 3) {
    throw new TypeError("licenseTexts.fonts must contain the Carlito, Caladea, and Liberation notices.");
  }
  const fonts = licenseTexts.fonts.map((font, index) => {
    if (!font || typeof font !== "object") throw new TypeError(`licenseTexts.fonts[${index}] is invalid.`);
    return {
      name: requiredText(font.name, `licenseTexts.fonts[${index}].name`),
      text: requiredText(font.text, `licenseTexts.fonts[${index}].text`),
    };
  });
  return { labels, metadata, licenseTexts: { agpl: licenseTexts.agpl, fonts, unicode: licenseTexts.unicode } };
}

/**
 * Build an offline About dialog from caller-owned metadata and complete license texts.
 *
 * Required labels: dialogTitle, close, tabList, overviewTab, agplTab,
 * fontLicensePrefix, unicodeTab, version, buildDate, sourceUrl, agplHeading,
 * and unicodeHeading. A font tab is named `${fontLicensePrefix} ${font.name}`.
 *
 * @param {{labels:Record<string,string>,metadata:{version:string,buildDate:string,buildDateIso?:string,sourceUrl:string},licenseTexts:{agpl:string,fonts:Array<{name:string,text:string}>,unicode:string}}} options
 * @returns {{element:HTMLDialogElement,open:Function,close:Function,dispose:Function}}
 */
export function createAboutDialog(options) {
  const { labels, metadata, licenseTexts } = validateOptions(options);
  const content = element("div", "about-dialog-content");
  const tablist = element("div", "about-dialog-tabs");
  const entries = [];

  const addTab = (name, heading, text, className = "") => {
    const tab = element("button", "about-dialog-tab", name);
    tab.type = "button";
    const panel = element("section", `about-dialog-panel ${className}`.trim());
    const panelHeading = element("h3", "about-dialog-panel-heading", heading);
    const license = element("pre", "about-dialog-license-text", text);
    license.tabIndex = 0;
    license.setAttribute("role", "region");
    license.setAttribute("aria-label", name);
    panel.append(panelHeading, license);
    entries.push({ tab, panel });
  };

  const overviewTab = element("button", "about-dialog-tab", labels.overviewTab);
  overviewTab.type = "button";
  const overviewPanel = element("section", "about-dialog-panel about-dialog-overview");
  const overviewHeading = element("h3", "about-dialog-panel-heading", labels.overviewTab);
  const metadataList = element("dl", "about-dialog-metadata");
  for (const [label, value, isoDay] of [
    [labels.version, metadata.version],
    [labels.buildDate, metadata.buildDate, metadata.buildDateIso],
    [labels.sourceUrl, metadata.sourceUrl],
  ]) {
    const term = element("dt", "about-dialog-metadata-label", label);
    const detail = element("dd", "about-dialog-metadata-value");
    const code = element("code", "about-dialog-metadata-text");
    if (typeof isoDay === "string" && /^\d{4}-\d{2}-\d{2}$/.test(isoDay)) {
      const time = element("time", "", value);
      time.dateTime = isoDay;
      code.append(time);
    } else code.textContent = value;
    detail.append(code);
    metadataList.append(term, detail);
  }
  overviewPanel.append(overviewHeading, metadataList);
  entries.push({ tab: overviewTab, panel: overviewPanel });

  addTab(labels.agplTab, labels.agplHeading, licenseTexts.agpl, "about-dialog-license-panel");
  for (const font of licenseTexts.fonts) {
    const name = `${labels.fontLicensePrefix} ${font.name}`;
    addTab(name, name, font.text, "about-dialog-license-panel");
  }
  addTab(labels.unicodeTab, labels.unicodeHeading, licenseTexts.unicode, "about-dialog-license-panel");

  const dialogContent = element("div", "about-dialog");
  tablist.setAttribute("aria-label", labels.tabList);
  dialogContent.append(tablist, ...entries.map(entry => entry.panel));
  content.append(dialogContent);

  let tabs;
  const dialog = createDialog({
    label: labels.dialogTitle,
    closeLabel: labels.close,
    content,
    initialFocus: () => overviewTab,
  });
  dialog.element.classList.add("about-dialog");
  tabs = attachTabs({ tablist, tabs: entries, label: labels.tabList });
  const onEscape = event => {
    if (event.key !== "Escape" || !dialog.element.open) return;
    event.preventDefault();
    dialog.close();
  };
  dialog.element.addEventListener("keydown", onEscape);

  let disposed = false;
  return Object.freeze({
    element: dialog.element,
    open(options) { if (!disposed) dialog.open(options); },
    close() { if (!disposed) dialog.close(); },
    dispose() {
      if (disposed) return;
      disposed = true;
      dialog.element.removeEventListener("keydown", onEscape);
      tabs.dispose();
      dialog.dispose();
    },
  });
}
