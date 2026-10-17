// SPDX-License-Identifier: AGPL-3.0-or-later
const frame = () => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
const wheel = (node, values = {}) => {
  const bounds = node.getBoundingClientRect();
  const event = new WheelEvent("wheel", { deltaY: -80, clientX: bounds.left + 150,
    clientY: bounds.top + 120, bubbles: true, cancelable: true, ...values });
  node.dispatchEvent(event);
  return event;
};

export async function checkEditorZoom({ editor, scrollport, canvas, openText, assert, waitFor, key, inputText, pointer }) {
  await openText(Array.from({ length: 250 }, (_, i) => `ligne ${i} café`).join("\r\n"));
  editor.setZoom(1);
  scrollport.scrollTop = 1200;
  await frame();
  const before = editor.state;
  const oldY = scrollport.scrollTop + scrollport.clientHeight / 2;
  const page = Math.floor(oldY / (841.89 + 16));
  const offset = oldY - page * (841.89 + 16);
  editor.setZoom(2);
  const expectedY = page * (841.89 * 2 + 16) + offset * 2;
  assert(Math.abs(scrollport.scrollTop + scrollport.clientHeight / 2 - expectedY) < 2,
    `zoom preserves the reading point across fixed page gaps without returning to the caret (expected ${expectedY}, actual ${scrollport.scrollTop + scrollport.clientHeight / 2}, height ${scrollport.scrollHeight})`);
  assert(editor.state === before, "zoom preserves exact editor state object, selection and history");
  assert(scrollport.scrollWidth > scrollport.clientWidth, "200 percent TXT creates a real horizontal page extent");
  const pan = Math.min(180, scrollport.scrollWidth - scrollport.clientWidth);
  scrollport.scrollLeft = pan; await frame();
  assert(scrollport.scrollLeft === pan && Math.abs(canvas.getBoundingClientRect().left - scrollport.getBoundingClientRect().left) < 3,
    "TXT Canvas stays pinned while the right side of an enlarged page scrolls into view");
  const ordinary = wheel(scrollport);
  const altGraph = wheel(scrollport, { ctrlKey: true, altKey: true });
  assert(!ordinary.defaultPrevented && !altGraph.defaultPrevented && editor.getZoom() === 2,
    "ordinary wheel and AltGr wheel leave document zoom unchanged");
  scrollport.scrollLeft = 100;
  const oldScrollX = scrollport.scrollLeft;
  const rect = scrollport.getBoundingClientRect();
  const pointerY = scrollport.scrollTop + 120;
  const pointerPage = Math.floor(pointerY / (841.89 * 2 + 16));
  const pointerOffset = (pointerY - pointerPage * (841.89 * 2 + 16)) / 2;
  const controlled = wheel(scrollport, { ctrlKey: true });
  const nextZoom = editor.getZoom();
  assert(controlled.defaultPrevented && nextZoom > 2, "Ctrl wheel zooms the actual editor and suppresses browser zoom");
  assert(Math.abs(scrollport.scrollLeft - ((oldScrollX + 150) * nextZoom / 2 - 150)) < 2
      && Math.abs(scrollport.scrollTop + 120 - (pointerPage * (841.89 * nextZoom + 16) + pointerOffset * nextZoom)) < 2,
    "Ctrl wheel preserves the document point under the pointer on both axes");
  assert(rect.width > 0, "wheel is exercised on a visible browser viewport");
  for (const invalid of [NaN, 0.09, 5.01]) {
    let threw = false;
    try { editor.setZoom(invalid); } catch (error) { threw = error instanceof RangeError; }
    assert(threw && editor.getZoom() === nextZoom, "invalid zoom is rejected atomically: " + String(invalid));
  }
  editor.setZoom(0.1); assert(editor.getZoom() === 0.1, "10 percent is supported by real editor rendering");
  editor.setZoom(5); assert(editor.getZoom() === 5, "500 percent is supported by real editor rendering");
  editor.fitZoom("pages");
  assert(editor.getZoomMode() === "pages" && 2 * 841.89 * editor.getZoom() + 16 <= scrollport.clientHeight + 1,
    "two-page mode fits two vertically stacked A4 pages including their fixed gap");
  editor.fitZoom("width");
  await frame();
  assert(Math.abs(595.276 * editor.getZoom() - scrollport.clientWidth) < 2, "width mode uses the actual scrollport width");
  const beforeResize = editor.getZoom();
  const originalWidth = scrollport.style.width;
  scrollport.style.width = "400px";
  await waitFor(() => editor.getZoom() < beforeResize, "container ResizeObserver refits width without a window resize");
  assert(Math.abs(595.276 * editor.getZoom() - scrollport.clientWidth) < 2, "container resize refits document zoom");
  scrollport.style.width = originalWidth;
  editor.setZoom(2);
  key(editor.element, "Home", { ctrlKey: true });
  await waitFor(() => editor.state.focus === 0, "zoomed caret document start");
  key(editor.element, "ArrowRight");
  key(editor.element, "ArrowRight");
  await waitFor(() => editor.state.focus === 2, "zoomed caret at byte two");
  const caret = editor.state.caret;
  const bounds = canvas.getBoundingClientRect();
  pointer("pointerdown", bounds.left + caret.x * 2 - scrollport.scrollLeft + 0.2,
    bounds.top + caret.y * 2 - scrollport.scrollTop + caret.height);
  pointer("pointerup", bounds.left + caret.x * 2 - scrollport.scrollLeft + 0.2,
    bounds.top + caret.y * 2 - scrollport.scrollTop + caret.height);
  await waitFor(() => editor.state.focus === 2 && editor.state.anchor === 2, "zoomed click maps back to document byte offset");
  assert(editor.state.focus === 2, "zoomed hit-testing retains UTF-8 model coordinates");
  const text = editor.state.text;
  inputText(editor.element, "Z");
  await waitFor(() => editor.state.text !== text, "edit after zoom");
  await editor.undo();
  assert(editor.state.text === text && editor.getZoom() === 2, "undo restores text after zoom without changing its factor");
}

export async function checkAppZoom({ app, scrollport, assert, waitFor }) {
  const range = document.querySelector("[data-zoom-range]");
  const percent = document.querySelector("[data-zoom-percent]");
  const dialog = document.querySelector("[data-zoom-dialog]");
  const value = dialog.querySelector("[data-zoom-value]");
  const apply = () => dialog.querySelector("form").dispatchEvent(new SubmitEvent("submit", { bubbles: true, cancelable: true }));
  assert(!range.disabled && percent.textContent === "100\u202f%", "status zoom controls initialize from the real document view");
  range.value = "250"; range.dispatchEvent(new Event("input", { bubbles: true }));
  assert(app.editor.getZoom() === 2.5 && percent.textContent === "250\u202f%", "status slider changes Canvas factor and displayed percentage together");
  percent.focus(); percent.click();
  assert(dialog.open && document.activeElement === value, "zoom percentage opens a named modal with focused number field");
  value.value = "501"; apply();
  assert(dialog.open && value.getAttribute("aria-invalid") === "true" && app.editor.getZoom() === 2.5,
    "invalid dialog percentage remains visible without changing the document");
  const behindModal = wheel(scrollport, { ctrlKey: true });
  assert(!behindModal.defaultPrevented && app.editor.getZoom() === 2.5, "document wheel zoom is blocked behind a modal");
  value.value = "125";
  dialog.querySelector('input[value="custom"]').checked = true; apply();
  assert(!dialog.open && app.editor.getZoom() === 1.25 && percent.textContent === "125\u202f%" && document.activeElement === percent,
    "valid dialog submission applies once and restores the trigger focus");
  percent.click(); value.value = "450";
  dialog.querySelector(".zoom-dialog-cancel").click();
  assert(!dialog.open && app.editor.getZoom() === 1.25, "dialog cancellation does not apply its uncommitted percentage");
  app.ribbon.selectTab("view");
  document.querySelector("#zoom-command").click();
  assert(dialog.open, "Affichage ribbon opens the same zoom dialog");
  dialog.querySelector('input[value="page"]').checked = true; apply();
  assert(app.editor.getZoomMode() === "page" && app.editor.getZoom() * 841.89 <= scrollport.clientHeight + 1,
    "whole-page dialogue mode follows the real Canvas viewport");
  percent.click(); dialog.querySelector('input[value="width"]').checked = true; apply();
  await new Promise(resolve => setTimeout(resolve, 30));
  percent.click(); dialog.querySelector('input[value="pages"]').checked = true; apply();
  assert(!dialog.open && app.editor.getZoomMode() === "pages",
    "fractional fit percentage can switch directly to the two-page mode");
  app.editor.setZoom(1.5);
  await waitFor(() => percent.textContent === "150\u202f%", "external view changes synchronize the status controls");
  app.ribbon.selectTab("file");
}
