// SPDX-License-Identifier: AGPL-3.0-or-later
// Bitmaps of the pictures the v2 pages draw (R7b-1), by object identity (BigInt, the `imageId` of
// the `drawImage` commands). The engine sends them on request (`ImageBitmap`, EXIF orientation
// applied); this cache turns them into drawable sources for `CanvasRenderer.resolveImage`, bounded
// in bytes: when the bound is passed the least recently used pictures that the shown pages do not
// need (`retain`) are dropped, and asked for again if they come back on screen.
import { drawable } from "./image-insert.mjs";

export const MAX_RICH_IMAGE_CACHE_BYTES = 128 * 1024 * 1024;

/**
 * @param {{maxBytes?: number, draw?: (bitmap: {width: number, height: number, rgba: Uint8Array}) => Promise<object>}} [options]
 *   `draw` makes a drawable source from RGBA pixels (a `createImageBitmap` by default).
 */
export function createRichImageCache({ maxBytes = MAX_RICH_IMAGE_CACHE_BYTES, draw = drawable } = {}) {
  if (!Number.isInteger(maxBytes) || maxBytes < 1) throw new TypeError("maxBytes must be a positive integer.");
  const items = new Map(); // id (BigInt) -> {source, bytes}; insertion order is the order of use
  let retained = new Set();
  let total = 0;
  let generation = 0;
  const release = (entry) => { try { entry.source.close?.(); } catch { /* already released */ } };

  function evict(keep) {
    for (const [id, entry] of items) {
      if (total <= maxBytes) break;
      if (id === keep || retained.has(id)) continue;
      items.delete(id); total -= entry.bytes; release(entry);
    }
  }

  return Object.freeze({
    /** The drawable source of a picture, or null when it is not held. */
    get(id) {
      const entry = items.get(id);
      if (!entry) return null;
      items.delete(id); items.set(id, entry); // most recently used
      return entry.source;
    },
    has(id) { return items.has(id); },
    /** Holds a bitmap `{width, height, rgba}`; a result that arrives after `clear()` is dropped. */
    async add(id, bitmap) {
      if (typeof id !== "bigint") throw new TypeError("Picture identity must be BigInt.");
      const mine = generation;
      const source = await draw(bitmap);
      const entry = { source, bytes: bitmap.rgba.byteLength };
      if (mine !== generation) { release(entry); return false; }
      const old = items.get(id);
      if (old) { total -= old.bytes; release(old); items.delete(id); }
      items.set(id, entry);
      total += entry.bytes;
      evict(id);
      return true;
    },
    /** The pictures the shown pages draw: they are never evicted. */
    retain(ids) { retained = new Set(ids); },
    /** A new document or a new atlas epoch: forget everything (identities may be reused). */
    clear() {
      generation += 1;
      for (const entry of items.values()) release(entry);
      items.clear(); retained = new Set(); total = 0;
    },
    get size() { return items.size; },
    get bytes() { return total; },
  });
}
