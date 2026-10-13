// SPDX-License-Identifier: AGPL-3.0-or-later
import { createDocumentStatus } from "../web/document-status.mjs";

const LABELS = Object.freeze({
  statusPageCount: "Page {page} of {total}", statusPageUnavailable: "Page unavailable",
  statusLanguage: "Language: {language}", statusLanguageUnknown: "Language undefined",
  statusViewText: "Text", statusViewPreview: "Preview", statusSaved: "Saved",
  statusUnsaved: "Unsaved", statusReadOnly: "Read only", statusConfigure: "Configure status",
  statusShowPage: "Show page", statusShowWords: "Show words", statusShowLanguage: "Show language",
  statusShowView: "Show view", statusShowZoom: "Show zoom", statusShowSaved: "Show saved state",
  statusGotoTitle: "Go to page", statusGotoPage: "Page number", statusGotoApply: "Go",
  statusGotoCancel: "Cancel", statusGotoInvalid: "Enter a whole page number",
  statusGotoRange: "Enter a page from 1 to {total}", statusGotoClose: "Close page dialog",
});

const key = (target, name, extra = {}) => target.dispatchEvent(new KeyboardEvent("keydown", {
  key: name, bubbles: true, cancelable: true, ...extra,
}));
const click = target => target.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));

export async function run({ assert = (condition, message) => { if (!condition) throw new Error(message); }, fixture } = {}) {
  if (!document.querySelector('link[data-document-status-test-style]')) {
    const stylesheet = document.createElement("link");
    stylesheet.rel = "stylesheet";
    stylesheet.href = "/document-status.css";
    stylesheet.dataset.documentStatusTestStyle = "";
    document.head.append(stylesheet);
    await new Promise((resolve, reject) => {
      stylesheet.addEventListener("load", resolve, { once: true });
      stylesheet.addEventListener("error", reject, { once: true });
    });
  }
  const provided = typeof fixture === "function" ? await fixture() : fixture;
  const host = provided instanceof Element ? provided : document.createElement("div");
  if (!provided) document.body.append(host);
  const checks = [];
  const verify = (condition, message) => { assert(condition, message); checks.push(message); };
  const components = [];
  const add = component => { components.push(component); host.append(component.element); return component; };
  try {
    let requestedPage;
    const changes = [];
    const status = add(createDocumentStatus({
      labels: LABELS,
      onGoToPage: page => { requestedPage = page; },
      onVisibilityChange: change => changes.push(change),
    }));
    const field = name => status.element.querySelector(`[data-status-field="${name}"]`);
    const pageButton = status.element.querySelector("[data-status-field=page] button");
    const dialog = status.element.querySelector("[data-status-goto-dialog]");
    const input = dialog.querySelector("[data-goto-page]");
    const form = dialog.querySelector(".document-status-goto-form");
    const error = dialog.querySelector(".document-status-goto-error");
    const apply = dialog.querySelector("[data-goto-apply]");
    const cancel = dialog.querySelector("[data-goto-cancel]");

    status.setState({ page: 1, totalPages: 1, language: null, mode: "text", dirty: false, busy: false });
    verify(pageButton.textContent === "Page 1 of 1" && !pageButton.disabled, "single-page lower and upper boundary remains navigable");
    verify(status.element.getAttribute("role") === "group" && status.element.getAttribute("aria-label") === LABELS.statusConfigure, "status group has its localized accessible name");
    verify(field("language").textContent === "Language undefined", "missing document language uses its localized unknown label");
    verify(field("saved").textContent === "Saved", "clean text documents show the saved state");

    pageButton.focus(); click(pageButton);
    verify(dialog.open && document.activeElement === input && pageButton.getAttribute("aria-expanded") === "true", "page dialog opens modally with its labeled numeric input focused");
    input.value = "1"; form.requestSubmit();
    verify(requestedPage === 1 && !dialog.open, "native form submission applies the first page without a button click");

    status.setState({ page: 8, totalPages: 8 });
    verify(pageButton.textContent === "Page 8 of 8", "last page boundary is shown without clamping");
    click(pageButton); input.value = "8"; click(apply);
    verify(requestedPage === 8, "valid last page is passed to visual navigation");

    click(pageButton); input.value = "1.5"; click(apply);
    verify(error.textContent === LABELS.statusGotoInvalid && requestedPage === 8 && dialog.open, "fractional page is rejected without navigating or closing the dialog");
    input.value = "9"; click(apply);
    verify(error.textContent === "Enter a page from 1 to 8" && requestedPage === 8, "out-of-range page reports the current page limit");
    input.value = "0"; click(apply);
    verify(error.textContent === "Enter a page from 1 to 8", "page zero is rejected at the lower boundary");
    const beforeCancel = requestedPage;
    click(cancel);
    verify(!dialog.open && requestedPage === beforeCancel, "cancel closes the page dialog without invoking navigation");
    click(pageButton);
    verify(dialog.open && error.textContent === "" && !input.hasAttribute("aria-invalid"), "reopening clears the prior validation message and invalid state");

    status.setState({ mode: "preview", dirty: null, busy: true });
    verify(field("view").textContent === "Preview" && field("saved").textContent === "Read only", "preview shows its view and read-only state");
    verify(pageButton.disabled && apply.disabled && input.disabled, "busy preview blocks page navigation controls");
    input.value = "2"; click(apply);
    verify(requestedPage === 8 && dialog.open, "busy state prevents submission even when the modal remains open");
    status.setState({ busy: false });
    verify(!pageButton.disabled && !apply.disabled, "clearing busy restores visual page navigation in preview mode");
    click(apply);
    verify(requestedPage === 2, "preview allows visual page navigation without a document mutation callback");

    status.setState({ mode: "text", dirty: true, language: "fr-FR" });
    verify(field("saved").textContent === "Unsaved" && field("language").textContent === "Language: fr-FR", "rerender updates language and dirty state in place");
    const initialFieldCount = status.element.querySelectorAll("[data-status-field]").length;
    for (let i = 0; i < 5; i++) status.setState({ page: 3, totalPages: 8, language: "fr-FR" });
    verify(status.element.querySelectorAll("[data-status-field]").length === initialFieldCount, "duplicate renders reuse the existing field nodes");

    status.element.focus(); key(status.element, "F10", { shiftKey: true });
    const menu = status.element.querySelector("[data-status-visibility-menu]");
    const pageChoice = menu.querySelector('[data-visibility-field="page"]');
    verify(!menu.hidden && pageChoice.getAttribute("role") === "menuitemcheckbox" && pageChoice.getAttribute("aria-checked") === "true", "default target opens a named keyboard menu with checked visibility items");
    const marker = () => getComputedStyle(pageChoice, "::before").content;
    verify(marker().includes("✓") && getComputedStyle(pageChoice).paddingInlineStart !== "0px", "checked visibility choice has a visible checkmark and reserved indicator space");
    click(pageChoice);
    verify(field("page").hidden && pageChoice.getAttribute("aria-checked") === "false" && status.getVisibility().page === false, "menu choice hides its field and synchronizes aria-checked");
    verify(!marker().includes("✓"), "unchecked visibility choice removes the visible checkmark");
    verify(changes.at(-1)?.field === "page" && changes.at(-1)?.visible === false, "visibility callback reports the field and new state");
    const wordsChoice = menu.querySelector('[data-visibility-field="words"]');
    status.element.focus(); key(status.element, "F10", { shiftKey: true });
    click(wordsChoice);
    verify(status.getVisibility().words === false && changes.at(-1)?.field === "words", "parent-owned word visibility is reported without rendering its slot");
    const zoomChoice = menu.querySelector('[data-visibility-field="zoom"]');
    status.element.focus(); key(status.element, "F10", { shiftKey: true });
    click(zoomChoice);
    verify(status.getVisibility().zoom === false && changes.at(-1)?.field === "zoom", "parent-owned zoom visibility is reported without rendering its slot");
    status.element.focus(); key(status.element, "F10", { shiftKey: true });
    verify(!menu.hidden, "menu can be reopened from Shift+F10 after a selection");
    const changesBeforeEscape = changes.length;
    key(document.activeElement, "Escape");
    verify(menu.hidden && document.activeElement === status.element && changes.length === changesBeforeEscape, "Escape closes the menu, restores focus, and emits no visibility callback");

    status.setState({ page: null, totalPages: null });
    verify(pageButton.textContent === "Page unavailable" && pageButton.disabled, "invalid layout state reports unavailable page and disables navigation");

    const partial = add(createDocumentStatus({ labels: { statusConfigure: "Configure", statusShowPage: "Show page" } }));
    verify(!partial.element.querySelector("[role=menu]") && !partial.element.textContent.includes("statusShow"), "partial catalogs omit unlabeled menu actions and never expose raw keys");
    const missing = add(createDocumentStatus({ labels: {} }));
    verify(missing.element.textContent === "" && !missing.element.querySelector("button"), "missing catalog produces no untranslated or unlabeled tools");

    const externalTarget = document.createElement("button");
    externalTarget.type = "button"; externalTarget.textContent = "External menu target"; host.append(externalTarget);
    let externalChanges = 0;
    const external = add(createDocumentStatus({ document, labels: LABELS, menuTarget: externalTarget, onVisibilityChange: () => externalChanges++ }));
    externalTarget.focus(); key(externalTarget, "F10", { shiftKey: true });
    const externalMenu = external.element.querySelector("[role=menu]");
    verify(!externalMenu.hidden, "provided target receives the named keyboard menu");
    external.dispose();
    key(externalTarget, "F10", { shiftKey: true });
    verify(!externalMenu.isConnected && externalMenu.hidden && externalChanges === 0, "disposing an open menu removes its listener without calling visibility handlers");

    status.element.focus(); key(status.element, "F10", { shiftKey: true });
    click(menu.querySelector('[data-visibility-field="page"]'));
    status.setState({ page: 8, totalPages: 8 });
    click(pageButton);
    const beforeDisposePage = requestedPage;
    const beforeDisposeChanges = changes.length;
    status.dispose();
    verify(!status.element.isConnected && !dialog.open && requestedPage === beforeDisposePage && changes.length === beforeDisposeChanges, "disposing an open page dialog removes the widget without callbacks");
    return checks;
  } finally {
    components.reverse().forEach(component => component.dispose());
    if (!provided) host.remove();
  }
}
