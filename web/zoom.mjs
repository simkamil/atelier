// SPDX-License-Identifier: AGPL-3.0-or-later

export const MIN_ZOOM = 0.1;
export const MAX_ZOOM = 5;

export function clampZoom(value) {
  if (!Number.isFinite(value) || value < MIN_ZOOM || value > MAX_ZOOM) {
    throw new RangeError("zoom is outside the supported range");
  }
  return value;
}

export function fitZoom(mode, pageWidth, pageHeight, viewportWidth, viewportHeight, pageGap = 16) {
  if (!["width", "page", "pages"].includes(mode)) throw new TypeError("invalid zoom fit mode");
  if (![pageWidth, pageHeight, viewportWidth, viewportHeight, pageGap].every(Number.isFinite)
      || pageWidth <= 0 || pageHeight <= 0 || viewportWidth < 0 || viewportHeight < 0 || pageGap < 0) {
    throw new RangeError("invalid zoom fit geometry");
  }
  const widthFit = viewportWidth / pageWidth;
  const heightFit = mode === "page" ? viewportHeight / pageHeight
    : mode === "pages" ? Math.max(0, viewportHeight - pageGap) / (2 * pageHeight) : Infinity;
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, Math.min(widthFit, heightFit)));
}

export function zoomScrollPosition({ oldZoom, newZoom, scrollLeft, scrollTop,
  viewportLeft, viewportTop, clientX, clientY, newClientX = clientX, newClientY = clientY,
  oldClientLeft = 0, oldClientTop = 0, newClientLeft = oldClientLeft, newClientTop = oldClientTop,
  pageHeights = null, pageGap = 0 }) {
  const scale = newZoom / oldZoom;
  const oldY = scrollTop + clientY - viewportTop - oldClientTop;
  let newY = oldY * scale;
  if (Array.isArray(pageHeights) && pageHeights.length) {
    let oldTop = 0;
    let newTop = 0;
    let mapped = false;
    for (const height of pageHeights) {
      const oldBottom = oldTop + height * oldZoom;
      const newBottom = newTop + height * newZoom;
      if (oldY <= oldBottom) {
        newY = newTop + (oldY - oldTop) * scale;
        mapped = true;
        break;
      }
      if (oldY < oldBottom + pageGap) {
        newY = newBottom + oldY - oldBottom;
        mapped = true;
        break;
      }
      oldTop = oldBottom + pageGap;
      newTop = newBottom + pageGap;
    }
    if (!mapped) newY = newTop + (oldY - oldTop) * scale;
  }
  return {
    left: Math.max(0, (scrollLeft + clientX - viewportLeft - oldClientLeft) * scale
      - (newClientX - viewportLeft - newClientLeft)),
    top: Math.max(0, newY - (newClientY - viewportTop - newClientTop)),
  };
}

export function isZoomModalOpen(doc) {
  return Boolean(doc?.querySelector?.('dialog[open], [aria-modal="true"]'));
}

export function isAltGraphEvent(event) {
  return Boolean(event.getModifierState?.("AltGraph") || (event.ctrlKey && event.altKey));
}

export function wheelZoom(event, { active, modalOpen, getZoom, setZoom, scrollport }) {
  if (!event.ctrlKey || isAltGraphEvent(event) || !active || modalOpen) return false;
  event.preventDefault();
  const oldZoom = getZoom();
  const nextZoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, oldZoom * Math.exp(-event.deltaY * 0.002)));
  if (nextZoom !== oldZoom) setZoom(nextZoom, { x: event.clientX, y: event.clientY });
  return true;
}
