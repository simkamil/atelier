// SPDX-License-Identifier: AGPL-3.0-or-later

import { createDialog } from "./ui-components.mjs";

/**
 * Create the compact zoom controls for one document view.
 *
 * Required labels: group, range, percent, title, value, custom, width, page,
 * pages, apply, cancel, close, invalid. All user-facing strings are supplied
 * by the caller so this module does not depend on a particular locale.
 */
const MODES = Object.freeze(["custom", "width", "page", "pages"]);
let nextZoomControlId = 0;
const clampFactor = factor => Math.max(0.1, Math.min(5, factor));
const percentFor = factor => factor * 100;
const formatPercent = factor => {
  const percent = percentFor(factor);
  return Number.isInteger(percent) ? String(percent) : String(Number(percent.toFixed(2)));
};

const make = (document, tag, className, text) => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = String(text);
  return node;
};

export function createZoomControls({ document = globalThis.document, labels, onChange } = {}) {
  if (!document || !labels || typeof onChange !== "function") {
    throw new TypeError("A document, labels, and onChange callback are required.");
  }
  for (const key of ["group", "range", "percent", "title", "value", "custom", "width", "page", "pages", "apply", "cancel", "close", "invalid"]) {
    if (typeof labels[key] !== "string") throw new TypeError(`Missing zoom label: ${key}`);
  }

  const element = make(document, "div", "zoom-controls");
  element.setAttribute("role", "group");
  element.setAttribute("aria-label", labels.group);

  const range = make(document, "input", "zoom-controls-range");
  range.type = "range";
  range.min = "10";
  range.max = "500";
  range.step = "10";
  range.setAttribute("aria-label", labels.range);
  range.dataset.zoomRange = "";

  const percent = make(document, "button", "zoom-controls-percent");
  percent.type = "button";
  percent.setAttribute("aria-label", labels.percent);
  percent.setAttribute("aria-haspopup", "dialog");
  percent.dataset.zoomPercent = "";
  element.append(range, percent);

  const form = make(document, "form", "zoom-dialog-form");
  form.noValidate = true;
  const valueLabel = make(document, "label", "zoom-dialog-value-label", labels.value);
  const value = make(document, "input", "zoom-dialog-value");
  value.type = "number";
  value.min = "10";
  value.max = "500";
  value.step = "any";
  value.required = true;
  value.dataset.zoomValue = "";
  const valueId = `zoom-value-${++nextZoomControlId}`;
  const invalidId = `zoom-invalid-${nextZoomControlId}`;
  valueLabel.htmlFor = valueId;
  value.id = valueLabel.htmlFor;
  value.setAttribute("aria-describedby", invalidId);
  valueLabel.append(value);
  const invalid = make(document, "p", "zoom-dialog-error", labels.invalid);
  invalid.id = invalidId;
  invalid.hidden = true;
  invalid.setAttribute("role", "alert");

  const modes = make(document, "fieldset", "zoom-dialog-modes");
  const modeLegend = make(document, "legend", "zoom-dialog-legend", labels.group);
  modes.append(modeLegend);
  const radios = new Map();
  for (const mode of MODES) {
    const radioLabel = make(document, "label", "zoom-dialog-mode");
    const radio = make(document, "input");
    radio.type = "radio";
    radio.name = "zoom-mode";
    radio.value = mode;
    radioLabel.append(radio, document.createTextNode(labels[mode]));
    modes.append(radioLabel);
    radios.set(mode, radio);
  }

  const actions = make(document, "div", "zoom-dialog-actions");
  const cancel = make(document, "button", "zoom-dialog-cancel", labels.cancel);
  cancel.type = "button";
  const apply = make(document, "button", "zoom-dialog-apply", labels.apply);
  apply.type = "submit";
  apply.dataset.zoomApply = "";
  actions.append(cancel, apply);
  form.append(valueLabel, invalid, modes, actions);
  const dialogController = createDialog({ label: labels.title, content: form, closeLabel: labels.close, initialFocus: value });
  const dialog = dialogController.element;
  dialog.classList.add("zoom-dialog");
  dialog.dataset.zoomDialog = "";
  dialog.querySelector(".ui-dialog-title")?.classList.add("zoom-dialog-title");
  dialog.querySelector(".ui-dialog-close")?.classList.add("zoom-dialog-close");
  dialog.querySelector(".ui-dialog-content")?.classList.add("zoom-dialog-content");
  document.body.append(dialog);

  let zoom = 1;
  let mode = "custom";
  let disabled = false;
  let disposed = false;

  const render = () => {
    const percentValue = percentFor(zoom);
    range.value = String(percentValue);
    range.setAttribute("aria-valuetext", `${formatPercent(zoom)}%`);
    percent.textContent = `${formatPercent(zoom)}%`;
    const description = `${labels[mode]} — ${formatPercent(zoom)}%`;
    percent.title = description;
    percent.setAttribute("aria-label", description);
    percent.disabled = disabled;
    range.disabled = disabled;
    for (const radio of radios.values()) radio.disabled = disabled;
    value.disabled = disabled;
    apply.disabled = disabled;
  };
  const setInvalid = state => {
    invalid.hidden = !state;
    value.setAttribute("aria-invalid", String(state));
  };
  const closeDialog = ({ restoreFocus = true } = {}) => {
    if (dialog.open) dialogController.close();
    else if (restoreFocus) dialogController.close();
    setInvalid(false);
  };
  const open = ({ returnFocusTo = document.activeElement } = {}) => {
    if (disposed || disabled || dialog.open) return;
    value.value = formatPercent(zoom);
    radios.get(mode).checked = true;
    setInvalid(false);
    dialogController.open({ returnFocusTo });
    value.select();
  };
  const onRange = () => {
    if (disabled) return;
    zoom = clampFactor(Number(range.value) / 100);
    mode = "custom";
    render();
    onChange({ zoom, mode });
  };
  const onPercent = () => open({ returnFocusTo: percent });
  const onCancel = () => closeDialog();
  const onDialogCancel = event => { event.preventDefault(); closeDialog(); };
  const onSubmit = event => {
    event.preventDefault();
    if (disabled) return;
    const percentValue = Number(value.value);
    const selectedMode = [...radios.entries()].find(([, radio]) => radio.checked)?.[0];
    const validPercent = Number.isFinite(percentValue) && percentValue >= 10 && percentValue <= 500;
    const validCustom = selectedMode !== "custom" || Number.isInteger(percentValue);
    if (!validPercent || !validCustom || !selectedMode) {
      setInvalid(true);
      value.focus();
      return;
    }
    zoom = clampFactor(percentValue / 100);
    mode = selectedMode;
    render();
    closeDialog();
    onChange({ zoom, mode });
  };
  range.addEventListener("input", onRange);
  percent.addEventListener("click", onPercent);
  cancel.addEventListener("click", onCancel);
  dialog.addEventListener("cancel", onDialogCancel);
  form.addEventListener("submit", onSubmit);
  render();

  return Object.freeze({
    element,
    setState(state = {}) {
      if (disposed) return;
      if (Number.isFinite(state.zoom)) zoom = clampFactor(state.zoom);
      if (MODES.includes(state.mode)) mode = state.mode;
      disabled = Boolean(state.disabled);
      render();
    },
    open,
    dispose() {
      if (disposed) return;
      disposed = true;
      closeDialog();
      range.removeEventListener("input", onRange);
      percent.removeEventListener("click", onPercent);
      cancel.removeEventListener("click", onCancel);
      dialog.removeEventListener("cancel", onDialogCancel);
      form.removeEventListener("submit", onSubmit);
      element.remove();
      dialogController.dispose();
    },
  });
}
