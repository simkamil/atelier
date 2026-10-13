// SPDX-License-Identifier: AGPL-3.0-or-later

/**
 * Keyboard contract: popup menus/listboxes use Up/Down, Home/End, Enter/Space
 * and Escape; Escape and selection restore the opener. Font combos keep the
 * input focused, expose active options through aria-activedescendant, and defer
 * filtering during IME composition. Color swatches use arrows/Home/End. Modal
 * dialogs trap Tab and restore their opener. Tabs use one roving tab stop,
 * orientation-aware arrows, Home/End and automatic activation. Tooltips show
 * on focus/hover and Escape dismisses them. Labels and option text are injected.
 */

let nextId = 0;
const idFor = prefix => `${prefix}-${++nextId}`;
const element = (name, className, text) => {
  const node = document.createElement(name);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = String(text);
  return node;
};
const setDisabled = (node, disabled) => {
  node.disabled = Boolean(disabled);
  node.setAttribute("aria-disabled", String(Boolean(disabled)));
};
const controller = (root, cleanup, extra = {}, { removeElement = true } = {}) => Object.freeze({
  element: root,
  dispose() { cleanup(); if (removeElement) root.remove(); },
  ...extra,
});
const listen = (node, type, listener, options) => node.addEventListener(type, listener, options);

function makeMenu(items, { label, onSelect, role = "menu", optionRole = "menuitem", onClose, inside } = {}) {
  const popup = element("div", "ui-popup ui-menu");
  popup.id = idFor(role);
  popup.setAttribute("role", role);
  popup.setAttribute("aria-label", label);
  popup.hidden = true;
  const entries = items.map((item, index) => {
    const button = element("button", "ui-menu-item", item.label);
    button.type = "button";
    button.setAttribute("role", optionRole);
    if (item.disabled) button.setAttribute("aria-disabled", "true");
    button.tabIndex = -1;
    popup.append(button);
    return { item, button, index };
  });
  let returnFocus = null;
  const enabled = () => entries.filter(entry => !entry.item.disabled);
  const focusAt = index => {
    const available = enabled();
    if (!available.length) return;
    const entry = available[(index + available.length) % available.length];
    entries.forEach(value => { value.button.tabIndex = value === entry ? 0 : -1; });
    entry.button.focus();
  };
  const close = ({ restoreFocus = true } = {}) => {
    if (popup.hidden) return;
    popup.hidden = true;
    onClose?.();
    if (restoreFocus) returnFocus?.focus();
  };
  const open = ({ focusIndex = 0, focus = true, returnFocusTo = document.activeElement } = {}) => {
    returnFocus = returnFocusTo;
    popup.hidden = false;
    if (focus) focusAt(focusIndex);
  };
  const keydown = event => {
    const available = enabled();
    const current = available.findIndex(entry => entry.button === document.activeElement);
    if (event.key === "Escape") { event.preventDefault(); close(); return; }
    if ((event.key === "Enter" || event.key === " " || event.key === "Spacebar") && current >= 0) {
      event.preventDefault(); available[current].button.click(); return;
    }
    if (event.key === "ArrowDown") { event.preventDefault(); focusAt(current + 1); }
    else if (event.key === "ArrowUp") { event.preventDefault(); focusAt(current < 0 ? available.length - 1 : current - 1); }
    else if (event.key === "Home") { event.preventDefault(); focusAt(0); }
    else if (event.key === "End") { event.preventDefault(); focusAt(available.length - 1); }
    else if (event.key === "Tab") close({ restoreFocus: false });
  };
  listen(popup, "keydown", keydown);
  entries.forEach(({ item, button }) => listen(button, "click", () => {
    if (item.disabled) return;
    onSelect?.(item.value, item);
    item.onSelect?.(item.value, item);
    close();
  }));
  const outside = event => { if (!popup.hidden && !popup.contains(event.target) && !inside?.contains(event.target)) close({ restoreFocus: false }); };
  listen(document, "pointerdown", outside);
  return {
    popup, entries, open, close,
    dispose() { close({ restoreFocus: false }); popup.removeEventListener("keydown", keydown); document.removeEventListener("pointerdown", outside); popup.remove(); },
  };
}

export function createButton({ label, onActivate, disabled = false, className = "" }) {
  const button = element("button", `ui-button ${className}`.trim(), label);
  button.type = "button";
  button.setAttribute("aria-label", label);
  setDisabled(button, disabled);
  const activate = event => { if (!button.disabled) onActivate?.(event); };
  listen(button, "click", activate);
  return controller(button, () => button.removeEventListener("click", activate), { button });
}

export function createToggleButton({ label, pressed = false, disabled = false, onChange }) {
  const button = element("button", "ui-button ui-toggle", label);
  button.type = "button";
  button.setAttribute("aria-label", label);
  button.setAttribute("aria-pressed", String(Boolean(pressed)));
  setDisabled(button, disabled);
  const change = () => { if (!button.disabled) setPressed(!isPressed()); };
  const isPressed = () => button.getAttribute("aria-pressed") === "true";
  const setPressed = value => {
    button.setAttribute("aria-pressed", String(Boolean(value)));
    onChange?.(Boolean(value));
  };
  listen(button, "click", change);
  return controller(button, () => button.removeEventListener("click", change), { button, isPressed, setPressed });
}

export function createSplitButton({ label, onActivate, items = [], menuLabel = label, disabled = false, onSelect }) {
  const root = element("div", "ui-split-button");
  const action = element("button", "ui-button ui-split-action", label);
  action.type = "button"; action.setAttribute("aria-label", label); setDisabled(action, disabled);
  const disclosure = element("button", "ui-button ui-split-disclosure", "▾");
  disclosure.type = "button"; disclosure.setAttribute("aria-label", menuLabel); disclosure.setAttribute("aria-haspopup", "menu");
  disclosure.setAttribute("aria-expanded", "false"); setDisabled(disclosure, disabled);
  const menu = makeMenu(items, { label: menuLabel, onSelect, inside: root, onClose: () => disclosure.setAttribute("aria-expanded", "false") });
  disclosure.setAttribute("aria-controls", menu.popup.id);
  root.append(action, disclosure, menu.popup);
  const update = () => disclosure.setAttribute("aria-expanded", String(!menu.popup.hidden));
  const open = () => { if (disclosure.disabled) return; menu.open({ returnFocusTo: disclosure }); update(); };
  const close = ({ restoreFocus = true } = {}) => { menu.close({ restoreFocus }); update(); };
  const onAction = event => { if (!action.disabled) onActivate?.(event); };
  const onDisclosure = () => menu.popup.hidden ? open() : close({ restoreFocus: false });
  const onOutside = () => update();
  listen(action, "click", onAction); listen(disclosure, "click", onDisclosure);
  listen(menu.popup, "keydown", onOutside);
  const outsideObserver = new MutationObserver(update); outsideObserver.observe(menu.popup, { attributes: true, attributeFilter: ["hidden"] });
  return controller(root, () => { action.removeEventListener("click", onAction); disclosure.removeEventListener("click", onDisclosure); outsideObserver.disconnect(); menu.dispose(); }, { action, disclosure, open, close });
}

export function createDropdown({ label, options = [], value, onChange, disabled = false }) {
  const root = element("div", "ui-dropdown");
  const trigger = element("button", "ui-button ui-popup-trigger", "");
  trigger.type = "button"; trigger.setAttribute("aria-label", label); trigger.setAttribute("aria-haspopup", "listbox"); trigger.setAttribute("aria-expanded", "false");
  setDisabled(trigger, disabled);
  const menuOptions = options.map(option => ({ label: option.label, value: option.value, disabled: option.disabled }));
  const popup = makeMenu(menuOptions, { label, role: "listbox", optionRole: "option", inside: root, onSelect: selected => setValue(selected), onClose: () => trigger.setAttribute("aria-expanded", "false") });
  trigger.setAttribute("aria-controls", popup.popup.id);
  root.append(trigger, popup.popup);
  const selected = () => options.find(option => option.value === value);
  const render = () => {
    const option = selected();
    trigger.textContent = option?.label ?? label;
    popup.entries.forEach(({ item, button }) => button.setAttribute("aria-selected", String(item.value === value)));
  };
  const setValue = next => {
    if (!options.some(option => option.value === next && !option.disabled)) return;
    value = next; render(); onChange?.(value);
  };
  const open = () => {
    if (trigger.disabled) return;
    const enabled = options.filter(option => !option.disabled);
    const selectedIndex = enabled.findIndex(option => option.value === value);
    popup.open({ focusIndex: selectedIndex >= 0 ? selectedIndex : 0, returnFocusTo: trigger });
    trigger.setAttribute("aria-expanded", "true");
  };
  const close = ({ restoreFocus = true } = {}) => { popup.close({ restoreFocus }); trigger.setAttribute("aria-expanded", "false"); };
  const click = () => popup.popup.hidden ? open() : close({ restoreFocus: false });
  listen(trigger, "click", click); render();
  return controller(root, () => { trigger.removeEventListener("click", click); popup.dispose(); }, { trigger, setValue, getValue: () => value, open, close });
}

export function createFontCombo({ label, options = [], value = "", onChange, disabled = false }) {
  const root = element("div", "ui-font-combo");
  const input = element("input", "ui-font-input");
  input.type = "text"; input.setAttribute("role", "combobox"); input.setAttribute("aria-label", label);
  input.setAttribute("aria-autocomplete", "list"); input.setAttribute("aria-haspopup", "listbox"); input.setAttribute("aria-expanded", "false");
  input.disabled = Boolean(disabled);
  const popup = element("div", "ui-popup ui-listbox"); popup.setAttribute("role", "listbox"); popup.setAttribute("aria-label", label); popup.id = idFor("font-list"); popup.hidden = true;
  input.setAttribute("aria-controls", popup.id);
  let active = -1;
  let composing = false;
  let visibleOptions = [];
  let closeAfterComposition = false;
  let disposed = false;
  const filtered = showAll => showAll ? options : options.filter(option => option.label.toLocaleLowerCase().includes(input.value.toLocaleLowerCase()));
  const render = (showAll = false) => {
    input.removeAttribute("aria-activedescendant");
    popup.replaceChildren(); active = -1; visibleOptions = filtered(showAll);
    for (const option of visibleOptions) {
      const item = element("div", "ui-listbox-option", option.label);
      item.setAttribute("role", "option"); item.id = idFor("font-option"); item.style.fontFamily = option.value;
      item.dataset.value = option.value;
      listen(item, "mousedown", event => event.preventDefault());
      listen(item, "click", () => choose(option));
      popup.append(item);
    }
    popup.hidden = popup.childElementCount === 0;
    input.setAttribute("aria-expanded", String(!popup.hidden));
  };
  const close = ({ restoreValue = false } = {}) => {
    popup.hidden = true; input.setAttribute("aria-expanded", "false"); input.removeAttribute("aria-activedescendant"); active = -1; visibleOptions = [];
    if (restoreValue) input.value = options.find(option => option.value === value)?.label ?? value;
  };
  const choose = option => { if (!option) return; value = option.value; input.value = option.label; close(); onChange?.(value, option); };
  const setActive = index => {
    const entries = [...popup.children]; if (!entries.length) return;
    active = (index + entries.length) % entries.length;
    entries.forEach((entry, i) => entry.setAttribute("aria-selected", String(i === active)));
    input.setAttribute("aria-activedescendant", entries[active].id); entries[active].scrollIntoView?.({ block: "nearest" });
  };
  const onInput = () => { if (!composing) render(); };
  const onCompositionStart = () => { composing = true; };
  const onCompositionEnd = () => {
    composing = false; render();
    if (closeAfterComposition) { closeAfterComposition = false; close(); }
  };
  const onFocusOut = () => queueMicrotask(() => {
    if (disposed || root.contains(document.activeElement)) return;
    if (composing) closeAfterComposition = true; else close();
  });
  const onOutside = event => {
    if (root.contains(event.target)) return;
    if (composing) closeAfterComposition = true; else close();
  };
  const onKey = event => {
    if (event.isComposing || composing) return;
    if (event.key === "ArrowDown") { event.preventDefault(); if (popup.hidden) render(true); setActive(active + 1); }
    else if (event.key === "ArrowUp") { event.preventDefault(); if (popup.hidden) render(true); setActive(active < 0 ? popup.childElementCount - 1 : active - 1); }
    else if (event.key === "Enter" && !popup.hidden && active >= 0) { event.preventDefault(); choose(visibleOptions[active]); }
    else if (event.key === "Escape" && !popup.hidden) { event.preventDefault(); close({ restoreValue: true }); }
    else if (event.key === "Home" && !popup.hidden) { event.preventDefault(); setActive(0); }
    else if (event.key === "End" && !popup.hidden) { event.preventDefault(); setActive(popup.childElementCount - 1); }
  };
  listen(root, "focusout", onFocusOut); listen(document, "pointerdown", onOutside);
  listen(input, "input", onInput); listen(input, "compositionstart", onCompositionStart); listen(input, "compositionend", onCompositionEnd); listen(input, "keydown", onKey);
  input.value = options.find(option => option.value === value)?.label ?? value;
  root.append(input, popup);
  return controller(root, () => { disposed = true; close(); root.removeEventListener("focusout", onFocusOut); document.removeEventListener("pointerdown", onOutside); input.removeEventListener("input", onInput); input.removeEventListener("compositionstart", onCompositionStart); input.removeEventListener("compositionend", onCompositionEnd); input.removeEventListener("keydown", onKey); }, { input, setValue: next => { const option = options.find(item => item.value === next); if (option) choose(option); }, getValue: () => value });
}

export function createColorPicker({ label, themeColors = [], standardColors = [], value = "#000000", customLabel = label, onChange, disabled = false }) {
  const root = element("div", "ui-color-picker");
  const trigger = element("button", "ui-button ui-color-trigger", label); trigger.type = "button";
  trigger.setAttribute("aria-label", label); trigger.setAttribute("aria-haspopup", "dialog"); trigger.setAttribute("aria-expanded", "false");
  setDisabled(trigger, disabled);
  const popup = element("div", "ui-popup ui-color-popup"); popup.setAttribute("role", "dialog"); popup.setAttribute("aria-label", label); popup.id = idFor("color-dialog"); popup.hidden = true;
  trigger.setAttribute("aria-controls", popup.id);
  const swatches = [];
  const paletteValues = new Set([...themeColors.colors ?? [], ...standardColors.colors ?? []].map(color => String(color.value).toLowerCase()));
  const validColor = next => typeof next === "string" && (paletteValues.has(next.toLowerCase()) || /^#[0-9a-f]{6}$/i.test(next));
  const addGroup = (groupLabel, colors) => {
    if (!colors.length) return;
    const group = element("div", "ui-color-group"); group.setAttribute("role", "group"); if (groupLabel) group.setAttribute("aria-label", groupLabel);
    for (const color of colors) {
      const button = element("button", "ui-color-swatch"); button.type = "button"; button.setAttribute("aria-label", color.label); button.title = color.label;
      button.dataset.value = color.value; button.style.setProperty("--ui-swatch-color", color.value); button.setAttribute("aria-pressed", String(color.value.toLowerCase() === value.toLowerCase()));
      listen(button, "click", () => choose(color.value)); group.append(button); swatches.push(button);
    }
    popup.append(group);
  };
  addGroup(themeColors.label, themeColors.colors ?? []);
  addGroup(standardColors.label, standardColors.colors ?? []);
  const custom = element("label", "ui-color-custom", customLabel);
  const input = element("input"); input.type = "color"; input.setAttribute("aria-label", customLabel); input.value = /^#[0-9a-f]{6}$/i.test(value) ? value : "#000000";
  listen(input, "input", () => setValue(input.value)); custom.append(input); popup.append(custom); root.append(trigger, popup);
  const choose = next => {
    if (!validColor(next)) return false;
    value = next;
    input.value = /^#[0-9a-f]{6}$/i.test(value) ? value : "#000000";
    swatches.forEach(button => button.setAttribute("aria-pressed", String(button.dataset.value.toLowerCase() === value.toLowerCase())));
    onChange?.(value);
    return true;
  };
  const setValue = next => choose(next);
  const close = ({ restoreFocus = true } = {}) => { popup.hidden = true; trigger.setAttribute("aria-expanded", "false"); if (restoreFocus) trigger.focus(); };
  const open = () => { if (trigger.disabled) return; popup.hidden = false; trigger.setAttribute("aria-expanded", "true"); input.focus(); };
  const click = () => popup.hidden ? open() : close({ restoreFocus: false });
  const keydown = event => {
    if (event.key === "Escape") { event.preventDefault(); close(); return; }
    const current = swatches.indexOf(event.target);
    if (current < 0 || !swatches.length) return;
    let target = current;
    if (event.key === "ArrowRight" || event.key === "ArrowDown") target = (current + 1) % swatches.length;
    else if (event.key === "ArrowLeft" || event.key === "ArrowUp") target = (current + swatches.length - 1) % swatches.length;
    else if (event.key === "Home") target = 0;
    else if (event.key === "End") target = swatches.length - 1;
    else return;
    event.preventDefault(); swatches[target].focus();
  };
  const outside = event => { if (!popup.hidden && !root.contains(event.target)) close({ restoreFocus: false }); };
  listen(trigger, "click", click); listen(popup, "keydown", keydown);
  listen(document, "pointerdown", outside);
  return controller(root, () => { trigger.removeEventListener("click", click); popup.removeEventListener("keydown", keydown); document.removeEventListener("pointerdown", outside); }, { trigger, popup, open, close, setValue, getValue: () => value });
}

export function createNumberUnitField({ label, value = 0, unit, units = [unit], min = -Infinity, max = Infinity, step = 1, disabled = false, onChange }) {
  const root = element("div", "ui-number-unit");
  const input = element("input", "ui-number-input"); input.type = "number"; input.setAttribute("aria-label", label); input.step = String(step);
  input.disabled = Boolean(disabled);
  if (Number.isFinite(min)) input.min = String(min); if (Number.isFinite(max)) input.max = String(max); input.defaultValue = String(value); input.value = String(value);
  const select = element("select", "ui-unit-select"); select.setAttribute("aria-label", label);
  select.disabled = Boolean(disabled);
  for (const entry of units) { const option = element("option", "", typeof entry === "string" ? entry : entry.label); option.value = typeof entry === "string" ? entry : entry.value; select.append(option); }
  select.value = unit ?? select.options[0]?.value ?? "";
  const unitValues = new Set([...select.options].map(option => option.value));
  const stepBase = Number.isFinite(min) ? min : Number(input.defaultValue || 0);
  const isValidNumber = number => {
    const increment = (number - stepBase) / Number(step);
    return Number.isFinite(number) && number >= min && number <= max && Number.isFinite(increment)
      && Math.abs(increment - Math.round(increment)) <= 1e-9;
  };
  const emit = () => {
    const number = input.valueAsNumber;
    if (disabled || !isValidNumber(number) || !unitValues.has(select.value)) return;
    value = number; unit = select.value; onChange?.(value, unit);
  };
  listen(input, "change", emit); listen(select, "change", emit); root.append(input, select);
  return controller(root, () => { input.removeEventListener("change", emit); select.removeEventListener("change", emit); }, { input, select, getValue: () => ({ value, unit }), setValue: (next, nextUnit = unit) => {
    if (disabled || !isValidNumber(next) || !unitValues.has(nextUnit)) return false;
    value = next; unit = nextUnit; input.value = String(next); select.value = nextUnit; return true;
  } });
}

export function createDialog({ label, content, closeLabel = label, initialFocus }) {
  const dialog = element("dialog", "ui-dialog");
  dialog.tabIndex = -1;
  const heading = element("h2", "ui-dialog-title", label); heading.id = idFor("dialog-title"); dialog.setAttribute("aria-labelledby", heading.id);
  const closeButton = element("button", "ui-button ui-dialog-close", closeLabel); closeButton.type = "button";
  const body = element("div", "ui-dialog-content");
  if (content instanceof Node) body.append(content); else body.textContent = String(content ?? "");
  dialog.append(heading, closeButton, body);
  let returnFocus = null;
  const focusables = () => [...dialog.querySelectorAll('button:not(:disabled),input:not(:disabled),select:not(:disabled),textarea:not(:disabled),a[href],summary,[contenteditable]:not([contenteditable="false"]),[tabindex]')].filter(node => {
    const editable = node.isContentEditable;
    if ((node.tabIndex < 0 && (!editable || node.hasAttribute("tabindex"))) || !node.isConnected || node.closest('[hidden],[inert],[aria-hidden="true"]')) return false;
    const closedDetails = node.closest('details:not([open])');
    if (closedDetails && !(node.tagName === "SUMMARY" && node.parentElement === closedDetails && closedDetails.querySelector("summary") === node)) return false;
    const closedPopover = node.closest("[popover]");
    if (closedPopover) { try { if (!closedPopover.matches(":popover-open")) return false; } catch { return false; } }
    const style = getComputedStyle(node);
    return style.display !== "none" && style.visibility !== "hidden" && style.visibility !== "collapse" && node.getClientRects().length > 0;
  });
  const open = ({ returnFocusTo = document.activeElement } = {}) => {
    returnFocus = returnFocusTo;
    if (!dialog.open) dialog.showModal();
    const target = typeof initialFocus === "function" ? initialFocus(dialog) : initialFocus;
    (target ?? focusables()[0] ?? dialog).focus?.();
  };
  const close = () => { if (dialog.open) dialog.close(); returnFocus?.focus(); };
  const onClose = () => returnFocus?.focus();
  const onTrap = event => {
    if (event.key !== "Tab") return;
    const list = focusables(); if (!list.length) { event.preventDefault(); dialog.focus(); return; }
    const current = list.indexOf(document.activeElement);
    const next = current < 0 ? 0 : (current + (event.shiftKey ? list.length - 1 : 1)) % list.length;
    event.preventDefault(); list[next].focus();
  };
  listen(closeButton, "click", close); listen(dialog, "close", onClose); listen(dialog, "keydown", onTrap);
  return controller(dialog, () => { close(); closeButton.removeEventListener("click", close); dialog.removeEventListener("close", onClose); dialog.removeEventListener("keydown", onTrap); }, { open, close, closeButton, content: body });
}

export function createContextMenu({ label, items = [], onSelect, target }) {
  const menu = makeMenu(items, { label, onSelect });
  menu.popup.classList.add("ui-context-menu");
  const open = ({ x = 0, y = 0, returnFocusTo = document.activeElement } = {}) => {
    menu.open({ returnFocusTo }); menu.popup.style.position = "fixed"; menu.popup.style.left = `${x}px`; menu.popup.style.top = `${y}px`;
    const rect = menu.popup.getBoundingClientRect();
    const margin = 4;
    const left = Math.max(margin, Math.min(x, window.innerWidth - rect.width - margin));
    const top = Math.max(margin, Math.min(y, window.innerHeight - rect.height - margin));
    menu.popup.style.left = `${left}px`; menu.popup.style.top = `${top}px`;
  };
  const onContext = event => { event.preventDefault(); open({ x: event.clientX, y: event.clientY, returnFocusTo: target }); };
  const onKey = event => {
    if (event.key !== "ContextMenu" && event.key !== "Menu" && !(event.key === "F10" && event.shiftKey)) return;
    event.preventDefault(); const rect = target.getBoundingClientRect(); open({ x: rect.left, y: rect.bottom, returnFocusTo: target });
  };
  if (target) { listen(target, "contextmenu", onContext); listen(target, "keydown", onKey); }
  return controller(menu.popup, () => { if (target) { target.removeEventListener("contextmenu", onContext); target.removeEventListener("keydown", onKey); } menu.dispose(); }, { open, close: menu.close, popup: menu.popup });
}

export function attachTabs({ tablist, tabs, orientation = "horizontal", label, onSelect }) {
  if (!tablist || !Array.isArray(tabs)) throw new TypeError("A tablist and tabs are required.");
  tablist.setAttribute("role", "tablist"); tablist.setAttribute("aria-orientation", orientation);
  if (label) tablist.setAttribute("aria-label", label);
  const tabId = tab => { if (!tab.id) tab.id = idFor("tab"); return tab.id; };
  const panelId = panel => { if (!panel.id) panel.id = idFor("panel"); return panel.id; };
  const available = () => tabs.map((entry, index) => ({ ...entry, index })).filter(entry => !entry.tab.disabled && entry.tab.getAttribute("aria-disabled") !== "true");
  const initiallySelected = tabs.findIndex(({ tab }) => tab.getAttribute("aria-selected") === "true");
  let selected = initiallySelected >= 0 && available().some(entry => entry.index === initiallySelected) ? initiallySelected : (available()[0]?.index ?? -1);
  const select = index => {
    const entry = tabs[index]; if (!entry || entry.tab.disabled || entry.tab.getAttribute("aria-disabled") === "true") return false;
    selected = index;
    tabs.forEach(({ tab, panel }, current) => {
      const active = current === selected;
      tab.setAttribute("role", "tab"); tab.setAttribute("aria-controls", panelId(panel)); tab.setAttribute("aria-selected", String(active)); tab.tabIndex = active ? 0 : -1;
      panel.setAttribute("role", "tabpanel"); panel.setAttribute("aria-labelledby", tabId(tab)); panel.hidden = !active;
    });
    onSelect?.(index, tabs[index]); return true;
  };
  tabs.forEach(({ tab, panel }) => {
    tablist.append(tab); tab.setAttribute("role", "tab"); tab.setAttribute("aria-controls", panelId(panel)); tab.setAttribute("aria-selected", "false"); tab.tabIndex = -1;
    panel.setAttribute("role", "tabpanel"); panel.setAttribute("aria-labelledby", tabId(tab)); panel.hidden = true;
  });
  const keydown = event => {
    const current = tabs.findIndex(entry => entry.tab === event.target); if (current < 0) return;
    const vertical = orientation === "vertical";
    const forward = vertical ? "ArrowDown" : "ArrowRight", backward = vertical ? "ArrowUp" : "ArrowLeft";
    const availableTabs = available(); if (!availableTabs.length) return;
    const position = availableTabs.findIndex(entry => entry.index === current);
    if (position < 0) return;
    let next;
    if (event.key === forward) next = (position + 1) % availableTabs.length;
    else if (event.key === backward) next = (position + availableTabs.length - 1) % availableTabs.length;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = availableTabs.length - 1;
    else return;
    event.preventDefault(); const target = availableTabs[next];
    target.tab.focus(); select(target.index);
  };
  listen(tablist, "keydown", keydown);
  const clicks = tabs.map(({ tab }, index) => { const click = () => select(index); listen(tab, "click", click); return [tab, click]; });
  if (selected >= 0) select(selected);
  return Object.freeze({ select, dispose() { tablist.removeEventListener("keydown", keydown); clicks.forEach(([tab, click]) => tab.removeEventListener("click", click)); } });
}

export function createTooltip({ trigger, text, delay = 350 }) {
  if (!trigger) throw new TypeError("A tooltip trigger is required.");
  const tooltip = element("div", "ui-tooltip", text); tooltip.setAttribute("role", "tooltip"); tooltip.id = idFor("tooltip"); tooltip.hidden = true;
  const previous = trigger.getAttribute("aria-describedby");
  const descriptions = new Set((previous ?? "").split(/\s+/).filter(Boolean)); descriptions.add(tooltip.id);
  trigger.setAttribute("aria-describedby", [...descriptions].join(" "));
  document.body.append(tooltip);
  let showTimer = 0, hideTimer = 0, focused = false, hoveringTrigger = false, hoveringTooltip = false, dismissed = false;
  const position = () => {
    const anchor = trigger.getBoundingClientRect();
    const rect = tooltip.getBoundingClientRect();
    const margin = 8;
    const maxLeft = Math.max(margin, window.innerWidth - rect.width - margin);
    const left = Math.max(margin, Math.min(anchor.left, maxLeft));
    const below = anchor.bottom + 6;
    const above = anchor.top - rect.height - 6;
    const top = below + rect.height <= window.innerHeight - margin ? below : Math.max(margin, above);
    tooltip.style.left = `${left}px`; tooltip.style.top = `${top}px`;
  };
  const schedule = () => {
    clearTimeout(hideTimer);
    if (dismissed || !(focused || hoveringTrigger || hoveringTooltip)) { clearTimeout(showTimer); hideTimer = setTimeout(() => { tooltip.hidden = true; }, 80); return; }
    clearTimeout(showTimer);
    showTimer = setTimeout(() => { tooltip.hidden = false; position(); }, delay);
  };
  const show = () => { dismissed = false; focused = true; schedule(); };
  const hide = () => { focused = false; hoveringTrigger = false; hoveringTooltip = false; clearTimeout(showTimer); clearTimeout(hideTimer); tooltip.hidden = true; };
  const onFocus = () => { dismissed = false; focused = true; schedule(); };
  const onBlur = () => { focused = false; schedule(); };
  const onTriggerEnter = () => { dismissed = false; hoveringTrigger = true; schedule(); };
  const onTriggerLeave = () => { hoveringTrigger = false; schedule(); };
  const onTooltipEnter = () => { hoveringTooltip = true; schedule(); };
  const onTooltipLeave = () => { hoveringTooltip = false; schedule(); };
  const onEscape = event => { if (event.key === "Escape") { dismissed = true; hide(); } };
  listen(trigger, "focus", onFocus); listen(trigger, "blur", onBlur); listen(trigger, "mouseenter", onTriggerEnter); listen(trigger, "mouseleave", onTriggerLeave); listen(trigger, "keydown", onEscape);
  listen(tooltip, "mouseenter", onTooltipEnter); listen(tooltip, "mouseleave", onTooltipLeave);
  return controller(trigger, () => {
    hide(); tooltip.remove(); trigger.removeEventListener("focus", onFocus); trigger.removeEventListener("blur", onBlur); trigger.removeEventListener("mouseenter", onTriggerEnter); trigger.removeEventListener("mouseleave", onTriggerLeave); trigger.removeEventListener("keydown", onEscape);
    tooltip.removeEventListener("mouseenter", onTooltipEnter); tooltip.removeEventListener("mouseleave", onTooltipLeave);
    const remaining = new Set((trigger.getAttribute("aria-describedby") ?? "").split(/\s+/).filter(Boolean)); remaining.delete(tooltip.id);
    if (remaining.size) trigger.setAttribute("aria-describedby", [...remaining].join(" ")); else trigger.removeAttribute("aria-describedby");
  }, { tooltip, show, hide }, { removeElement: false });
}
