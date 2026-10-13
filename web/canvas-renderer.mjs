// SPDX-License-Identifier: AGPL-3.0-or-later
import { decodeDisplayList } from "./display-list.mjs";

// `then(m, n)` applies m first and n second, matching Rust's affine contract.
const mul = (m, n) => ({
  a: n.a * m.a + n.c * m.b,
  b: n.b * m.a + n.d * m.b,
  c: n.a * m.c + n.c * m.d,
  d: n.b * m.c + n.d * m.d,
  e: n.a * m.e + n.c * m.f + n.e,
  f: n.b * m.e + n.d * m.f + n.f,
});
const map = (m, p) => ({
  x: m.a * p.x + m.c * p.y + m.e,
  y: m.b * p.x + m.d * p.y + m.f,
});
const colorCss = c =>
  `rgba(${Math.round(c.r * 255)},${Math.round(c.g * 255)},${Math.round(c.b * 255)},${c.a})`;
const intersects = (a, b) =>
  a.x <= b.x + b.width && a.x + a.width >= b.x &&
  a.y <= b.y + b.height && a.y + a.height >= b.y;
const rectOf = points => {
  let minX = Infinity, minY = Infinity;
  let maxX = -Infinity, maxY = -Infinity;
  for (const point of points) {
    minX = Math.min(minX, point.x);
    minY = Math.min(minY, point.y);
    maxX = Math.max(maxX, point.x);
    maxY = Math.max(maxY, point.y);
  }
  return { x: minX, y: minY, width: maxX - minX, height: maxY - minY };
};
function pathBounds(path, matrix) {
  const points = [];
  for (const element of path) {
    if (element.point) points.push(element.point);
    if (element.control) points.push(element.control);
    if (element.control1) points.push(element.control1);
    if (element.control2) points.push(element.control2);
    if (element.to) points.push(element.to);
  }
  return points.length ? rectOf(points.map(point => map(matrix, point))) : null;
}
function makePath(elements) {
  const p = new Path2D();
  for (const element of elements) {
    if (element.type === "moveTo") p.moveTo(element.point.x, element.point.y);
    else if (element.type === "lineTo") p.lineTo(element.point.x, element.point.y);
    else if (element.type === "quadraticTo") {
      p.quadraticCurveTo(element.control.x, element.control.y, element.to.x, element.to.y);
    } else if (element.type === "cubicTo") {
      p.bezierCurveTo(element.control1.x, element.control1.y,
        element.control2.x, element.control2.y, element.to.x, element.to.y);
    }
    else p.closePath();
  }
  return p;
}

/**
 * Canvas replay for page-space ATDL. Glyph resolvers return {path: Path2D,
 * bounds?: {x,y,width,height}} in page units relative to the glyph origin;
 * converting font units, shaping, and origin normalization belong to the
 * provider. Image resolvers return a CanvasImageSource. Both resolvers are
 * synchronous and never load resources themselves.
 */
export class CanvasRenderer {
  constructor(canvas, { resolveGlyph = () => null, resolveImage = () => null,
    maxGlyphCache = 4096, pageGap = 16, pageBackground = null, onDiagnostic = () => {} } = {}) {
    if (!canvas?.getContext) throw new TypeError("CanvasRenderer needs a canvas element");
    if (!Number.isInteger(maxGlyphCache) || maxGlyphCache < 0) throw new RangeError("invalid glyph cache size");
    if (!Number.isFinite(pageGap) || pageGap < 0) throw new RangeError("invalid page gap");
    if (pageBackground !== null && typeof pageBackground !== "string") throw new TypeError("invalid page background");
    if (typeof resolveGlyph !== "function" || typeof resolveImage !== "function" ||
        typeof onDiagnostic !== "function") {
      throw new TypeError("resource resolvers and diagnostic handler must be functions");
    }
    this.canvas = canvas;
    this.ctx = canvas.getContext("2d");
    if (!this.ctx) throw new Error("2D canvas context is unavailable");
    this.resolveGlyph = resolveGlyph;
    this.resolveImage = resolveImage;
    this.maxGlyphCache = maxGlyphCache;
    this.pageGap = pageGap;
    this.pageBackground = pageBackground;
    this.onDiagnostic = onDiagnostic;
    this.pages = [];
    this.overlay = [];
    this.editorOverlay = { selectionRects: [], caret: null, compositionRects: [], caretVisible: true };
    this.zoom = 1;
    this.scrollX = 0;
    this.scrollY = 0;
    this.width = canvas.clientWidth || canvas.width || 0;
    this.height = canvas.clientHeight || canvas.height || 0;
    this.dpr = globalThis.devicePixelRatio || 1;
    this.glyphCache = new Map();
    this.stats = this._emptyStats();
    this._resizeFrame = 0;
    this._resizeObserver = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(() => {
      if (this._resizeFrame) return;
      this._resizeFrame = requestAnimationFrame(() => {
        this._resizeFrame = 0;
        this.setViewport({ width: canvas.clientWidth, height: canvas.clientHeight,
          dpr: globalThis.devicePixelRatio || 1 });
      });
    });
    this._resizeObserver?.observe(canvas);
    this._onWindowResize = () => this.setViewport({
      width: canvas.clientWidth,
      height: canvas.clientHeight,
      dpr: globalThis.devicePixelRatio || 1,
    });
    globalThis.addEventListener?.("resize", this._onWindowResize);
  }
  _emptyStats() {
    return { pagesDrawn: 0, commandsDrawn: 0, commandsCulled: 0,
      glyphCacheHits: 0, glyphCacheMisses: 0, resourceErrors: 0 };
  }
  destroy() {
    this._resizeObserver?.disconnect();
    if (this._resizeFrame) cancelAnimationFrame(this._resizeFrame);
    this._resizeFrame = 0;
    globalThis.removeEventListener?.("resize", this._onWindowResize);
    this.glyphCache.clear();
  }
  setPages(pages) {
    this.pages = pages.map(page => page instanceof Uint8Array || page instanceof ArrayBuffer
      ? decodeDisplayList(page) : page);
    return this.render();
  }
  setViewport({ width = this.width, height = this.height, zoom = this.zoom,
    scrollX = this.scrollX, scrollY = this.scrollY, dpr = this.dpr } = {}) {
    const values = { width, height, zoom, scrollX, scrollY, dpr };
    for (const [name, value] of Object.entries(values)) {
      if (!Number.isFinite(value) || value < 0 || ((name === "zoom" || name === "dpr") && value === 0)) {
        throw new RangeError(`invalid ${name}`);
      }
    }
    const changed = Object.keys(values).some(key => this[key] !== values[key]);
    Object.assign(this, values);
    const backingWidth = Math.round(width * dpr);
    const backingHeight = Math.round(height * dpr);
    if (this.canvas.width !== backingWidth) this.canvas.width = backingWidth;
    if (this.canvas.height !== backingHeight) this.canvas.height = backingHeight;
    if (changed) return this.render();
    return { ...this.stats };
  }
  invalidateResources() {
    this.glyphCache.clear();
    return this.render();
  }
  setOverlay(rectangles) {
    if (!Array.isArray(rectangles)) throw new TypeError("overlay must be an array");
    this.overlay = rectangles.map(rect => {
      for (const key of ["x", "y", "width", "height"]) {
        if (!Number.isFinite(rect[key])) throw new TypeError("invalid overlay rectangle");
      }
      if (rect.width < 0 || rect.height < 0) throw new RangeError("invalid overlay dimensions");
      return { ...rect };
    });
    return this.render();
  }
  clearOverlay() {
    return this.setOverlay([]);
  }
  setEditorOverlay({ selectionRects = [], searchRects = [], activeSearchRects = [], caret = null, compositionRects = [], caretVisible = true } = {}) {
    const rects = (value, name) => {
      if (!Array.isArray(value)) throw new TypeError(`${name} must be an array`);
      return value.map(rect => {
        for (const key of ["x", "y", "width", "height"]) {
          if (!Number.isFinite(rect?.[key])) throw new TypeError(`invalid ${name} rectangle`);
        }
        if (rect.width < 0 || rect.height < 0) throw new RangeError(`invalid ${name} dimensions`);
        return { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
      });
    };
    if (typeof caretVisible !== "boolean") throw new TypeError("caretVisible must be boolean");
    const normalizedCaret = caret === null ? null : rects([caret], "caret")[0];
    this.editorOverlay = {
      selectionRects: rects(selectionRects, "selection"),
      searchRects: rects(searchRects, "search"),
      activeSearchRects: rects(activeSearchRects, "active search"),
      caret: normalizedCaret,
      compositionRects: rects(compositionRects, "composition"),
      caretVisible,
    };
    return this.render();
  }
  render(dirty = null) {
    const { ctx, width, height, dpr } = this;
    const clip = dirty || { x: 0, y: 0, width, height };
    for (const key of ["x", "y", "width", "height"]) {
      if (!Number.isFinite(clip[key])) throw new TypeError("invalid dirty rectangle");
    }
    if (clip.width < 0 || clip.height < 0) throw new RangeError("invalid dirty rectangle");
    ctx.save();
    try {
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(clip.x, clip.y, clip.width, clip.height);
      ctx.beginPath();
      ctx.rect(0, 0, width, height);
      ctx.clip();
      ctx.beginPath();
      ctx.rect(clip.x, clip.y, clip.width, clip.height);
      ctx.clip();
      this.stats = this._emptyStats();
      let top = -this.scrollY;
      for (const page of this.pages) {
        const bounds = page.page.bounds;
        const pageHeight = bounds.height * this.zoom;
        const visible = { x: -this.scrollX, y: top,
          width: bounds.width * this.zoom, height: pageHeight };
        const viewport = { x: 0, y: 0, width, height };
        if (width > 0 && height > 0 && intersects(visible, viewport) && intersects(visible, clip)) {
          this.stats.pagesDrawn++;
          if (this.pageBackground !== null) {
            ctx.fillStyle = this.pageBackground;
            ctx.fillRect(visible.x, visible.y, visible.width, visible.height);
          }
          this._drawPage(page, top, clip);
        }
        top += pageHeight + this.pageGap;
      }
      this._drawOverlay();
    } finally {
      ctx.restore();
    }
    return { ...this.stats };
  }
  _drawPage(page, top, dirty) {
    const { ctx } = this;
    const pageBounds = page.page.bounds;
    const base = {
      a: this.zoom * this.dpr,
      b: 0,
      c: 0,
      d: this.zoom * this.dpr,
      e: (-this.scrollX - pageBounds.min.x * this.zoom) * this.dpr,
      f: (top - pageBounds.min.y * this.zoom) * this.dpr,
    };
    const identity = { a: 1, b: 0, c: 0, d: 1, e: 0, f: 0 };
    const transforms = [identity];
    const clips = [];

    for (const command of page.commands) {
      if (command.type === "pushTransform") {
        transforms.push(mul(transforms.at(-1), command.transform));
        continue;
      }
      if (command.type === "popTransform") {
        transforms.pop();
        continue;
      }
      if (command.type === "pushClip") {
        clips.push(command.bounds);
        continue;
      }
      if (command.type === "popClip") {
        clips.pop();
        continue;
      }
      if (command.type === "link" || command.type === "structure") continue;

      const documentTransform = transforms.at(-1);
      const documentBounds = this._bounds(command, documentTransform);
      const screenBounds = documentBounds && {
        x: -this.scrollX + (documentBounds.x - pageBounds.min.x) * this.zoom,
        y: top + (documentBounds.y - pageBounds.min.y) * this.zoom,
        width: documentBounds.width * this.zoom,
        height: documentBounds.height * this.zoom,
      };
      if (screenBounds && !intersects(screenBounds, dirty)) {
        this.stats.commandsCulled++;
        continue;
      }

      ctx.save();
      try {
        // Clips use page coordinates and therefore see the page transform alone.
        ctx.setTransform(...this._matrix(base));
        for (const clip of clips) {
          ctx.beginPath();
          ctx.rect(clip.min.x, clip.min.y, clip.width, clip.height);
          ctx.clip();
        }
        // Apply the document transform first, then map the result to the viewport.
        const canvasTransform = mul(documentTransform, base);
        ctx.setTransform(...this._matrix(canvasTransform));
        this._paint(command, canvasTransform);
      } catch (error) {
        this.stats.resourceErrors++;
        try {
          this.onDiagnostic({ type: "canvas-resource", command, error });
        } catch {
          // A diagnostic callback cannot interrupt rendering or state cleanup.
        }
      } finally {
        ctx.restore();
        this.stats.commandsDrawn++;
      }
    }
  }
  _matrix(matrix) {
    return [matrix.a, matrix.b, matrix.c, matrix.d, matrix.e, matrix.f];
  }
  _bounds(command, matrix) {
    if (command.type === "fillPath" || command.type === "strokePath") {
      const bounds = pathBounds(command.path, matrix);
      if (bounds && command.type === "strokePath") {
        // Frobenius norm bounds the largest scale under rotation or shear.
        const norm = Math.hypot(matrix.a, matrix.b, matrix.c, matrix.d);
        const radius = command.width * command.miterLimit * norm / 2;
        bounds.x -= radius;
        bounds.y -= radius;
        bounds.width += 2 * radius;
        bounds.height += 2 * radius;
      }
      return bounds;
    }
    if (command.type === "drawImage") {
      const rect = command.bounds;
      return rectOf([
        map(matrix, rect.min),
        map(matrix, { x: rect.max.x, y: rect.min.y }),
        map(matrix, rect.max),
        map(matrix, { x: rect.min.x, y: rect.max.y }),
      ]);
    }
    // Font outlines may overhang either side of their advances and may descend
    // below the baseline. Until every provider supplies trustworthy bounds,
    // preserve glyphs during dirty-region replay instead of culling them.
    if (command.type === "drawGlyphRun") return null;
    return null;
  }
  _paint(command, matrix) {
    const { ctx } = this;
    if (command.type === "fillPath") {
      ctx.fillStyle = colorCss(command.color);
      ctx.fill(makePath(command.path), command.rule === "evenOdd" ? "evenodd" : "nonzero");
    } else if (command.type === "strokePath") {
      ctx.strokeStyle = colorCss(command.color);
      ctx.lineWidth = command.width;
      ctx.lineCap = command.cap;
      ctx.lineJoin = command.join;
      ctx.miterLimit = command.miterLimit;
      ctx.stroke(makePath(command.path));
    } else if (command.type === "drawImage") {
      const image = this.resolveImage(command.imageId);
      if (!image) throw new Error(`image resource ${command.imageId} is unavailable`);
      const bounds = command.bounds;
      ctx.drawImage(image, bounds.min.x, bounds.min.y, bounds.width, bounds.height);
    } else if (command.type === "drawGlyphRun") {
      for (const glyph of command.glyphs) {
        const key = `${command.fontId}:${glyph.glyphId}`;
        let outline = this.glyphCache.get(key);
        if (outline) {
          this.glyphCache.delete(key);
          this.glyphCache.set(key, outline);
          this.stats.glyphCacheHits++;
        } else {
          this.stats.glyphCacheMisses++;
          outline = this.resolveGlyph(command.fontId, glyph.glyphId);
          if (!outline?.path) throw new Error(`glyph resource ${key} is unavailable`);
          if (this.maxGlyphCache) {
            this.glyphCache.set(key, outline);
            while (this.glyphCache.size > this.maxGlyphCache) {
              this.glyphCache.delete(this.glyphCache.keys().next().value);
            }
          }
        }
        const origin = { a: 1, b: 0, c: 0, d: 1, e: glyph.position.x, f: glyph.position.y };
        const glyphMatrix = mul(origin, matrix);
        ctx.save();
        try {
          ctx.setTransform(...this._matrix(glyphMatrix));
          ctx.fillStyle = colorCss(command.color);
          ctx.fill(outline.path);
        } finally {
          ctx.restore();
        }
      }
    }
  }
  _drawOverlay() {
    const { ctx } = this;
    for (const rect of this.overlay) {
      ctx.save();
      try {
        ctx.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);
        ctx.strokeStyle = rect.color || "#1683ff";
        ctx.lineWidth = rect.lineWidth || 1;
        ctx.beginPath();
        ctx.moveTo(rect.x, rect.y + rect.height);
        ctx.lineTo(rect.x + rect.width, rect.y + rect.height);
        ctx.stroke();
      } finally {
        ctx.restore();
      }
    }
    const editor = this.editorOverlay;
    ctx.save();
    try {
      ctx.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);
      ctx.fillStyle = "rgba(255,205,54,0.38)";
      for (const rect of editor.searchRects ?? []) ctx.fillRect(rect.x, rect.y, rect.width, rect.height);
      ctx.fillStyle = "rgba(244,139,42,0.44)";
      for (const rect of editor.activeSearchRects ?? []) ctx.fillRect(rect.x, rect.y, rect.width, rect.height);
      ctx.fillStyle = "rgba(35,108,87,0.24)";
      for (const rect of editor.selectionRects) ctx.fillRect(rect.x, rect.y, rect.width, rect.height);
      ctx.strokeStyle = "#236c57";
      ctx.lineWidth = 1;
      for (const rect of editor.compositionRects) {
        ctx.beginPath();
        ctx.moveTo(rect.x, rect.y + rect.height);
        ctx.lineTo(rect.x + rect.width, rect.y + rect.height);
        ctx.stroke();
      }
      if (editor.caretVisible && editor.caret) {
        ctx.fillStyle = "#172a2a";
        ctx.fillRect(editor.caret.x, editor.caret.y, Math.max(1, editor.caret.width), editor.caret.height);
      }
    } finally { ctx.restore(); }
  }
}
