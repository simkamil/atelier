// SPDX-License-Identifier: AGPL-3.0-or-later
import assert from "node:assert/strict";
import { decodeEditorResponse, encodeEditorCommand, EditorOpcode } from "../web/editor-protocol.mjs";

const encoder = new TextEncoder();
const viewOf = bytes => new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
function searchResponse({ excerpt = "éx", matchStart = 0, matchEnd = 3, x = 54, trailing = false } = {}) {
  const excerptBytes = encoder.encode(excerpt);
  const bytes = new Uint8Array(24 + 24 + excerptBytes.length + 4 + 36 + (trailing ? 1 : 0));
  bytes.set(encoder.encode("ATQF"));
  const view = viewOf(bytes);
  view.setUint16(4, 1, true); view.setBigUint64(8, 9n, true); view.setUint32(16, 1, true);
  view.setUint32(24, 4, true); view.setUint32(28, 7, true);
  view.setUint32(32, excerptBytes.length, true); view.setUint32(36, matchStart, true); view.setUint32(40, matchEnd, true);
  bytes.set(excerptBytes, 48);
  const rectOffset = 48 + excerptBytes.length;
  view.setUint32(rectOffset, 1, true); view.setUint32(rectOffset + 4, 0, true);
  view.setFloat64(rectOffset + 8, x, true); view.setFloat64(rectOffset + 16, 65, true);
  view.setFloat64(rectOffset + 24, 20, true); view.setFloat64(rectOffset + 32, 14, true);
  return bytes;
}

const searchCommand = encodeEditorCommand(EditorOpcode.Search, {
  revision: 7n, query: "é", options: { matchCase: true, ignoreAccents: true },
});
assert.equal(viewOf(searchCommand).getUint16(6, true), 19);
assert.equal(searchCommand.length, 26, "ATED header plus a 16-byte search prefix and UTF-8 query");
assert.equal(viewOf(searchCommand).getBigUint64(8, true), 7n);
assert.equal(searchCommand[16], 5, "case and accent flags");
assert.deepEqual([...searchCommand.subarray(24)], [0xc3, 0xa9]);
const statisticsCommand = encodeEditorCommand(EditorOpcode.Statistics, { revision: 7n });
assert.equal(viewOf(statisticsCommand).getUint16(6, true), 20);
assert.equal(statisticsCommand.length, 16);
assert.throws(() => encodeEditorCommand(EditorOpcode.Search, { revision: 1n, query: "x".repeat(1025) }), e => e.code === "MESSAGE_TOO_LARGE");

const decoded = decodeEditorResponse(0x8020, searchResponse());
assert.equal(decoded.kind, "search");
assert.equal(decoded.revision, 9n);
assert.equal(decoded.items[0].excerpt.text, "éx");
assert.deepEqual([decoded.items[0].excerpt.start, decoded.items[0].excerpt.end], [0, 3]);
assert.deepEqual(decoded.items[0].rects[0], { page: 0, x: 54, y: 65, width: 20, height: 14 });

for (const [name, payload] of [
  ["unknown response flags", (() => { const b = searchResponse(); b[20] = 0x80; return b; })()],
  ["invalid geometry", searchResponse({ x: -1 })],
  ["excerpt scalar boundary", searchResponse({ matchStart: 1, matchEnd: 3 })],
  ["trailing payload", searchResponse({ trailing: true })],
  ["invalid occurrence bounds", (() => { const b = searchResponse(); viewOf(b).setUint32(24, 8, true); return b; })()],
]) {
  assert.throws(() => decodeEditorResponse(0x8020, payload), e => e.code === "INVALID_RESPONSE" || e.code === "TRAILING_BYTES", name);
}

const stats = new Uint8Array(72);
stats.set(encoder.encode("ATQT"));
viewOf(stats).setUint16(4, 1, true); viewOf(stats).setBigUint64(8, 3n, true);
viewOf(stats).setUint32(16, 1, true); viewOf(stats).setUint32(20, 2, true);
viewOf(stats).setUint32(24, 4, true); viewOf(stats).setUint32(40, 3, true); viewOf(stats).setUint32(44, 1, true);
const decodedStats = decodeEditorResponse(0x8020, stats);
assert.equal(decodedStats.kind, "statistics");
assert.equal(decodedStats.document.words, 4);
assert.equal(decodedStats.document.lines, 3);
assert.equal(decodedStats.selection.words, 0);
assert.throws(() => decodeEditorResponse(0x8020, stats.slice(0, -1)), e => e.code === "INVALID_RESPONSE");
console.log("Navigation query protocol: command encoding, strict query envelopes, UTF-8 excerpts, geometry and statistics passed.");
