// SPDX-License-Identifier: AGPL-3.0-or-later

import { createDialog } from "./ui-components.mjs";

const FIELDS = Object.freeze([
  ["pages", "statisticsFieldPages"],
  ["words", "statisticsFieldWords"],
  ["characters", "statisticsFieldCharacters"],
  ["charactersWithSpaces", "statisticsFieldCharactersWithSpaces"],
  ["paragraphs", "statisticsFieldParagraphs"],
  ["lines", "statisticsFieldLines"],
]);
let nextStatisticsId = 0;

const label = (labels, key) => String(labels?.[key] ?? key);
const updateText = (node, value) => { if (node.textContent !== value) node.textContent = value; };
const validCount = value => Number.isSafeInteger(value) && value >= 0;

export function createStatisticsControls({ document: doc = globalThis.document, labels, i18n, onOpen } = {}) {
  if (!doc || !labels || typeof i18n?.t !== "function") throw new TypeError("A document, localized labels and an i18n translator are required.");
  // Numbers use the translator's locale and the word count agrees in number ("1 mot").
  const number = value => validCount(value) ? i18n.formatNumber(value) : "—";
  const wordsText = (selectedValue, wordsValue, hasSelection) => {
    const key = hasSelection ? "statusSelection" : "statusDocument";
    return i18n.t(key, hasSelection ? { selected: selectedValue, words: wordsValue } : { words: wordsValue });
  };

  const root = doc.createElement("div");
  root.className = "statistics-controls";
  const status = doc.createElement("button");
  status.type = "button";
  status.className = "statistics-status";
  status.dataset.statisticsStatus = "";
  status.setAttribute("aria-haspopup", "dialog");
  status.setAttribute("aria-expanded", "false");
  const live = doc.createElement("span");
  live.className = "statistics-live";
  live.setAttribute("aria-live", "polite");
  live.setAttribute("role", "status");

  const table = doc.createElement("table");
  table.className = "statistics-table";
  const thead = doc.createElement("thead");
  const header = doc.createElement("tr");
  const metricHeading = doc.createElement("th");
  metricHeading.scope = "col";
  metricHeading.textContent = label(labels, "statisticsMetric");
  header.append(metricHeading);
  const columnHeading = key => {
    const th = doc.createElement("th");
    th.scope = "col";
    th.textContent = label(labels, key);
    return th;
  };
  header.append(columnHeading("statisticsScopeDocument"), columnHeading("statisticsScopeSelection"));
  thead.append(header);
  const tbody = doc.createElement("tbody");
  const valueNodes = new Map();
  for (const [field, fieldLabel] of FIELDS) {
    const row = doc.createElement("tr");
    const th = doc.createElement("th");
    th.scope = "row";
    th.textContent = label(labels, fieldLabel);
    row.append(th);
    for (const scope of ["document", "selection"]) {
      const cell = doc.createElement("td");
      cell.dataset.statisticsCount = `${scope}:${field}`;
      cell.textContent = "—";
      row.append(cell);
      valueNodes.set(`${scope}:${field}`, cell);
    }
    tbody.append(row);
  }
  table.append(thead, tbody);

  const notesRow = doc.createElement("div");
  notesRow.className = "statistics-unavailable";
  const notesOption = doc.createElement("input");
  notesOption.type = "checkbox";
  notesOption.disabled = true;
  const notesLabel = doc.createElement("label");
  notesLabel.append(notesOption, doc.createTextNode(` ${label(labels, "statisticsNotesTextBoxes")}`));
  const notesExplanation = doc.createElement("span");
  notesExplanation.className = "statistics-unavailable-reason";
  notesExplanation.textContent = label(labels, "statisticsNotesUnavailable");
  notesExplanation.id = `statistics-notes-unavailable-${++nextStatisticsId}`;
  notesOption.setAttribute("aria-describedby", notesExplanation.id);
  notesRow.append(notesLabel, notesExplanation);

  const content = doc.createElement("div");
  content.className = "statistics-dialog-body";
  const dialogStatus = doc.createElement("p");
  dialogStatus.className = "statistics-dialog-status";
  dialogStatus.id = `statistics-dialog-status-${++nextStatisticsId}`;
  dialogStatus.setAttribute("role", "status");
  dialogStatus.setAttribute("aria-live", "polite");
  table.setAttribute("aria-describedby", dialogStatus.id);
  content.append(dialogStatus, table, notesRow);
  const dialog = createDialog({
    label: label(labels, "statisticsDialogTitle"),
    closeLabel: label(labels, "statisticsClose"),
    content,
    initialFocus: dialogNode => dialogNode.querySelector(".ui-dialog-close"),
  });
  dialog.element.classList.add("statistics-dialog");
  dialog.element.dataset.statisticsDialog = "";
  root.append(status, live, dialog.element);

  let state = { document: null, selection: null, hasSelection: false, busy: true, disabled: false, error: null };
  let disposed = false;

  const render = () => {
    const { document: docCounts, selection, hasSelection, busy, disabled, error } = state;
    const missing = !docCounts || (hasSelection && !selection) || !validCount(docCounts.words) || (hasSelection && !validCount(selection.words));
    if (busy) {
      status.textContent = label(labels, "statisticsLoading");
      updateText(live, label(labels, "statisticsLoading"));
    } else if (error) {
      status.textContent = label(labels, "statisticsError");
      updateText(live, label(labels, "statisticsError"));
    } else if (missing) {
      status.textContent = label(labels, "statisticsUnavailableData");
      updateText(live, label(labels, "statisticsUnavailableData"));
    } else if (hasSelection) {
      status.textContent = wordsText(selection.words, docCounts.words, true);
      updateText(live, status.textContent);
    } else {
      status.textContent = wordsText(null, docCounts.words, false);
      updateText(live, status.textContent);
    }
    status.setAttribute("aria-label", busy ? label(labels, "statisticsLoading") : error
      ? label(labels, "statisticsError")
      : missing
      ? label(labels, "statisticsUnavailableData")
      : hasSelection
        ? wordsText(selection.words, docCounts.words, true)
        : wordsText(null, docCounts.words, false));
    status.disabled = Boolean(disabled);
    status.setAttribute("aria-disabled", String(status.disabled));

    for (const [field] of FIELDS) {
      valueNodes.get(`document:${field}`).textContent = number(docCounts?.[field]);
      valueNodes.get(`selection:${field}`).textContent = hasSelection ? number(selection?.[field]) : "—";
    }
    if (busy) updateText(dialogStatus, label(labels, "statisticsLoading"));
    else if (error) updateText(dialogStatus, label(labels, "statisticsError"));
    else if (missing) updateText(dialogStatus, label(labels, "statisticsUnavailableData"));
    else updateText(dialogStatus, "");
    dialogStatus.classList.toggle("is-error", Boolean(error));
  };

  const open = ({ returnFocusTo = status } = {}) => {
    if (disposed || status.disabled) return false;
    if (!dialog.element.open) {
      dialog.open({ returnFocusTo });
      status.setAttribute("aria-expanded", "true");
      onOpen?.();
    }
    return true;
  };
  const closeState = () => status.setAttribute("aria-expanded", "false");
  const onDialogClose = () => closeState();
  dialog.element.addEventListener("close", onDialogClose);
  const onActivate = () => open();
  const onCancel = () => closeState();
  status.addEventListener("click", onActivate);
  dialog.element.addEventListener("cancel", onCancel);
  render();

  return Object.freeze({
    element: root,
    setState(next = {}) {
      if (disposed) return;
      state = { ...state, ...next };
      render();
    },
    open,
    dispose() {
      if (disposed) return;
      disposed = true;
      status.removeEventListener("click", onActivate);
      dialog.element.removeEventListener("close", onDialogClose);
      dialog.element.removeEventListener("cancel", onCancel);
      dialog.dispose();
      root.remove();
    },
  });
}
