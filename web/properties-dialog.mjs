// SPDX-License-Identifier: AGPL-3.0-or-later
// Document properties dialog (US-328). It knows nothing about the engine: the caller supplies
// `load()` (resolves to the snapshot of properties-protocol.mjs) and `apply(changes)`.
import { createDialog } from "./ui-components.mjs";

export const PROPERTIES_LABEL_KEYS = Object.freeze([
  "title", "close", "ok", "cancel", "loadError", "applyError", "documentHeading", "readOnlyHeading", "unavailable", "counter", "tooLong",
  "field-title", "field-subject", "field-author", "field-keywords", "field-comments", "field-category",
  "created", "modified", "lastPrinted", "lastModifiedBy", "revision", "application",
  "stat-pages", "stat-words", "stat-characters", "stat-charactersWithSpaces", "stat-lines", "stat-paragraphs",
]);
const FIELDS = Object.freeze(["title", "subject", "author", "keywords", "comments", "category"]);
const STATS = Object.freeze(["pages", "words", "characters", "charactersWithSpaces", "lines", "paragraphs"]);
const DATES = Object.freeze(["created", "modified", "lastPrinted"]);
const codePoints = text => { let n = 0; for (const _ of text) n += 1; return n; };
let serial = 0;

const make = (tag, className, text) => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = String(text);
  return node;
};

/**
 * @param {{ i18n: { t: Function, has: Function, formatNumber: Function, formatDate: Function, formatDateTime: Function },
 *   load: () => Promise<object>, apply: (changes: Record<string,string>) => Promise<{kind:"ok"}|{kind:"tooLong",field:string,max:number,length:number}> }} options
 * Messages are `properties-<key>` for every key of PROPERTIES_LABEL_KEYS.
 */
export function createPropertiesDialog({ i18n, load, apply } = {}) {
  if (!i18n || typeof load !== "function" || typeof apply !== "function") throw new TypeError("i18n, load and apply are required.");
  for (const key of PROPERTIES_LABEL_KEYS) if (!i18n.has(`properties-${key}`)) throw new TypeError(`Missing properties dialog message: ${key}.`);
  const text = (key, params) => i18n.t(`properties-${key}`, params);
  const id = `properties-${++serial}`;

  const form = make("form", "properties-dialog");
  form.noValidate = true;
  const status = make("p", "properties-status");
  status.setAttribute("role", "alert");
  status.hidden = true;
  const documentSection = make("section", "properties-section");
  const documentHeading = make("h3", "properties-heading", text("documentHeading"));
  documentSection.append(documentHeading);

  const controls = new Map();
  for (const name of FIELDS) {
    const multiline = name === "comments";
    const row = make("div", "properties-field");
    const label = make("label", "properties-label", text(`field-${name}`));
    const input = multiline ? make("textarea", "properties-input") : make("input", "properties-input");
    if (multiline) input.rows = 4; else input.type = "text";
    input.id = `${id}-${name}`; label.htmlFor = input.id;
    input.autocomplete = "off"; input.spellcheck = false;
    const counter = make("span", "properties-count"); counter.id = `${input.id}-count`;
    const error = make("span", "properties-error"); error.id = `${input.id}-error`; error.hidden = true; error.setAttribute("aria-live", "polite");
    input.setAttribute("aria-describedby", `${counter.id} ${error.id}`);
    row.append(label, input, counter, error);
    documentSection.append(row);
    controls.set(name, { input, counter, error, max: 0, original: "" });
  }

  const readOnlySection = make("section", "properties-section");
  const readOnlyHeading = make("h3", "properties-heading", text("readOnlyHeading"));
  const readOnly = make("dl", "properties-readonly");
  readOnly.setAttribute("aria-label", text("readOnlyHeading"));
  const values = new Map();
  const addReadOnly = (key, labelKey) => {
    const term = make("dt", "properties-term", text(labelKey));
    const detail = make("dd", "properties-value");
    readOnly.append(term, detail);
    values.set(key, detail);
  };
  for (const key of DATES) addReadOnly(key, key);
  addReadOnly("lastModifiedBy", "lastModifiedBy");
  addReadOnly("revision", "revision");
  addReadOnly("application", "application");
  for (const key of STATS) addReadOnly(`stat-${key}`, `stat-${key}`);
  readOnlySection.append(readOnlyHeading, readOnly);

  const actions = make("div", "properties-actions");
  const ok = make("button", "ui-button properties-ok", text("ok")); ok.type = "submit";
  const cancel = make("button", "ui-button properties-cancel", text("cancel")); cancel.type = "button";
  actions.append(ok, cancel);
  form.append(status, documentSection, readOnlySection, actions);

  const dialog = createDialog({ label: text("title"), closeLabel: text("close"), content: form, initialFocus: () => controls.get("title").input });
  dialog.element.classList.add("properties-dialog-host");

  let busy = false, loading = false, disposed = false;
  const unavailable = () => text("unavailable");
  const showStatus = text => { status.hidden = !text; status.textContent = text ?? ""; };
  const formatDate = date => {
    if (!date) return unavailable();
    const time = Date.parse(date.text);
    if (!date.valid || Number.isNaN(time)) return date.text;
    return /T/.test(date.text) ? i18n.formatDateTime(new Date(time)) : i18n.formatDate(new Date(time), { timeZone: "UTC" });
  };
  const fieldLabel = name => text(`field-${name}`);
  function refresh(name) {
    const control = controls.get(name);
    const length = codePoints(control.input.value);
    const over = length > control.max;
    control.counter.textContent = text("counter", { count: length, max: control.max });
    control.input.setAttribute("aria-invalid", String(over));
    control.error.hidden = !over;
    control.error.textContent = over ? text("tooLong", { field: fieldLabel(name), max: control.max, length }) : "";
    return over;
  }
  function refreshAll() {
    let invalid = false;
    for (const name of FIELDS) invalid = refresh(name) || invalid;
    ok.disabled = invalid || busy;
  }
  function populate(snapshot) {
    for (const name of FIELDS) {
      const control = controls.get(name);
      control.original = snapshot.editable[name] ?? "";
      control.max = snapshot.limits[name];
      control.input.value = control.original;
    }
    for (const key of DATES) values.get(key).textContent = formatDate(snapshot[key]);
    for (const key of ["lastModifiedBy", "revision", "application"]) values.get(key).textContent = snapshot[key] ?? unavailable();
    for (const key of STATS) values.get(`stat-${key}`).textContent = snapshot.statistics[key] === null || snapshot.statistics[key] === undefined ? unavailable() : i18n.formatNumber(snapshot.statistics[key]);
    showStatus("");
    refreshAll();
  }

  const onInput = () => { if (!busy) refreshAll(); };
  form.addEventListener("input", onInput);
  const onSubmit = async event => {
    event.preventDefault();
    if (busy || disposed || ok.disabled) return;
    const changes = {};
    for (const [name, control] of controls) if (control.input.value !== control.original) changes[name] = control.input.value;
    if (Object.keys(changes).length === 0) { dialog.close(); return; }
    busy = true; ok.disabled = true; showStatus("");
    try {
      const result = await apply(changes);
      if (disposed) return;
      if (result?.kind === "tooLong" && controls.has(result.field)) {
        const control = controls.get(result.field);
        control.max = result.max;
        busy = false; refreshAll();
        // The engine counts differently (for example after cleaning control characters): show its figures until the next edit.
        control.counter.textContent = text("counter", { count: result.length, max: result.max });
        control.input.setAttribute("aria-invalid", "true");
        control.error.hidden = false; control.error.textContent = text("tooLong", { field: fieldLabel(result.field), max: result.max, length: result.length });
        control.input.focus();
      } else if (result?.kind === "ok") { busy = false; dialog.close(); }
      else throw new Error("PROPERTIES_APPLY_FAILED");
    } catch {
      if (disposed) return;
      busy = false; refreshAll(); showStatus(text("applyError"));
    }
  };
  form.addEventListener("submit", onSubmit);
  const onCancel = () => { if (!busy) dialog.close(); };
  cancel.addEventListener("click", onCancel);
  const onKeydown = event => {
    if (event.key !== "Escape" || !dialog.element.open || event.isComposing) return;
    event.preventDefault();
    event.stopPropagation(); // the app's own Escape (close the DOCX preview) must not see this key
    onCancel();
  };
  dialog.element.addEventListener("keydown", onKeydown);
  const onNativeCancel = event => { if (busy) event.preventDefault(); };
  dialog.element.addEventListener("cancel", onNativeCancel);

  return Object.freeze({
    element: dialog.element,
    /** Loads the snapshot, then opens the modal dialog. Rejects (dialog stays closed) when the load fails; resolves false when already open or loading. */
    async open(options) {
      if (disposed || loading || dialog.element.open) return false;
      loading = true;
      try {
        const snapshot = await load();
        if (disposed) return false;
        populate(snapshot);
        dialog.open(options);
        return true;
      } finally { loading = false; }
    },
    close() { if (!disposed && !busy) dialog.close(); },
    get isOpen() { return dialog.element.open; },
    dispose() {
      if (disposed) return;
      disposed = true;
      form.removeEventListener("input", onInput); form.removeEventListener("submit", onSubmit);
      cancel.removeEventListener("click", onCancel);
      dialog.element.removeEventListener("keydown", onKeydown); dialog.element.removeEventListener("cancel", onNativeCancel);
      dialog.dispose();
    },
  });
}
