// SPDX-License-Identifier: AGPL-3.0-or-later
const frame = () => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
const wheel = (node, values = {}) => {
  const bounds = node.getBoundingClientRect();
  const event = new WheelEvent("wheel", { deltaY: -80, clientX: bounds.left + 150,
    clientY: bounds.top + 120, bubbles: true, cancelable: true, ...values });
  node.dispatchEvent(event);
  return event;
};

export async function checkEditorZoom({ editor, scrollport, canvas, openText, assert, waitFor, key, inputText, pointer, textOf = null }) {
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
  // v1 reads the text from the state; another engine passes `textOf` (and an asynchronous `waitFor`).
  const read = textOf ?? (async () => editor.state.text);
  const text = await read();
  inputText(editor.element, "Z");
  await waitFor(textOf ? async () => await read() !== text : () => editor.state.text !== text, "edit after zoom");
  await editor.undo();
  assert(await read() === text && editor.getZoom() === 2, "undo restores text after zoom without changing its factor");
}
