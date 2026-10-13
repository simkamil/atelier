// SPDX-License-Identifier: AGPL-3.0-or-later
import { decodeDisplayList } from "../web/display-list.mjs";

/** Exercise the status widget through the real app, editor and viewport callbacks. */
export async function checkDocumentStatus({ app, assert, waitFor, key, inputText, openText }) {
  const status = app.documentStatus;
  const scrollport = document.querySelector("#document-scroll");
  const footer = document.querySelector("footer");
  const wordsSlot = document.querySelector("#statistics-slot");
  const zoomSlot = document.querySelector("#document-zoom");
  const pageField = status?.element.querySelector('[data-status-field="page"]');
  const pageButton = pageField?.querySelector("button");
  const languageField = status?.element.querySelector('[data-status-field="language"]');
  const savedField = status?.element.querySelector('[data-status-field="saved"]');
  const dialog = status?.element.querySelector("[data-status-goto-dialog]");
  const menu = status?.element.querySelector("[data-status-visibility-menu]");
  const initialVisibility = status?.getVisibility();
  const initialSlotHidden = { words: wordsSlot?.hidden, zoom: zoomSlot?.hidden };
  const initialZoom = app.editor.getZoom();
  const initialScrollTop = scrollport.scrollTop;
  const pageInfo = () => {
    const match = /^Page\s+(\d+)\s+sur\s+(\d+)$/u.exec(pageButton?.textContent.trim() ?? "");
    return match ? { page: Number(match[1]), total: Number(match[2]) } : null;
  };
  const openMenu = async () => {
    if (menu.hidden) key(footer, "F10", { shiftKey: true });
    await waitFor(() => !menu.hidden, "document status visibility menu opens");
  };
  const closeMenu = async () => {
    if (!menu?.hidden) {
      key(document.activeElement, "Escape");
      await waitFor(() => menu.hidden, "document status visibility menu closes");
    }
  };
  const setVisible = async (field, visible) => {
    if (status.getVisibility()[field] === visible) return;
    await openMenu();
    const item = menu.querySelector(`[data-visibility-field="${field}"]`);
    assert(item, `status menu exposes the ${field} visibility control`);
    item.click();
    await waitFor(() => status.getVisibility()[field] === visible && menu.hidden,
      `${field} visibility changes through the status menu`);
  };
  const sameEditorState = before => {
    const current = app.getState();
    assert(current.text === before.text && current.anchor === before.anchor && current.focus === before.focus
      && current.dirty === before.dirty && current.canUndo === before.canUndo && current.canRedo === before.canRedo
      && current.revision === before.revision,
    "scroll, page navigation and zoom preserve text, selection, dirty state, history availability and revision");
  };

  assert(status && pageButton && languageField && savedField && dialog && menu && scrollport && footer && wordsSlot && zoomSlot,
    "real app exposes document status fields, page dialog, visibility menu and parent slots");
  try {
    const documentText = Array.from({ length: 320 }, (_, index) =>
      `Ligne ${String(index + 1).padStart(3, "0")} — contenu de test pour le défilement et la navigation des pages.`).join("\n");
    await openText(documentText);
    await waitFor(() => app.getState()?.text === documentText && !app.getState()?.dirty
      && pageInfo()?.total >= 2, "clean multi-page TXT and status page count");
    const firstPageHeight = decodeDisplayList(app.getState().pages[0]).page.bounds.height;
    const pageTwoOffset = firstPageHeight + 16;
    assert(!scrollport.hidden && scrollport.scrollHeight > scrollport.clientHeight,
      "multi-page TXT has a real scrollable viewport");
    assert(languageField.textContent.includes("non définie")
      && !languageField.textContent.includes(document.documentElement.lang),
    "document language remains undefined and is distinct from the French interface locale");
    assert(savedField.textContent === "Enregistré", "a clean opened TXT document reports its saved state");

    if (!status.getVisibility().page) await setVisible("page", true);
    app.editor.setZoom(1);
    scrollport.scrollTop = pageTwoOffset;
    scrollport.dispatchEvent(new Event("scroll", { bubbles: true }));
    await waitFor(() => app.editor.getViewportState().page === 2 && pageInfo()?.page === 2,
      "scrolling to page two updates both viewport and status");

    key(app.editor.element, "End", { ctrlKey: true });
    await waitFor(() => app.getState()?.focus === new TextEncoder().encode(documentText).length,
      "caret reaches the end before the real edit");
    inputText(app.editor.element, "X");
    await waitFor(() => app.getState()?.text === `${documentText}X` && app.getState()?.dirty,
      "real TXT edit becomes dirty");
    key(app.editor.element, "Home", { ctrlKey: true });
    await waitFor(() => app.getState()?.anchor === 0 && app.getState()?.focus === 0,
      "caret returns to the document start");
    key(app.editor.element, "ArrowRight", { shiftKey: true });
    await waitFor(() => app.getState()?.anchor === 0 && app.getState()?.focus === 1,
      "directed selection is established for navigation invariance");
    assert(savedField.textContent === "Non enregistré", "an edited TXT document reports unsaved changes");
    const beforeNavigation = app.getState();
    assert(beforeNavigation.dirty && beforeNavigation.canUndo && beforeNavigation.anchor !== beforeNavigation.focus,
      "invariance baseline includes a dirty document, undo history and a directed selection");

    scrollport.scrollTop = pageTwoOffset;
    scrollport.dispatchEvent(new Event("scroll", { bubbles: true }));
    await waitFor(() => app.editor.getViewportState().page === 2 && pageInfo()?.page === 2,
      "scroll status remains synchronized after editing");
    sameEditorState(beforeNavigation);

    scrollport.scrollTop = 0;
    scrollport.dispatchEvent(new Event("scroll", { bubbles: true }));
    await waitFor(() => app.editor.getViewportState().page === 1 && pageInfo()?.page === 1,
      "scrolling back to page one updates both viewport and status before dialog navigation");
    const pageOneScrollTop = scrollport.scrollTop;
    pageButton.click();
    await waitFor(() => dialog.open, "real status page dialog opens");
    const pageInput = dialog.querySelector("[data-goto-page]");
    const pageForm = dialog.querySelector(".document-status-goto-form");
    pageInput.value = "2";
    pageForm.requestSubmit();
    await waitFor(() => !dialog.open && scrollport.scrollTop > pageOneScrollTop
      && app.editor.getViewportState().page === 2 && pageInfo()?.page === 2,
      "status dialog navigates the real TXT viewport to page two");
    sameEditorState(beforeNavigation);

    app.editor.setZoom(1.25);
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await waitFor(() => app.editor.getViewportState().page === 2 && pageInfo()?.page === 2,
      "status page remains synchronized to page two after zoom changes");
    assert(Math.abs(app.editor.getZoom() - 1.25) < 0.001, "real TXT zoom changes while the status page stays synchronized");
    sameEditorState(beforeNavigation);

    const originalVisible = { ...initialVisibility };
    for (const field of ["words", "zoom"]) {
      if (!status.getVisibility()[field]) await setVisible(field, true);
      const slot = field === "words" ? wordsSlot : zoomSlot;
      const item = menu.querySelector(`[data-visibility-field="${field}"]`);
      assert(!slot.hidden && item.getAttribute("aria-checked") === "true",
        `${field} parent slot starts visible with its checked menu state`);
      await setVisible(field, false);
      assert(slot.hidden && item.getAttribute("aria-checked") === "false",
        `${field} menu choice hides the parent-owned app slot`);
      await setVisible(field, true);
      assert(!slot.hidden && item.getAttribute("aria-checked") === "true",
        `${field} menu choice restores the parent-owned app slot`);
      if (!originalVisible[field]) await setVisible(field, false);
      assert(status.getVisibility()[field] === originalVisible[field] && slot.hidden === initialSlotHidden[field],
        `${field} visibility returns to its initial session state`);
    }

    await closeMenu();
    assert(!dialog.open && menu.hidden, "integration test leaves the page dialog and visibility menu closed");
    await app.editor.undo();
    await waitFor(() => app.getState()?.text === documentText && !app.getState()?.dirty
      && savedField.textContent === "Enregistré", "one undo restores the opened clean document and saved status");
  } finally {
    try {
      if (dialog?.open) dialog.querySelector("[data-goto-cancel]")?.click();
      await closeMenu();
      for (const field of ["page", "words", "language", "view", "zoom", "saved"]) {
        if (initialVisibility && status.getVisibility()[field] !== initialVisibility[field]) {
          await setVisible(field, initialVisibility[field]);
        }
      }
      if (app.editor.getZoom() !== initialZoom) app.editor.setZoom(initialZoom);
      scrollport.scrollTop = initialScrollTop;
      scrollport.dispatchEvent(new Event("scroll", { bubbles: true }));
    } catch { /* Best-effort fixture cleanup must not hide the original assertion failure. */ }
  }
}
