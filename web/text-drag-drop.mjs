// SPDX-License-Identifier: AGPL-3.0-or-later

const clamp = (value, min, max) => Math.max(min, Math.min(max, value));
const finite = value => typeof value === "number" && Number.isFinite(value);

function normalizeGeometry(value) {
  if (!value || !Array.isArray(value.pages) || value.pages.length === 0 ||
      !finite(value.zoom) || value.zoom <= 0 || !finite(value.pageGap) || value.pageGap < 0) return null;
  const pages = value.pages.map(page => page && [page.minX, page.minY, page.width, page.height].every(finite) &&
    page.width > 0 && page.height > 0 ? page : null);
  return pages.some(page => page === null) ? null : { ...value, pages };
}

function validSnapshot(value) {
  // `text: null` (v2): no text copy; the engine checks the boundaries.
  return value && (typeof value.text === "string" || value.text === null) && typeof value.revision === "bigint" &&
    Number.isInteger(value.anchor) && Number.isInteger(value.focus);
}

function validBoundary(text, offset) {
  if (!Number.isInteger(offset) || offset < 0) return false;
  if (text === null) return true;
  const bytes = new TextEncoder().encode(text);
  return offset <= bytes.length && (offset === bytes.length || (bytes[offset] & 0xc0) !== 0x80);
}

function validCaret(caret, allowNoCaret = false) {
  if (caret === null) return allowNoCaret; // v2 only: a probe gives a position and no caret (no drop marker)
  return caret && Number.isInteger(caret.page) && caret.page >= 0 &&
    [caret.x, caret.y, caret.height].every(finite) && caret.height >= 0;
}

/** Attach drag/drop and selected-text click handling to the editor Canvas. */
export function attachTextDragDrop({ canvas, scrollport, getGeometry, getSelectionSnapshot,
  isPointInSelection, probePoint, onDrop, onFilesDrop = null, onClick = () => {}, onDragState = () => {},
  isEnabled = () => true, focus = () => {}, onError = () => {}, allowNoCaret = false } = {}) {
  if (!canvas?.addEventListener || !scrollport?.addEventListener ||
      ![getGeometry, getSelectionSnapshot, isPointInSelection, probePoint, onDrop,
        onClick, onDragState, isEnabled, focus, onError].every(fn => typeof fn === "function")) {
    throw new TypeError("text drag/drop requires a Canvas, scrollport, and callbacks");
  }
  if (onFilesDrop !== null && typeof onFilesDrop !== "function") throw new TypeError("onFilesDrop must be a function");

  const raf = globalThis.requestAnimationFrame?.bind(globalThis) ?? (callback => setTimeout(callback, 16));
  const caf = globalThis.cancelAnimationFrame?.bind(globalThis) ?? clearTimeout;
  let gesture = null;
  let externalDrag = null;
  let disposed = false;
  let serial = 0;

  function report(error) { try { onError(error); } catch { /* reporting cannot strand capture */ } }
  function enabled() {
    try { return Boolean(isEnabled()); }
    catch (error) { cancel(); report(error); return false; }
  }
  function state(value) { try { onDragState(value); } catch (error) { report(error); } }

  function release(current) {
    if (current.frame) caf(current.frame);
    current.frame = 0;
    try {
      if (canvas.hasPointerCapture?.(current.pointerId)) canvas.releasePointerCapture(current.pointerId);
    } catch { /* the browser may already have released capture */ }
  }

  function cancel() {
    const current = gesture;
    clearExternal();
    // Also invalidate a drop probe that already cleared its drag marker.
    serial += 1;
    if (!current) return;
    gesture = null;
    current.cancelled = true;
    current.serial = ++serial;
    release(current);
    state(null);
  }

  function clearExternal() {
    if (!externalDrag) return;
    externalDrag.cancelled = true;
    externalDrag = null;
    serial += 1;
    state(null);
  }

  function pointAt(clientX, clientY) {
    const geometry = normalizeGeometry(getGeometry());
    if (!geometry || !finite(clientX) || !finite(clientY)) return null;
    const rect = canvas.getBoundingClientRect();
    const sx = Number.isFinite(scrollport.scrollLeft) ? scrollport.scrollLeft : 0;
    const sy = Number.isFinite(scrollport.scrollTop) ? scrollport.scrollTop : 0;
    const x = clientX - rect.left + sx;
    const y = clientY - rect.top + sy;
    let top = 0;
    let selectedPage = 0;
    let selectedTop = 0;
    let nearest = Infinity;
    for (let i = 0; i < geometry.pages.length; i += 1) {
      const page = geometry.pages[i];
      const height = page.height * geometry.zoom;
      const bottom = top + height;
      if (y >= top && y <= bottom) { selectedPage = i; selectedTop = top; nearest = -1; break; }
      const distance = y < top ? top - y : y - bottom;
      if (distance < nearest) { nearest = distance; selectedPage = i; selectedTop = top; }
      top = bottom + geometry.pageGap;
    }
    const page = geometry.pages[selectedPage];
    return { page: selectedPage,
      x: page.minX + clamp(x / geometry.zoom, 0, page.width),
      y: page.minY + clamp((y - selectedTop) / geometry.zoom, 0, page.height) };
  }

  async function probe(current, point, { marker = false } = {}) {
    if (!point || gesture !== current || current.cancelled || !enabled()) return null;
    const ticket = ++serial;
    current.serial = ticket;
    try {
      const result = await probePoint(point);
      if (gesture !== current || current.cancelled || current.serial !== ticket || !enabled()) return null;
      const snapshot = getSelectionSnapshot();
      if (!validSnapshot(snapshot) || snapshot.revision !== current.snapshot.revision ||
          snapshot.anchor !== current.snapshot.anchor || snapshot.focus !== current.snapshot.focus ||
          !result || result.revision !== current.snapshot.revision ||
          !validBoundary(current.snapshot.text, result.offset) || !validCaret(result.caret, allowNoCaret)) {
        throw new Error("EDITOR_STALE_EDIT");
      }
      if (marker && result.caret) state(result.caret);
      return result;
    } catch (error) {
      if (gesture === current && current.serial === ticket) {
        if (error?.message === "EDITOR_STALE_EDIT") report(error);
        else report(error);
        cancel();
      }
      return null;
    }
  }

  function consume(event) {
    event.preventDefault?.();
    event.stopImmediatePropagation?.();
  }

  function pointerDown(event) {
    if (disposed || gesture || event.button !== 0 || event.detail !== 1 || !enabled()) return;
    // Modifier-driven selection owns Shift/Alt/Meta gestures; Ctrl remains the
    // copy modifier and is sampled at pointerup by the drag controller.
    if (event.shiftKey || event.altKey || event.metaKey) return;
    let inside = false;
    try { inside = Boolean(isPointInSelection(event.clientX, event.clientY)); }
    catch (error) { report(error); return; }
    if (!inside) return;
    const snapshot = getSelectionSnapshot();
    if (!validSnapshot(snapshot) || snapshot.anchor === snapshot.focus ||
        !validBoundary(snapshot.text, snapshot.anchor) || !validBoundary(snapshot.text, snapshot.focus)) return;
    clearExternal();
    consume(event);
    const current = gesture = {
      pointerId: event.pointerId, snapshot: { ...snapshot }, startX: event.clientX, startY: event.clientY,
      clientX: event.clientX, clientY: event.clientY, moved: false, ended: false, cancelled: false,
      latest: null, pumping: false, frame: 0, serial: 0,
    };
    try { canvas.setPointerCapture?.(current.pointerId); }
    catch (error) { cancel(); report(error); }
  }

  function queueProbe(current, x, y) {
    current.serial = ++serial; // Invalidate an older async marker immediately.
    current.latest = { x, y };
    if (!current.pumping) void pump(current);
  }

  async function pump(current) {
    if (current.pumping || gesture !== current) return;
    current.pumping = true;
    try {
      while (gesture === current && current.latest) {
        const { x, y } = current.latest;
        current.latest = null;
        const result = await probe(current, pointAt(x, y), { marker: current.moved });
        if (gesture !== current) return;
        if (result) current.lastPoint = { x, y };
        if (current.ended && !current.latest) {
          if (result) await finish(current, result);
          else cancel();
        }
      }
    } finally {
      current.pumping = false;
      if (gesture === current && current.latest) void pump(current);
    }
  }

  function crossedThreshold(current, x, y) {
    return Math.hypot(x - current.startX, y - current.startY) >= 4;
  }

  function pointerMove(event) {
    const current = gesture;
    if (!current || current.pointerId !== event.pointerId || current.cancelled) return;
    consume(event);
    if (!enabled()) { cancel(); return; }
    current.clientX = event.clientX;
    current.clientY = event.clientY;
    if (crossedThreshold(current, event.clientX, event.clientY)) current.moved = true;
    if (current.moved) {
      queueProbe(current, event.clientX, event.clientY);
      scheduleAutoScroll(current);
    }
  }

  async function finish(current, probed = null) {
    if (gesture !== current || current.cancelled) return;
    if (!enabled()) { cancel(); return; }
    const point = pointAt(current.clientX, current.clientY);
    const result = probed && current.lastPoint?.x === current.clientX && current.lastPoint?.y === current.clientY
      ? probed : await probe(current, point, { marker: false });
    if (gesture !== current || current.cancelled) return;
    if (!result) { cancel(); return; }
    const { snapshot } = current;
    if (!current.moved) {
      gesture = null;
      release(current);
      state(null);
      try { await onClick(result); } catch (error) { report(error); }
      try { focus(); } catch (error) { report(error); }
      return;
    }
    const sourceStart = Math.min(snapshot.anchor, snapshot.focus);
    const sourceEnd = Math.max(snapshot.anchor, snapshot.focus);
    const target = result.offset;
    const kind = current.ctrlKey ? "copy" : "move";
    if (kind === "move" && target >= sourceStart && target <= sourceEnd) {
      gesture = null;
      release(current);
      state(null);
      try { focus(); } catch (error) { report(error); }
      return;
    }
    // Ctrl is intentionally read at pointerup, so modifier changes during the drag work.
    gesture = null;
    release(current);
    state(null);
    try {
      await onDrop({ kind, revision: snapshot.revision, sourceAnchor: snapshot.anchor,
        sourceFocus: snapshot.focus, target, text: snapshot.text });
    } catch (error) { report(error); }
    try { focus(); } catch (error) { report(error); }
  }

  function pointerUp(event) {
    const current = gesture;
    if (!current || current.pointerId !== event.pointerId || current.cancelled) return;
    consume(event);
    current.clientX = event.clientX;
    current.clientY = event.clientY;
    current.ctrlKey = Boolean(event.ctrlKey);
    if (crossedThreshold(current, event.clientX, event.clientY)) current.moved = true;
    current.ended = true;
    if (current.frame) caf(current.frame);
    current.frame = 0;
    // Always replace any queued point with the exact pointerup location.
    current.latest = { x: event.clientX, y: event.clientY };
    if (!current.pumping) void pump(current);
  }

  function pointerCancel(event) {
    if (gesture && (event.pointerId === undefined || gesture.pointerId === event.pointerId)) cancel();
  }
  function lostPointerCapture(event) {
    if (gesture?.ended && (event.pointerId === undefined || gesture.pointerId === event.pointerId)) return;
    pointerCancel(event);
  }
  function keyDown(event) { if (event.key === "Escape") cancel(); }
  function blur() { cancel(); }

  function edgeDirection(position, extent) {
    const edge = Math.min(32, Math.max(16, extent * 0.08));
    if (position < edge) return -clamp((edge - position) / edge, 0, 1);
    if (position > extent - edge) return clamp((position - (extent - edge)) / edge, 0, 1);
    return 0;
  }

  function scheduleAutoScroll(current) {
    if (current.frame || gesture !== current || current.ended) return;
    current.frame = raf(() => {
      current.frame = 0;
      if (gesture !== current || current.ended || !enabled()) { if (gesture === current) cancel(); return; }
      const rect = scrollport.getBoundingClientRect?.() ?? canvas.getBoundingClientRect();
      const dx = edgeDirection(current.clientX - rect.left, scrollport.clientWidth || rect.width || 0);
      const dy = edgeDirection(current.clientY - rect.top, scrollport.clientHeight || rect.height || 0);
      const maxX = Math.max(0, (scrollport.scrollWidth || 0) - (scrollport.clientWidth || 0));
      const maxY = Math.max(0, (scrollport.scrollHeight || 0) - (scrollport.clientHeight || 0));
      const beforeX = scrollport.scrollLeft || 0;
      const beforeY = scrollport.scrollTop || 0;
      scrollport.scrollLeft = clamp(beforeX + dx * 18, 0, maxX);
      scrollport.scrollTop = clamp(beforeY + dy * 18, 0, maxY);
      if (scrollport.scrollLeft !== beforeX || scrollport.scrollTop !== beforeY) {
        queueProbe(current, current.clientX, current.clientY);
        scheduleAutoScroll(current);
      }
    });
  }

  function hasPlainText(event) {
    const types = event.dataTransfer?.types;
    if (!types) return false;
    try { return Array.from(types).includes("text/plain"); } catch { return false; }
  }
  // Files (pictures) dragged from the system: accepted only when the host handles them.
  function hasFiles(event) {
    if (!onFilesDrop) return false;
    try { return Array.from(event.dataTransfer?.types ?? []).includes("Files"); } catch { return false; }
  }
  function dragOver(event) {
    event.preventDefault?.();
    if (!hasPlainText(event) && !hasFiles(event)) { clearExternal(); return; }
    if (disposed || gesture || !enabled()) { clearExternal(); return; }
    const snapshot = getSelectionSnapshot();
    if (!validSnapshot(snapshot)) { clearExternal(); return; }
    if (!externalDrag || externalDrag.snapshot.revision !== snapshot.revision ||
        externalDrag.snapshot.anchor !== snapshot.anchor || externalDrag.snapshot.focus !== snapshot.focus) {
      clearExternal();
      externalDrag = { snapshot: { ...snapshot }, latest: null, pumping: false, cancelled: false, ticket: 0 };
    }
    externalDrag.latest = { x: event.clientX, y: event.clientY };
    externalDrag.ticket = ++serial;
    if (!externalDrag.pumping) void pumpExternal(externalDrag);
  }

  async function pumpExternal(current) {
    if (current.pumping || externalDrag !== current) return;
    current.pumping = true;
    try {
      while (externalDrag === current && current.latest) {
        const { x, y } = current.latest;
        current.latest = null;
        const point = pointAt(x, y);
        const ticket = current.ticket;
        if (!point) { state(null); continue; }
        try {
          const result = await probePoint(point);
          const snapshot = getSelectionSnapshot();
          if (externalDrag !== current || current.cancelled || current.ticket !== ticket || !enabled()) continue;
          if (!validSnapshot(snapshot) || snapshot.revision !== current.snapshot.revision ||
              snapshot.anchor !== current.snapshot.anchor || snapshot.focus !== current.snapshot.focus ||
              !result || result.revision !== current.snapshot.revision ||
              !validBoundary(current.snapshot.text, result.offset) || !validCaret(result.caret, allowNoCaret)) {
            throw new Error("EDITOR_STALE_EDIT");
          }
          if (result.caret) state(result.caret);
        } catch (error) {
          if (externalDrag === current && current.ticket === ticket) {
            report(error);
            clearExternal();
          }
        }
      }
    } finally {
      current.pumping = false;
      if (externalDrag === current && current.latest) void pumpExternal(current);
    }
  }

  function dragLeave() { clearExternal(); }

  async function drop(event) {
    // Always suppress browser navigation/HTML/file handling for plain-text drops,
    // including while busy or when the pointer cannot be mapped to a page.
    event.preventDefault?.();
    const fileDrop = !hasPlainText(event) && hasFiles(event);
    if (!hasPlainText(event) && !fileDrop) { clearExternal(); return; }
    clearExternal();
    if (disposed || gesture || !enabled()) return;
    // DataTransfer is protected once the native drop event returns to the browser.
    let text;
    let files = [];
    if (fileDrop) {
      try { files = Array.from(event.dataTransfer.files ?? []); }
      catch (error) { report(error); return; }
      if (files.length === 0) return;
    } else {
      try { text = event.dataTransfer.getData("text/plain"); }
      catch (error) { report(error); return; }
      if (typeof text !== "string" || text.length === 0) return;
    }
    const snapshot = getSelectionSnapshot();
    if (!validSnapshot(snapshot)) return;
    const point = pointAt(event.clientX, event.clientY);
    if (!point) return;
    const ticket = ++serial;
    try {
      const result = await probePoint(point);
      const now = getSelectionSnapshot();
      if (disposed || gesture || ticket !== serial || !enabled() || !validSnapshot(now) || now.revision !== snapshot.revision ||
          now.anchor !== snapshot.anchor || now.focus !== snapshot.focus || !result ||
          result.revision !== snapshot.revision || !validBoundary(snapshot.text, result.offset) || !validCaret(result.caret, allowNoCaret)) return;
      if (fileDrop) await onFilesDrop({ files, revision: snapshot.revision, target: result.offset });
      else await onDrop({ kind: "external", revision: snapshot.revision,
        sourceAnchor: snapshot.anchor, sourceFocus: snapshot.focus, target: result.offset, text });
      try { await focus(); } catch (error) { report(error); }
    } catch (error) { report(error); }
  }

  canvas.addEventListener("pointerdown", pointerDown, true);
  canvas.addEventListener("pointermove", pointerMove, true);
  canvas.addEventListener("pointerup", pointerUp, true);
  canvas.addEventListener("pointercancel", pointerCancel, true);
  canvas.addEventListener("lostpointercapture", lostPointerCapture, true);
  canvas.addEventListener("dragover", dragOver);
  canvas.addEventListener("dragleave", dragLeave);
  canvas.addEventListener("drop", drop);
  globalThis.addEventListener?.("keydown", keyDown, true);
  globalThis.addEventListener?.("blur", blur);

  return Object.freeze({
    cancel,
    dispose() {
      if (disposed) return;
      disposed = true;
      cancel();
      canvas.removeEventListener("pointerdown", pointerDown, true);
      canvas.removeEventListener("pointermove", pointerMove, true);
      canvas.removeEventListener("pointerup", pointerUp, true);
      canvas.removeEventListener("pointercancel", pointerCancel, true);
      canvas.removeEventListener("lostpointercapture", lostPointerCapture, true);
      canvas.removeEventListener("dragover", dragOver);
      canvas.removeEventListener("dragleave", dragLeave);
      canvas.removeEventListener("drop", drop);
      globalThis.removeEventListener?.("keydown", keyDown, true);
      globalThis.removeEventListener?.("blur", blur);
    },
  });
}
