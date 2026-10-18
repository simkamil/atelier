// SPDX-License-Identifier: AGPL-3.0-or-later
// R2b: pages of the rich editor drawn through the glyph atlas (drawGlyphRun of font 0 + the Path2D
// of EditorPageCache.glyphResolver) against the drawing the engine used before the atlas (one
// FillPath per glyph, contour already moved to the glyph position). Both go through the same
// CanvasRenderer, at several zoom factors; the pixels must be identical (tolerance 0).
import { CanvasRenderer } from "/canvas-renderer.mjs";
import { decodeDisplayList } from "/display-list.mjs";
import { EditorPageCache } from "/editor-page-cache.mjs";
import { decodeRichReply } from "/rich-editor-protocol.mjs";

const fromBase64 = (text) => Uint8Array.from(atob(text), (c) => c.charCodeAt(0));
const result = document.getElementById("result");

/** The commands the engine wrote before the atlas: glyph -> FillPath at its position. */
function expand(commands, cache) {
  const out = [];
  for (const command of commands) {
    if (command.type !== "drawGlyphRun") { out.push(command); continue; }
    assertFont(command.fontId);
    for (const glyph of command.glyphs) {
      const entry = cache.glyphResolver(command.fontId, glyph.glyphId);
      if (!entry) throw new Error(`unknown atlas entry ${glyph.glyphId}`);
      const at = (p) => ({ x: glyph.position.x + p.x, y: glyph.position.y + p.y });
      out.push({
        type: "fillPath", rule: "nonZero", color: command.color,
        path: entry.elements.map((e) => {
          switch (e.type) {
            case "moveTo": case "lineTo": return { type: e.type, point: at(e.point) };
            case "quadraticTo": return { type: e.type, control: at(e.control), to: at(e.to) };
            case "cubicTo": return { type: e.type, control1: at(e.control1), control2: at(e.control2), to: at(e.to) };
            default: return { type: "close" };
          }
        }),
      });
    }
  }
  return out;
}
function assertFont(id) { if (id !== 0n) throw new Error("atlas glyph runs use font 0"); }

function draw(canvas, page, zoom, resolveGlyph) {
  const renderer = new CanvasRenderer(canvas, { pageBackground: "#ffffff", resolveGlyph, maxGlyphCache: 4096 });
  renderer.setViewport({ width: canvas.width, height: canvas.height, zoom, dpr: 1 });
  renderer.setPages([page]);
  const stats = { ...renderer.stats };
  renderer.destroy();
  return stats;
}

function compare(a, b) {
  const x = a.getContext("2d").getImageData(0, 0, a.width, a.height).data;
  const y = b.getContext("2d").getImageData(0, 0, b.width, b.height).data;
  let different = 0, max = 0, inked = 0;
  for (let i = 0; i < x.length; i += 4) {
    let d = 0;
    for (let k = 0; k < 4; k += 1) d = Math.max(d, Math.abs(x[i + k] - y[i + k]));
    if (d > 0) different += 1;
    if (d > max) max = d;
    if (x[i] < 250 || x[i + 1] < 250 || x[i + 2] < 250) inked += 1;
  }
  return { different, max, inked };
}

try {
  const captures = await (await fetch("/__captures")).json();
  const report = { status: "passed", pages: [], notes: [] };
  for (const capture of captures) {
    const cache = new EditorPageCache();
    // The product path: reply frames -> decoder -> cache (atlas, pages) -> resolver.
    for (const frame of capture.frames) cache.apply(decodeRichReply(fromBase64(frame)).response);
    if (cache.atlasSize === 0) throw new Error(`${capture.name}: no atlas`);
    for (const index of capture.pages) {
      const bytes = cache.page(index);
      if (!bytes) throw new Error(`${capture.name}: page ${index} missing`);
      const decoded = decodeDisplayList(bytes);
      if (decoded.commands.some((c) => c.type === "fillPath")) report.notes.push(`${capture.name} p${index} carries inline contours`);
      const inline = { ...decoded, commands: expand(decoded.commands, cache) };
      for (const zoom of [1, 1.5, 2]) {
        const stats = draw(document.getElementById("atlas"), decoded, zoom, cache.glyphResolver);
        draw(document.getElementById("inline"), inline, zoom, () => null);
        const diff = compare(document.getElementById("atlas"), document.getElementById("inline"));
        const glyphs = decoded.commands.reduce((n, c) => n + (c.type === "drawGlyphRun" ? c.glyphs.length : 0), 0);
        report.pages.push({ name: capture.name, page: index, zoom, glyphs, cacheHits: stats.glyphCacheHits, ...diff });
        if (diff.different !== 0) report.status = "failed";
        if (diff.inked < 100 && glyphs > 0) { report.status = "failed"; report.notes.push(`${capture.name} p${index}: blank render`); }
      }
    }
  }
  await fetch("/__atlas_result", { method: "POST", body: JSON.stringify(report) });
  result.textContent = report.status;
} catch (error) {
  await fetch("/__atlas_result", { method: "POST", body: JSON.stringify({ status: "error", message: String(error?.stack ?? error) }) });
  result.textContent = `error ${error}`;
}
