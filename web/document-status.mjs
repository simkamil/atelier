// SPDX-License-Identifier: AGPL-3.0-or-later

import { createContextMenu, createDialog } from "./ui-components.mjs";

const FIELDS = Object.freeze(["page", "words", "language", "view", "zoom", "saved"]);
const OWN_FIELDS = new Set(["page", "language", "view", "saved"]);
const label = (labels, key) => typeof labels?.[key] === "string" ? labels[key] : "";
const hasLabels = (labels, ...keys) => keys.every(key => Boolean(label(labels, key)));
const format = (template, values) => template.replace(/\{(\w+)\}/g, (_, key) => String(values[key] ?? ""));
const validTotal = value => Number.isSafeInteger(value) && value > 0;
const validPage = (value, total) => Number.isSafeInteger(value) && value >= 1 && value <= total;

export function createDocumentStatus({ document: doc = globalThis.document, labels = {}, menuTarget, onGoToPage, onVisibilityChange } = {}) {
  if (!doc) throw new TypeError("A document is required.");

  const root = doc.createElement("div");
  root.className = "document-status";
  root.dataset.documentStatus = "";
  root.tabIndex = 0;
  root.setAttribute("role", "group");
  const configureLabel = label(labels, "statusConfigure");
  if (configureLabel) root.setAttribute("aria-label", configureLabel);
  const row = doc.createElement("div");
  row.className = "document-status-fields";
  row.setAttribute("role", "status");
  row.setAttribute("aria-live", "polite");
  root.append(row);

  const visibility = Object.fromEntries(FIELDS.map(field => [field, true]));
  const state = { page: null, totalPages: null, language: null, mode: "text", dirty: null, busy: false };
  const nodes = new Map();
  let disposed = false;

  const makeField = (field, name, valueTag = "span") => {
    const text = label(labels, name);
    if (!text) return null;
    const wrapper = doc.createElement("span");
    wrapper.className = "document-status-field";
    wrapper.dataset.statusField = field;
    const value = doc.createElement(valueTag);
    value.className = "document-status-value";
    wrapper.append(value);
    row.append(wrapper);
    nodes.set(field, { wrapper, value });
    return value;
  };

  const pageValue = hasLabels(labels, "statusPageCount", "statusPageUnavailable")
    ? makeField("page", "statusPageCount", "button") : null;
  if (pageValue) {
    pageValue.type = "button";
    pageValue.className = "document-status-value document-status-page";
    pageValue.setAttribute("aria-haspopup", "dialog");
    pageValue.setAttribute("aria-expanded", "false");
    pageValue.addEventListener("click", openGoto);
  }
  if (hasLabels(labels, "statusLanguage", "statusLanguageUnknown")) makeField("language", "statusLanguage");
  if (hasLabels(labels, "statusViewText", "statusViewPreview")) makeField("view", "statusViewText");
  if (hasLabels(labels, "statusSaved", "statusUnsaved", "statusReadOnly")) makeField("saved", "statusSaved");

  const dialogContent = doc.createElement("form");
  dialogContent.className = "document-status-goto-form";
  dialogContent.addEventListener("submit", event => { event.preventDefault(); submitGoto(); });
  const inputLabel = doc.createElement("label");
  inputLabel.textContent = label(labels, "statusGotoPage");
  const input = doc.createElement("input");
  input.type = "number";
  input.inputMode = "numeric";
  input.step = "any";
  input.dataset.gotoPage = "";
  inputLabel.append(input);
  const error = doc.createElement("p");
  error.className = "document-status-goto-error";
  error.setAttribute("role", "status");
  error.setAttribute("aria-live", "polite");
  const actions = doc.createElement("div");
  actions.className = "document-status-goto-actions";
  const apply = doc.createElement("button");
  apply.type = "submit";
  apply.textContent = label(labels, "statusGotoApply");
  apply.dataset.gotoApply = "";
  const cancel = doc.createElement("button");
  cancel.type = "button";
  cancel.textContent = label(labels, "statusGotoCancel");
  cancel.dataset.gotoCancel = "";
  actions.append(apply, cancel);
  dialogContent.append(inputLabel, error, actions);
  let dialog = null;
  let closeDialog = null;
  if (hasLabels(labels, "statusGotoTitle", "statusGotoClose", "statusGotoPage", "statusGotoApply", "statusGotoCancel", "statusGotoInvalid", "statusGotoRange")) {
    dialog = createDialog({
      label: label(labels, "statusGotoTitle"),
      closeLabel: label(labels, "statusGotoClose"),
      content: dialogContent,
      initialFocus: () => input,
    });
    dialog.element.classList.add("document-status-goto-dialog");
    dialog.element.dataset.statusGotoDialog = "";
    root.append(dialog.element);
    dialog.element.addEventListener("close", closeGotoState);
    dialog.element.addEventListener("cancel", closeGotoState);
    closeDialog = () => dialog.close();
    cancel.addEventListener("click", closeDialog);
    input.addEventListener("input", clearError);
  }

  function closeGotoState() {
    pageValue?.setAttribute("aria-expanded", "false");
  }

  function openGoto() {
    if (disposed || !dialog || !pageValue || pageValue.disabled) return false;
    clearError();
    input.value = String(validPage(state.page, state.totalPages) ? state.page : 1);
    dialog.open({ returnFocusTo: pageValue });
    pageValue.setAttribute("aria-expanded", "true");
    return true;
  }

  function clearError() {
    error.textContent = "";
    input.removeAttribute("aria-invalid");
  }

  function submitGoto() {
    if (disposed || state.busy || !validTotal(state.totalPages)) return;
    const raw = input.value.trim();
    const parsed = raw === "" ? NaN : Number(raw);
    if (!Number.isSafeInteger(parsed)) {
      error.textContent = label(labels, "statusGotoInvalid");
      input.setAttribute("aria-invalid", "true");
      return;
    }
    if (!validPage(parsed, state.totalPages)) {
      error.textContent = format(label(labels, "statusGotoRange"), { total: state.totalPages });
      input.setAttribute("aria-invalid", "true");
      return;
    }
    clearError();
    onGoToPage?.(parsed);
    dialog.close();
  }

  const menuItems = FIELDS.filter(field => label(labels, `statusShow${field[0].toUpperCase()}${field.slice(1)}`)
    && (!OWN_FIELDS.has(field) || nodes.has(field))).map(field => ({
    label: label(labels, `statusShow${field[0].toUpperCase()}${field.slice(1)}`),
    value: field,
  }));
  const contextTarget = menuTarget ?? root;
  let menu = null;
  if (configureLabel && menuItems.length) {
    menu = createContextMenu({
      label: configureLabel,
      target: contextTarget,
      items: menuItems,
      onSelect(field) {
        if (disposed || !FIELDS.includes(field)) return;
        visibility[field] = !visibility[field];
        render();
        onVisibilityChange?.({ field, visible: visibility[field] });
      },
    });
    menu.popup.dataset.statusVisibilityMenu = "";
    menu.popup.setAttribute("aria-label", configureLabel);
    root.append(menu.element);
    menu.popup.querySelectorAll("[role=menuitem]").forEach((item, index) => {
      item.setAttribute("role", "menuitemcheckbox");
      item.dataset.visibilityField = menuItems[index].value;
    });
  }

  function render() {
    const total = validTotal(state.totalPages) ? state.totalPages : null;
    const pageIsValid = total !== null && validPage(state.page, total);
    if (pageValue) {
      const text = pageIsValid
        ? format(label(labels, "statusPageCount"), { page: state.page, total })
        : label(labels, "statusPageUnavailable");
      if (pageValue.textContent !== text) pageValue.textContent = text;
      pageValue.setAttribute("aria-label", text);
      pageValue.disabled = !pageIsValid || state.busy || !dialog;
    }

    const languageNode = nodes.get("language")?.value;
    if (languageNode) {
      const text = typeof state.language === "string" && state.language.length
        ? format(label(labels, "statusLanguage"), { language: state.language })
        : label(labels, "statusLanguageUnknown");
      if (languageNode.textContent !== text) languageNode.textContent = text;
    }

    const viewNode = nodes.get("view")?.value;
    if (viewNode) {
      const text = state.mode === "preview" ? label(labels, "statusViewPreview") : label(labels, "statusViewText");
      if (viewNode.textContent !== text) viewNode.textContent = text;
    }

    const savedNode = nodes.get("saved")?.value;
    if (savedNode) {
      const text = state.mode === "preview" ? label(labels, "statusReadOnly")
        : state.dirty === true ? label(labels, "statusUnsaved")
          : state.dirty === false ? label(labels, "statusSaved") : "";
      if (savedNode.textContent !== text) savedNode.textContent = text;
      nodes.get("saved").wrapper.hidden = !text;
    }

    for (const field of OWN_FIELDS) {
      const item = nodes.get(field);
      if (item) item.wrapper.hidden = !visibility[field];
    }
    if (menu) {
      menu.popup.querySelectorAll("[data-visibility-field]").forEach(item => {
        item.setAttribute("aria-checked", String(visibility[item.dataset.visibilityField]));
      });
    }
    if (dialog) {
      apply.disabled = state.busy || !validTotal(state.totalPages);
      input.disabled = state.busy || !validTotal(state.totalPages);
    }
  }

  render();

  return Object.freeze({
    element: root,
    setState(next = {}) {
      if (disposed || !next || typeof next !== "object") return;
      for (const key of ["page", "totalPages", "language", "mode", "dirty", "busy"]) {
        if (Object.hasOwn(next, key)) state[key] = next[key];
      }
      state.mode = state.mode === "preview" ? "preview" : "text";
      state.busy = Boolean(state.busy);
      render();
    },
    getVisibility() { return { ...visibility }; },
    dispose() {
      if (disposed) return;
      disposed = true;
      pageValue?.removeEventListener("click", openGoto);
      if (dialog) {
        dialog.element.removeEventListener("close", closeGotoState);
        dialog.element.removeEventListener("cancel", closeGotoState);
        cancel.removeEventListener("click", closeDialog);
        input.removeEventListener("input", clearError);
        dialog.dispose();
      }
      menu?.dispose();
      root.remove();
    },
  });
}
