// SPDX-License-Identifier: AGPL-3.0-or-later
import {
  attachTabs, createButton, createColorPicker, createContextMenu, createDialog,
  createDropdown, createFontCombo, createNumberUnitField, createSplitButton,
  createToggleButton, createTooltip,
} from "../web/ui-components.mjs";

const key = (target, name, extra = {}) => target.dispatchEvent(new KeyboardEvent("keydown", {
  key: name, bubbles: true, cancelable: true, ...extra,
}));
const click = target => target.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
const pointerDown = target => target.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, cancelable: true }));
const tick = () => new Promise(resolve => setTimeout(resolve, 0));

export async function run({ assert = (condition, message) => { if (!condition) throw new Error(message); }, fixture } = {}) {
  const root = typeof fixture === "function" ? await fixture() : fixture;
  const host = root instanceof Element ? root : document.createElement("div");
  if (!root) document.body.append(host);
  host.classList.add("ui-component-fixture");
  const checks = [];
  const verify = (condition, message) => { assert(condition, message); checks.push(message); };
  const controllers = [];
  const add = component => { controllers.push(component); host.append(component.element); return component; };
  try {
    let activated = 0;
    const button = add(createButton({ label: "Action test", onActivate: () => activated++ }));
    click(button.button);
    verify(activated === 1 && button.button.getAttribute("aria-label") === "Action test", "native button has a name and activates once");
    const disabled = createButton({ label: "Disabled", disabled: true, onActivate: () => activated++ });
    click(disabled.button);
    verify(activated === 1 && disabled.button.disabled, "disabled button does not dispatch its callback");
    disabled.dispose();

    let pressed;
    const toggle = add(createToggleButton({ label: "Emphasis", onChange: value => { pressed = value; } }));
    click(toggle.button);
    verify(toggle.isPressed() && pressed === true && toggle.button.getAttribute("aria-pressed") === "true", "toggle exposes and reports its pressed state");
    toggle.setPressed(false);
    verify(!toggle.isPressed(), "toggle setter updates aria-pressed");

    let actionCount = 0, menuValue;
    const split = add(createSplitButton({ label: "Primary action", menuLabel: "More actions", onActivate: () => actionCount++,
      items: [{ label: "One", value: "one" }, { label: "Two", value: "two" }], onSelect: value => { menuValue = value; } }));
    click(split.action); click(split.disclosure);
    verify(actionCount === 1 && split.disclosure.getAttribute("aria-expanded") === "true" && split.disclosure.getAttribute("aria-controls"), "split button separates its action and labeled menu trigger");
    pointerDown(split.disclosure); click(split.disclosure);
    verify(split.disclosure.getAttribute("aria-expanded") === "false", "pointerdown and click on an open split trigger toggles it closed");
    click(split.disclosure);
    key(split.menu?.popup ?? split.element.querySelector('[role="menu"]'), "ArrowDown");
    const splitItems = split.element.querySelectorAll('[role="menuitem"]');
    click(splitItems[1]);
    verify(menuValue === "two" && split.disclosure.getAttribute("aria-expanded") === "false", "split menu selection calls back and closes");

    let dropdownValue;
    const dropdown = add(createDropdown({ label: "Paragraph style", options: [{ label: "Normal", value: "normal" }, { label: "Title", value: "title" }], value: "normal", onChange: value => { dropdownValue = value; } }));
    click(dropdown.trigger);
    verify(dropdown.trigger.getAttribute("aria-expanded") === "true" && dropdown.element.querySelector('[role="listbox"]') && dropdown.trigger.getAttribute("aria-controls") === dropdown.element.querySelector('[role="listbox"]').id, "dropdown exposes an expanded listbox relationship");
    verify(document.activeElement?.getAttribute("aria-selected") === "true", "dropdown opens on its selected enabled option");
    pointerDown(dropdown.trigger); click(dropdown.trigger);
    verify(dropdown.trigger.getAttribute("aria-expanded") === "false", "pointerdown and click on an open dropdown trigger toggles it closed");
    click(dropdown.trigger);
    key(dropdown.element.querySelector('[role="option"]'), "ArrowDown");
    key(document.activeElement, "Enter");
    verify(dropdownValue === "title" && dropdown.getValue() === "title" && dropdown.trigger.getAttribute("aria-expanded") === "false", "dropdown supports arrows and Enter selection");
    click(dropdown.trigger); key(document.activeElement, "Escape");
    verify(dropdown.trigger.getAttribute("aria-expanded") === "false" && document.activeElement === dropdown.trigger, "Escape closes a dropdown and restores trigger focus");
    click(dropdown.trigger); key(document.activeElement, "Home"); key(document.activeElement, " ");
    verify(dropdown.getValue() === "normal", "Space activates the focused dropdown option");
    click(dropdown.trigger); pointerDown(document.body);
    verify(dropdown.trigger.getAttribute("aria-expanded") === "false", "outside pointer dismissal closes a dropdown");

    let fontValue;
    const font = add(createFontCombo({ label: "Font family", options: [{ label: "Carlito", value: "Carlito" }, { label: "Calibri", value: "Calibri" }], value: "Carlito", onChange: value => { fontValue = value; } }));
    font.input.focus(); key(font.input, "ArrowDown");
    verify(!font.element.querySelector('[role="listbox"]').hidden && font.element.querySelectorAll('[role="option"]').length === 2, "font combo opens all suggestions from closed state without typing");
    key(font.input, "ArrowDown");
    font.input.value = "Cal"; font.input.dispatchEvent(new InputEvent("input", { bubbles: true, data: "Cal" }));
    verify(!font.input.hasAttribute("aria-activedescendant"), "font filtering clears the old active descendant");
    document.documentElement.dataset.theme = "dark";
    const activeFont = font.element.querySelector('[role="option"]'); activeFont.setAttribute("aria-selected", "true");
    const tokenProbe = document.createElement("span");
    if (matchMedia("(forced-colors: active)").matches) { tokenProbe.style.color = "HighlightText"; tokenProbe.style.backgroundColor = "Highlight"; }
    else { tokenProbe.style.color = "var(--chrome-ink)"; tokenProbe.style.backgroundColor = "var(--chrome-control-hover)"; }
    host.append(tokenProbe);
    verify(getComputedStyle(activeFont).color === getComputedStyle(tokenProbe).color && getComputedStyle(activeFont).backgroundColor === getComputedStyle(tokenProbe).backgroundColor,
      matchMedia("(forced-colors: active)").matches ? "font list selection uses forced-colors system foreground and background" : "font list selection uses semantic dark theme foreground and background tokens");
    tokenProbe.remove();
    delete document.documentElement.dataset.theme;
    key(font.input, "Escape");
    const compositionStart = new CompositionEvent("compositionstart", { bubbles: true }); font.input.dispatchEvent(compositionStart);
    font.input.value = "Calibri"; font.input.dispatchEvent(new InputEvent("input", { bubbles: true, data: "Calibri", isComposing: true }));
    verify(font.element.querySelector('[role="listbox"]').hidden, "font combo suppresses filtering during IME composition");
    font.input.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: "Calibri" }));
    verify(!font.element.querySelector('[role="listbox"]').hidden && font.element.querySelector('[role="option"]').style.fontFamily === "Calibri", "font combo filters results and previews each font");
    key(font.input, "ArrowDown");
    verify(font.input.getAttribute("aria-activedescendant") === font.element.querySelector('[role="option"]').id, "font combo tracks its active descendant");
    key(font.input, "Enter");
    verify(fontValue === "Calibri" && font.getValue() === "Calibri", "font combo commits a keyboard-selected family");
    key(font.input, "ArrowDown"); pointerDown(document.body);
    verify(font.input.getAttribute("aria-expanded") === "false" && font.input.getAttribute("aria-activedescendant") === null, "font combo closes and clears its active descendant on outside input");
    key(font.input, "ArrowDown"); button.button.focus(); await tick();
    verify(font.input.getAttribute("aria-expanded") === "false", "font combo closes when focus leaves its root");

    let colorValue;
    const colors = add(createColorPicker({ label: "Text color", themeColors: { label: "Theme", colors: [{ label: "Green", value: "#236c57" }] },
      standardColors: { label: "Standard", colors: [{ label: "Black", value: "#000000" }] }, value: "#000000", customLabel: "Custom", onChange: value => { colorValue = value; } }));
    click(colors.trigger);
    verify(colors.popup.getAttribute("role") === "dialog" && colors.popup.getAttribute("aria-label") === "Text color" && colors.trigger.getAttribute("aria-expanded") === "true", "color picker opens a named dialog");
    const colorSwatches = colors.popup.querySelectorAll(".ui-color-swatch"); colorSwatches[0].focus(); key(colorSwatches[0], "ArrowRight");
    verify(document.activeElement === colorSwatches[1], "color swatches support arrow-key navigation");
    click(colors.popup.querySelector('[aria-label="Green"]'));
    verify(colorValue === "#236c57" && colors.getValue() === "#236c57", "color picker reports a named theme color");
    colors.setValue("#aabbcc");
    verify(colors.popup.querySelector('input[type="color"]').value === "#aabbcc", "color picker accepts custom colors");
    const callbacksBeforeInvalidColor = colorValue;
    verify(colors.setValue("not-a-color") === false && colors.getValue() === "#aabbcc" && colorValue === callbacksBeforeInvalidColor && colors.popup.querySelector('input[type="color"]').value === "#aabbcc", "invalid colors leave state, native input and callbacks unchanged");
    verify(colors.setValue("#236c57") && colors.getValue() === "#236c57", "color picker accepts caller-supplied palette values");

    let numberChange;
    const number = add(createNumberUnitField({ label: "Margin", value: 1, unit: "cm", units: ["cm", "pt"], min: 0, max: 5, step: 0.1,
      onChange: (value, unit) => { numberChange = [value, unit]; } }));
    number.input.value = "Infinity"; number.input.dispatchEvent(new Event("change", { bubbles: true }));
    verify(numberChange === undefined, "number/unit rejects non-finite draft values");
    number.input.value = "2.3"; number.input.dispatchEvent(new Event("change", { bubbles: true }));
    number.select.value = "pt"; number.select.dispatchEvent(new Event("change", { bubbles: true }));
    verify(numberChange?.[0] === 2.3 && numberChange?.[1] === "pt", "number/unit reports a finite bounded value and selected unit");
    number.input.value = "8"; number.input.dispatchEvent(new Event("change", { bubbles: true }));
    verify(numberChange?.[0] === 2.3, "out-of-range numeric drafts do not dispatch changes");
    const fractionalBase = add(createNumberUnitField({ label: "Indented margin", value: 0.5, unit: "cm", units: ["cm", "pt"], min: 0.5, max: 5, step: 1 }));
    fractionalBase.input.value = "1.5"; fractionalBase.input.dispatchEvent(new Event("change", { bubbles: true }));
    verify(fractionalBase.getValue().value === 1.5, "number step validation uses fractional HTML min as its step base");
    const beforeUnknownUnit = fractionalBase.getValue();
    verify(fractionalBase.setValue(2.5, "inch") === false && fractionalBase.getValue().value === beforeUnknownUnit.value && fractionalBase.select.value === "cm", "number/unit rejects an unknown unit without changing state or select");

    const returnFocus = document.createElement("button"); returnFocus.textContent = "Open settings"; host.append(returnFocus); returnFocus.focus();
    const dialogContent = document.createElement("div");
    const editable = document.createElement("div"); editable.contentEditable = "true"; editable.textContent = "Editable settings"; dialogContent.append(editable);
    const dialogDropdown = createDropdown({ label: "Dialog option", options: [{ label: "One", value: "one" }, { label: "Two", value: "two" }], value: "one" });
    dialogContent.append(dialogDropdown.element);
    const details = document.createElement("details"), summary = document.createElement("summary"); summary.textContent = "Advanced"; details.append(summary); dialogContent.append(details);
    const dialog = add(createDialog({ label: "Settings", content: dialogContent, closeLabel: "Close settings" }));
    dialog.open({ returnFocusTo: returnFocus });
    verify(dialog.element.open && dialog.element.getAttribute("aria-labelledby") && document.activeElement === dialog.closeButton, "modal dialog opens with an accessible title and initial focus");
    key(dialog.closeButton, "Tab"); verify(document.activeElement === editable, "dialog Tab order includes editable content");
    key(editable, "Tab"); verify(document.activeElement === dialogDropdown.trigger, "dialog Tab order includes nested dropdown trigger");
    key(dialogDropdown.trigger, "Tab"); verify(document.activeElement === summary, "dialog trap skips descendants of its closed dropdown popup and keeps closed-details summary focusable");
    key(summary, "Tab", { shiftKey: true }); verify(document.activeElement === dialogDropdown.trigger, "dialog Shift+Tab moves back from the summary");
    dialogDropdown.open(); dialogDropdown.trigger.focus(); key(dialogDropdown.trigger, "Tab");
    verify(document.activeElement === dialogDropdown.element.querySelector('[role="option"]'), "dialog trap includes options from an open nested dropdown");
    key(document.activeElement, "Tab");
    verify(document.activeElement === dialog.closeButton, "dialog trap skips the negative-tabindex option when cycling after nested popup focus");
    dialogDropdown.close(); dialogDropdown.trigger.focus(); key(dialogDropdown.trigger, "Tab");
    verify(document.activeElement === summary, "dialog trap keeps a summary in a closed details element focusable");
    dialog.close();
    verify(!dialog.element.open && document.activeElement === returnFocus, "modal close restores focus to its opener");
    dialogDropdown.dispose();

    let contextValue;
    const contextTarget = elementForTest("button", "Context target"); host.append(contextTarget);
    const context = add(createContextMenu({ label: "Edit actions", target: contextTarget, items: [{ label: "Copy", value: "copy" }, { label: "Paste", value: "paste" }], onSelect: value => { contextValue = value; } }));
    key(contextTarget, "F10", { shiftKey: true });
    verify(!context.popup.hidden, "context menu opens from Shift+F10 on its bound target");
    context.close({ restoreFocus: false });
    returnFocus.focus(); context.open({ x: 12, y: 18, returnFocusTo: returnFocus });
    verify(context.popup.getAttribute("role") === "menu" && document.activeElement === context.popup.firstElementChild, "context menu opens with focus on its first command");
    key(document.activeElement, "End"); key(document.activeElement, "Enter");
    verify(contextValue === "paste" && document.activeElement === returnFocus, "context menu supports End/Enter and restores focus");
    context.open({ x: window.innerWidth - 2, y: window.innerHeight - 2, returnFocusTo: returnFocus });
    const edgeMenuRect = context.popup.getBoundingClientRect();
    verify(edgeMenuRect.right <= window.innerWidth && edgeMenuRect.bottom <= window.innerHeight, "context menu clamps its laid-out popup inside viewport edges");
    context.close({ restoreFocus: false });

    const tablist = elementForTest("div"), tabA = elementForTest("button", "A"), tabB = elementForTest("button", "B"), tabDisabled = elementForTest("button", "Disabled"), panelA = elementForTest("section", "Panel A"), panelB = elementForTest("section", "Panel B"), panelDisabled = elementForTest("section", "Panel disabled");
    tabDisabled.disabled = true; host.append(tablist, panelA, panelB, panelDisabled);
    const tabs = attachTabs({ tablist, tabs: [{ tab: tabA, panel: panelA }, { tab: tabB, panel: panelB }, { tab: tabDisabled, panel: panelDisabled }] });
    key(tabA, "ArrowRight");
    verify(tabB.getAttribute("aria-selected") === "true" && tabB.tabIndex === 0 && !panelB.hidden && panelA.hidden, "tabs synchronize roving focus, selected state and visible panel");
    key(tabB, "End");
    verify(document.activeElement === tabB, "tabs skip disabled entries during keyboard navigation");
    tabs.select(0);
    verify(tabA.getAttribute("aria-controls") === panelA.id && panelA.getAttribute("aria-labelledby") === tabA.id, "tabs connect labels and panels");
    tabA.focus(); key(tabA, "ArrowLeft");
    verify(tabB.getAttribute("aria-selected") === "true", "tabs wrap ArrowLeft from the first enabled tab to the last");
    tabs.dispose(); key(tabB, "ArrowLeft");
    verify(tabB.getAttribute("aria-selected") === "true", "disposed tabs no longer handle keyboard navigation");
    tablist.remove(); panelA.remove(); panelB.remove(); panelDisabled.remove();
    const disabledTablist = elementForTest("div"), disabledTab = elementForTest("button", "Unavailable"), disabledPanel = elementForTest("section", "Unavailable panel");
    disabledTab.disabled = true; host.append(disabledTablist, disabledPanel);
    const disabledTabs = attachTabs({ tablist: disabledTablist, tabs: [{ tab: disabledTab, panel: disabledPanel }] });
    verify(disabledTab.getAttribute("role") === "tab" && disabledTab.getAttribute("aria-selected") === "false" && disabledPanel.hidden, "all-disabled tab sets remain fully labeled and unavailable");
    disabledTabs.dispose(); disabledTablist.remove(); disabledPanel.remove();
    const verticalList = elementForTest("div"), verticalA = elementForTest("button", "Vertical A"), verticalB = elementForTest("button", "Vertical B"), verticalPanelA = elementForTest("section", "Vertical panel A"), verticalPanelB = elementForTest("section", "Vertical panel B");
    host.append(verticalList, verticalPanelA, verticalPanelB);
    const verticalTabs = attachTabs({ tablist: verticalList, orientation: "vertical", tabs: [{ tab: verticalA, panel: verticalPanelA }, { tab: verticalB, panel: verticalPanelB }] });
    verticalA.focus(); key(verticalA, "ArrowUp");
    verify(verticalB.getAttribute("aria-selected") === "true", "vertical tabs wrap ArrowUp from the first tab to the last");
    verticalTabs.dispose(); verticalList.remove(); verticalPanelA.remove(); verticalPanelB.remove();

    returnFocus.setAttribute("aria-describedby", "existing-help");
    const tooltip = createTooltip({ trigger: returnFocus, text: "Helpful text", delay: 0 });
    verify(tooltip.tooltip.isConnected && tooltip.tooltip.hidden && returnFocus.getAttribute("aria-describedby").includes(tooltip.tooltip.id), "tooltip description stays connected and hidden before its delay");
    returnFocus.focus(); await tick();
    verify(!tooltip.tooltip.hidden && tooltip.tooltip.getAttribute("role") === "tooltip", "tooltip becomes visible on focus");
    key(returnFocus, "Escape");
    verify(tooltip.tooltip.hidden, "Escape hides a visible tooltip");
    returnFocus.dispatchEvent(new MouseEvent("mouseenter")); await tick();
    returnFocus.dispatchEvent(new MouseEvent("mouseleave", { relatedTarget: tooltip.tooltip }));
    tooltip.tooltip.dispatchEvent(new MouseEvent("mouseenter"));
    verify(!tooltip.tooltip.hidden, "tooltip remains visible while the pointer moves from trigger onto tooltip");
    returnFocus.getBoundingClientRect = () => new DOMRect(window.innerWidth - 5, window.innerHeight - 5, 1, 1);
    tooltip.show(); await tick(); const tooltipRect = tooltip.tooltip.getBoundingClientRect();
    verify(tooltipRect.left >= 0 && tooltipRect.right <= window.innerWidth && tooltipRect.top >= 0 && tooltipRect.bottom <= window.innerHeight, "tooltip placement clamps to the viewport");
    returnFocus.dispatchEvent(new FocusEvent("blur")); tooltip.tooltip.dispatchEvent(new MouseEvent("mouseleave")); await new Promise(resolve => setTimeout(resolve, 100));
    verify(tooltip.tooltip.hidden, "tooltip hides after both focus and hover leave");
    tooltip.dispose();
    verify(!tooltip.tooltip.isConnected && returnFocus.getAttribute("aria-describedby") === "existing-help" && returnFocus.isConnected, "tooltip disposal removes its description and preserves caller DOM");
    returnFocus.blur();
    const delayedTooltip = createTooltip({ trigger: returnFocus, text: "Delayed", delay: 25 });
    returnFocus.focus(); delayedTooltip.dispose(); await new Promise(resolve => setTimeout(resolve, 35));
    verify(!delayedTooltip.tooltip.isConnected, "tooltip disposal cancels a pending show timer");
    return checks;
  } finally {
    controllers.reverse().forEach(component => component.dispose());
    if (!root) host.remove();
  }
}

function elementForTest(name, text) {
  const node = document.createElement(name);
  if (text) node.textContent = text;
  if (name === "button") node.type = "button";
  return node;
}
