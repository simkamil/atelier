// SPDX-License-Identifier: AGPL-3.0-or-later
import { createRibbon } from "/ribbon.mjs";
import { createThemeController } from "/theme.mjs";
import { createDialog, createDropdown, createTooltip } from "/ui-components.mjs";

/** Reuses the existing command nodes: their state and business listeners stay authoritative. */
export function createEditorShell({ toolbar, messages, navigationAvailable = true, symbolsAvailable = false, imageAvailable = false, aboutAvailable = false, propertiesAvailable = false, onProperties = () => {}, onSymbols = () => {}, onImage = () => {}, onAbout = () => {}, onZoom = () => {}, onSearch = () => {}, onReplace = () => {}, onStatistics = () => {} }) {
  const doc = toolbar.ownerDocument;
  const t = key => messages.get(key) ?? key;
  const byId = id => doc.getElementById(id);
  const command = (id, label, keyTip, element) => ({ id, label: t(label), keyTip, element });
  const future = (id, label, keyTip) => ({ id, label: t(label), keyTip, disabled: true });
  const status = byId("editor-status");
  const unavailable = new Set();
  const options = toolbar.querySelector(".editor-options");
  const zoomCommand = doc.createElement("button");
  zoomCommand.type = "button";
  zoomCommand.id = "zoom-command";
  zoomCommand.textContent = t("zoom-title");
  zoomCommand.disabled = true;
  zoomCommand.addEventListener("click", onZoom);
  const toolCommand = (id, label, listener) => {
    const element = doc.createElement("button");
    element.type = "button"; element.id = id; element.textContent = t(label); element.disabled = true;
    element.addEventListener("click",listener); return element;
  };
  const searchCommand = toolCommand("search-command","searchTitle",onSearch);
  const replaceCommand = toolCommand("replace-command","replaceTitle",onReplace);
  const statisticsCommand = toolCommand("statistics-command","statisticsDialogTitle",onStatistics);
  const symbolsCommand = toolCommand("symbols-command","symbolsTitle",onSymbols);
  const imageCommand = toolCommand("image-command","ribbon-image",onImage);
  const aboutCommand = toolCommand("about-command","aboutTitle",onAbout);
  aboutCommand.disabled = false;
  const propertiesCommand = toolCommand("properties-command","propertiesTitle",onProperties);
  const localizedNavigation = navigationAvailable && ["searchTitle", "replaceTitle", "statisticsDialogTitle", "statisticsScopeDocument"]
    .every(key => typeof messages.get(key) === "string");
  const stateRow = doc.createElement("div");
  stateRow.className = "editor-command-status";
  stateRow.append(status);
  toolbar.after(stateRow);

  const preferenceContent = doc.createElement("div");
  preferenceContent.className = "shell-preferences";
  const theme = createThemeController({ root: doc.documentElement });
  const choice = createDropdown({
    label: t("theme-label"), value: theme.getTheme(),
    options: ["light", "dark", "system"].map(value => ({ value, label: t(`theme-${value}`) })),
    onChange: value => theme.setTheme(value),
  });
  const choiceLabel = doc.createElement("p");
  choiceLabel.className = "preference-label";
  choiceLabel.textContent = t("theme-label");
  preferenceContent.append(choiceLabel, choice.element);
  const keyboardHelp = doc.createElement("p");
  keyboardHelp.textContent = t("ribbon-keyboard-help");
  preferenceContent.append(keyboardHelp);
  const preferences = createDialog({ label: t("appearance-title"), content: preferenceContent, closeLabel: t("action-close") });
  doc.body.append(preferences.element);

  const tabs = [
    { id: "file", label: t("ribbon-file"), keyTip: "F", groups: [{ id: "files", label: t("ribbon-files-group"), commands: [
      command("new", "action-new", "N", byId("new-document")),
      command("open", "action-open", "O", byId("open-document")),
      command("open-docx", "action-open-docx", "D", byId("open-docx")),
      command("save", "action-save", "S", byId("save-document")),
      ...(propertiesAvailable ? [command("properties", "propertiesTitle", "R", propertiesCommand)] : []),
      ...(aboutAvailable ? [command("about", "aboutTitle", "P", aboutCommand)] : []),
    ] }] },
    { id: "home", label: t("ribbon-home"), keyTip: "A", groups: [
      { id: "history", label: t("ribbon-history-group"), commands: [command("undo", "action-undo", "U", byId("undo")), command("redo", "action-redo", "R", byId("redo"))] },
      { id: "editing", label: t("ribbon-editing-group"), commands: [command("options", "options-label", "E", options), ...(localizedNavigation ? [command("search", "searchTitle", "C", searchCommand), command("replace", "replaceTitle", "H", replaceCommand)] : [])] },
      { id: "format", label: t("ribbon-format-group"), commands: [future("font", "ribbon-font", "P"), future("paragraph", "ribbon-paragraph", "G")] },
    ] },
    { id: "insert", label: t("ribbon-insert"), keyTip: "I", groups: [{ id: "objects", label: t("ribbon-objects-group"), commands: [future("table", "ribbon-table", "T"), imageAvailable ? command("image", "ribbon-image", "M", imageCommand) : future("image", "ribbon-image", "M"), ...(symbolsAvailable ? [command("symbols", "symbolsTitle", "S", symbolsCommand)] : [])] }] },
    { id: "layout", label: t("ribbon-layout"), keyTip: "M", groups: [{ id: "page", label: t("ribbon-page-group"), commands: [future("margins", "ribbon-margins", "G"), future("orientation", "ribbon-orientation", "O")] }] },
    { id: "references", label: t("ribbon-references"), keyTip: "R", groups: [{ id: "references", label: t("ribbon-references-group"), commands: [future("fields", "ribbon-fields", "C"), future("bookmarks", "ribbon-bookmarks", "S")] }] },
    { id: "view", label: t("ribbon-view"), keyTip: "V", groups: [
      { id: "appearance", label: t("ribbon-appearance-group"), commands: [{ id: "appearance", label: t("appearance-title"), keyTip: "T" }] },
      { id: "zoom", label: t("zoom-group"), commands: [command("zoom", "zoom-title", "Z", zoomCommand)] },
      ...(localizedNavigation ? [{ id: "statistics", label: t("statisticsScopeDocument"), commands: [command("statistics", "statisticsDialogTitle", "S", statisticsCommand)] }] : []),
    ] },
  ];
  toolbar.id = "ribbon";
  const ribbon = createRibbon({ root: toolbar, tabs, initialTab: "file",
    labels: { collapse: t("ribbon-collapse"), expand: t("ribbon-expand"), navigation: t("toolbar-label") },
    onCommand: id => { if (id === "appearance") preferences.open({ returnFocusTo: doc.activeElement }); },
  });
  for (const button of toolbar.querySelectorAll("button:disabled")) {
    if (button.id) continue;
    button.title = t("command-unavailable");
  }
  const help = createTooltip({ trigger: toolbar.querySelector('[role="tab"]'), text: t("ribbon-keyboard-help"), delay: 500 });
  return Object.freeze({
    theme, ribbon,
    setZoomEnabled(enabled) { zoomCommand.disabled = !enabled; ribbon.refresh(); },
    setNavigationEnabled(enabled) { for (const c of [searchCommand, replaceCommand, statisticsCommand]) if (!unavailable.has(c)) c.disabled = !enabled; ribbon.refresh(); },
    setPropertiesEnabled(enabled) { if (!unavailable.has(propertiesCommand)) propertiesCommand.disabled = !enabled; ribbon.refresh(); },
    /** Commands the engine in use does not offer stay disabled; the reason is their tooltip and description (R7a, engine v2). */
    setUnavailable(navigationReason, propertiesReason) {
      for (const [element, reason] of [[searchCommand, navigationReason], [replaceCommand, navigationReason], [statisticsCommand, navigationReason], [propertiesCommand, propertiesReason]]) {
        if (!reason || unavailable.has(element)) continue;
        unavailable.add(element);
        // Focusable and announced, not natively disabled: activating it executes nothing and says why in the status region.
        const reasonNode = doc.createElement("span");
        reasonNode.id = `${element.id}-reason`; reasonNode.hidden = true; reasonNode.textContent = reason;
        element.after(reasonNode);
        element.disabled = false;
        element.title = reason;
        element.setAttribute("aria-disabled", "true");
        element.setAttribute("aria-describedby", reasonNode.id);
        element.addEventListener("click", event => {
          event.stopImmediatePropagation(); event.preventDefault();
          status.textContent = reason; status.classList.remove("error");
        }, true);
      }
      ribbon.refresh();
    },
    setSymbolsEnabled(enabled) { symbolsCommand.disabled = !enabled; ribbon.refresh(); },
    setImageEnabled(enabled) { imageCommand.disabled = !enabled; ribbon.refresh(); },
    refresh: () => ribbon.refresh(),
    dispose() { imageCommand.removeEventListener("click",onImage); propertiesCommand.removeEventListener("click",onProperties); symbolsCommand.removeEventListener("click",onSymbols); aboutCommand.removeEventListener("click",onAbout); searchCommand.removeEventListener("click",onSearch); replaceCommand.removeEventListener("click",onReplace); statisticsCommand.removeEventListener("click",onStatistics); zoomCommand.removeEventListener("click", onZoom); help.dispose(); preferences.dispose(); choice.dispose(); ribbon.dispose(); theme.dispose(); toolbar.append(status); stateRow.remove(); },
  });
}
