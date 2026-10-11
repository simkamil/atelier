// SPDX-License-Identifier: AGPL-3.0-or-later

const clamp = (value, min, max) => Math.max(min, Math.min(max, value));
const finite = value => typeof value === "number" && Number.isFinite(value);

function normalizeGeometry(value) {
  if (!value || !Array.isArray(value.pages) || value.pages.length === 0 ||
      !finite(value.zoom) || value.zoom <= 0 || !finite(value.pageGap) || value.pageGap < 0) return null;
  const pages = value.pages.map(page => {
    if (!page || ![page.minX, page.minY, page.width, page.height].every(finite) ||
        page.width <= 0 || page.height <= 0) return null;
    return page;
  });
  return pages.some(page => page === null) ? null : { ...value, pages };
}

/**
 * Attach pointer selection behavior to a Canvas and its scrollport. Coordinates
 * passed to selectPoint are page-space points; CSS pixels and DPR stay at the
 * viewport boundary.
 */
export function attachMouseSelection({ canvas, scrollport, getGeometry, getSelection,
  selectPoint, focus = () => {}, isEnabled = () => true, onError = () => {} } = {}) {
  if (!canvas?.addEventListener || !scrollport?.addEventListener ||
      ![getGeometry, getSelection, selectPoint, focus, isEnabled, onError].every(fn => typeof fn === "function")) {
    throw new TypeError("mouse selection requires a Canvas, scrollport, and callbacks");
  }

  let active = null;
  let disposed = false;
  const raf = globalThis.requestAnimationFrame?.bind(globalThis) ?? (callback => setTimeout(callback, 16));
  const caf = globalThis.cancelAnimationFrame?.bind(globalThis) ?? clearTimeout;

  function report(error) {
    try { onError(error); } catch { /* Error reporting cannot strand capture or frames. */ }
  }

  function enabled() {
    try { return Boolean(isEnabled()); }
    catch (error) {
      cancelCurrent();
      report(error);
      return false;
    }
  }

  function cancelCurrent() {
    const drag = active;
    if (!drag) return;
    active = null;
    drag.cancelled = true;
    if (drag.frame) caf(drag.frame);
    drag.frame = 0;
    try {
      if (canvas.hasPointerCapture?.(drag.pointerId)) canvas.releasePointerCapture(drag.pointerId);
    } catch { /* Capture may already have been released by the browser. */ }
  }

  function pointAt(clientX, clientY) {
    const geometry = normalizeGeometry(getGeometry());
    if (!geometry || !finite(clientX) || !finite(clientY)) return null;
    const rect = canvas.getBoundingClientRect();
    const sx = Number.isFinite(scrollport.scrollLeft) ? scrollport.scrollLeft : 0;
    const sy = Number.isFinite(scrollport.scrollTop) ? scrollport.scrollTop : 0;
    const localX = clientX - rect.left;
    const contentY = clientY - rect.top + sy;
    let pageIndex = 0;
    let top = 0;
    let bestDistance = Infinity;
    let pageTop = 0;
    for (let index = 0; index < geometry.pages.length; index += 1) {
      const page = geometry.pages[index];
      const height = page.height * geometry.zoom;
      const bottom = top + height;
      if (contentY >= top && contentY <= bottom) {
        pageIndex = index;
        pageTop = top;
        bestDistance = -1;
        break;
      }
      const distance = contentY < top ? top - contentY : contentY - bottom;
      if (distance < bestDistance) {
        bestDistance = distance;
        pageIndex = index;
        pageTop = top;
      }
      top = bottom + geometry.pageGap;
    }
    const page = geometry.pages[pageIndex];
    const yInPage = clamp((contentY - pageTop) / geometry.zoom, 0, page.height);
    const xInPage = clamp((localX + sx) / geometry.zoom, 0, page.width);
    return {
      page: pageIndex,
      x: page.minX + xInPage,
      y: page.minY + yInPage,
    };
  }

  function queuePoint(drag, clientX, clientY) {
    const point = pointAt(clientX, clientY);
    if (point) drag.latest = point;
    if (drag.initialPending || !drag.latest || drag.cancelled || active !== drag) return;
    if (!drag.pumping) void pump(drag);
  }

  async function pump(drag) {
    if (drag.pumping || active !== drag || drag.cancelled) return;
    if (!enabled()) { cancelCurrent(); return; }
    drag.pumping = true;
    try {
      while (active === drag && !drag.cancelled) {
        if (!enabled()) { cancelCurrent(); return; }
        let point;
        let initial = false;
        if (drag.initialPending) {
          point = drag.initialPending;
          drag.initialPending = null;
          initial = true;
        } else if (drag.latest) {
          point = drag.latest;
          drag.latest = null;
        } else break;

        const values = {
          ...point,
          unit: drag.unit,
          extend: initial ? drag.extendInitial : true,
          anchor: initial ? drag.initialAnchor : drag.baseAnchor,
          anchorEnd: initial ? drag.initialAnchorEnd : drag.baseAnchorEnd,
        };
        const result = await selectPoint(values);
        if (active !== drag || drag.cancelled) return;
        if (!enabled()) { cancelCurrent(); return; }
        if (!result || !Number.isInteger(result.anchor) || !Number.isInteger(result.focus)) {
          throw new TypeError("selectPoint must resolve to a selection");
        }
        if (initial) {
          if (!drag.extendInitial || drag.unit !== "character") {
            drag.baseAnchor = drag.unit === "character" ? result.focus : Math.min(result.anchor, result.focus);
            drag.baseAnchorEnd = drag.unit === "character" ? drag.baseAnchor : Math.max(result.anchor, result.focus);
            drag.initialAnchor = drag.baseAnchor;
            drag.initialAnchorEnd = drag.baseAnchorEnd;
          }
        }
      }
    } catch (error) {
      if (active === drag) {
        cancelCurrent();
        report(error);
      }
    } finally {
      drag.pumping = false;
      if (active === drag && !drag.cancelled && !enabled()) cancelCurrent();
      if (active === drag && !drag.cancelled && drag.latest && !drag.initialPending) void pump(drag);
      if (active === drag && drag.ended && !drag.pumping && !drag.latest && !drag.initialPending) finish(drag);
    }
  }

  function finish(drag) {
    if (active !== drag) return;
    active = null;
    if (drag.frame) caf(drag.frame);
    drag.frame = 0;
    try {
      if (canvas.hasPointerCapture?.(drag.pointerId)) canvas.releasePointerCapture(drag.pointerId);
    } catch { /* Browser may have dropped capture during pointerup. */ }
    try { focus(); } catch (error) { report(error); }
  }

  function edgeDirection(position, extent) {
    const edge = Math.min(32, Math.max(16, extent * 0.08));
    if (position < edge) return -clamp((edge - position) / edge, 0, 1);
    if (position > extent - edge) return clamp((position - (extent - edge)) / edge, 0, 1);
    return 0;
  }

  function scheduleAutoScroll(drag) {
    if (drag.frame || active !== drag || drag.ended || drag.cancelled) return;
    drag.frame = raf(() => {
      drag.frame = 0;
      if (active !== drag || drag.cancelled || drag.ended) return;
      if (!enabled()) { cancelCurrent(); return; }
      const rect = scrollport.getBoundingClientRect?.() ?? canvas.getBoundingClientRect();
      const x = drag.clientX - rect.left;
      const y = drag.clientY - rect.top;
      const dx = edgeDirection(x, scrollport.clientWidth || rect.width || 0);
      const dy = edgeDirection(y, scrollport.clientHeight || rect.height || 0);
      const maxX = Math.max(0, (scrollport.scrollWidth || 0) - (scrollport.clientWidth || 0));
      const maxY = Math.max(0, (scrollport.scrollHeight || 0) - (scrollport.clientHeight || 0));
      const beforeX = scrollport.scrollLeft || 0;
      const beforeY = scrollport.scrollTop || 0;
      scrollport.scrollLeft = clamp(beforeX + dx * 18, 0, maxX);
      scrollport.scrollTop = clamp(beforeY + dy * 18, 0, maxY);
      const changed = scrollport.scrollLeft !== beforeX || scrollport.scrollTop !== beforeY;
      if (changed) {
        queuePoint(drag, drag.clientX, drag.clientY);
      }
      if (changed && (dx !== 0 || dy !== 0)) scheduleAutoScroll(drag);
    });
  }

  function pointerDown(event) {
    if (disposed || active || event.button !== 0 || event.ctrlKey || event.altKey || event.metaKey || !enabled()) return;
    const location = pointAt(event.clientX, event.clientY);
    if (!location) return;
    event.preventDefault?.();
    const snapshot = getSelection() ?? { anchor: 0, focus: 0 };
    const detail = Number.isInteger(event.detail) ? event.detail : 1;
    const unit = detail >= 3 ? "paragraph" : detail === 2 ? "word" : "character";
    const shift = Boolean(event.shiftKey);
    const existingAnchor = Number.isInteger(snapshot.anchor) ? snapshot.anchor : 0;
    const existingFocus = Number.isInteger(snapshot.focus) ? snapshot.focus : existingAnchor;
    const drag = {
      pointerId: event.pointerId,
      unit,
      extendInitial: shift,
      initialAnchor: shift ? (unit === "character" ? existingAnchor : Math.min(existingAnchor, existingFocus)) : 0,
      initialAnchorEnd: shift ? (unit === "character" ? existingAnchor : Math.max(existingAnchor, existingFocus)) : 0,
      baseAnchor: shift ? (unit === "character" ? existingAnchor : Math.min(existingAnchor, existingFocus)) : 0,
      baseAnchorEnd: shift ? (unit === "character" ? existingAnchor : Math.max(existingAnchor, existingFocus)) : 0,
      initialPending: location,
      latest: null,
      clientX: event.clientX,
      clientY: event.clientY,
      pumping: false,
      ended: false,
      cancelled: false,
      frame: 0,
    };
    active = drag;
    try { canvas.setPointerCapture?.(drag.pointerId); }
    catch (error) { cancelCurrent(); report(error); return; }
    void pump(drag);
    scheduleAutoScroll(drag);
  }

  function pointerMove(event) {
    const drag = active;
    if (!drag || event.pointerId !== drag.pointerId || drag.cancelled) return;
    if (!enabled()) { cancelCurrent(); return; }
    event.preventDefault?.();
    drag.clientX = event.clientX;
    drag.clientY = event.clientY;
    queuePoint(drag, event.clientX, event.clientY);
    scheduleAutoScroll(drag);
  }

  function pointerUp(event) {
    const drag = active;
    if (!drag || event.pointerId !== drag.pointerId || drag.cancelled) return;
    drag.clientX = event.clientX;
    drag.clientY = event.clientY;
    drag.ended = true;
    if (drag.frame) caf(drag.frame);
    drag.frame = 0;
    queuePoint(drag, event.clientX, event.clientY);
    if (!drag.pumping && !drag.initialPending && !drag.latest) finish(drag);
  }

  function pointerCancel(event) {
    if (active && (event.pointerId === undefined || event.pointerId === active.pointerId)) cancelCurrent();
  }
  function lostPointerCapture(event) {
    // Browsers may implicitly release capture as part of pointerup. That event
    // can arrive before the queued final selection has settled.
    if (active?.ended && (event.pointerId === undefined || event.pointerId === active.pointerId)) return;
    pointerCancel(event);
  }
  function blur() { cancelCurrent(); }
  function scroll() {
    const drag = active;
    if (drag && !drag.ended) queuePoint(drag, drag.clientX, drag.clientY);
  }

  canvas.addEventListener("pointerdown", pointerDown);
  canvas.addEventListener("pointermove", pointerMove);
  canvas.addEventListener("pointerup", pointerUp);
  canvas.addEventListener("pointercancel", pointerCancel);
  canvas.addEventListener("lostpointercapture", lostPointerCapture);
  scrollport.addEventListener("scroll", scroll, { passive: true });
  globalThis.addEventListener?.("blur", blur);

  return Object.freeze({
    cancel: cancelCurrent,
    dispose() {
      if (disposed) return;
      disposed = true;
      cancelCurrent();
      canvas.removeEventListener("pointerdown", pointerDown);
      canvas.removeEventListener("pointermove", pointerMove);
      canvas.removeEventListener("pointerup", pointerUp);
      canvas.removeEventListener("pointercancel", pointerCancel);
      canvas.removeEventListener("lostpointercapture", lostPointerCapture);
      scrollport.removeEventListener("scroll", scroll);
      globalThis.removeEventListener?.("blur", blur);
    },
  });
}
