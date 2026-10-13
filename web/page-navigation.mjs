// SPDX-License-Identifier: AGPL-3.0-or-later

function geometry(pages, zoom, gap) {
  if (!Array.isArray(pages) || !Number.isFinite(zoom) || zoom <= 0 || !Number.isFinite(gap) || gap < 0) throw new TypeError("Invalid page geometry.");
  return pages.map(page => {
    const height = page?.page?.bounds?.height;
    if (!Number.isFinite(height) || height <= 0) throw new TypeError("Invalid page height.");
    return height * zoom;
  });
}

/** Page nearest the viewport center, in the same fixed-gap stack as Canvas. */
export function visiblePage(pages, { zoom, gap, scrollTop, height }) {
  const heights = geometry(pages, zoom, gap);
  if (!Number.isFinite(scrollTop) || !Number.isFinite(height) || height < 0) throw new TypeError("Invalid viewport.");
  if (!heights.length) return { page: 0, totalPages: 0 };
  const center = Math.max(0, scrollTop) + height / 2;
  let top = 0;
  for (let i = 0; i < heights.length; i++) {
    if (center <= top + heights[i] + gap / 2 || i === heights.length - 1) return { page: i + 1, totalPages: heights.length };
    top += heights[i] + gap;
  }
}

export function pageScrollTop(pages, page, zoom, gap) {
  const heights = geometry(pages, zoom, gap);
  if (!Number.isSafeInteger(page) || page < 1 || page > heights.length) throw new RangeError("Invalid page index.");
  return heights.slice(0, page - 1).reduce((top, height) => top + height + gap, 0);
}
