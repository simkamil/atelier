// SPDX-License-Identifier: AGPL-3.0-or-later
// Decoding of the staged-engine responses that still use the ATED v1 frame (R8): the failure code and the DOCX preview
// (`ATRP`, produced by `staged.rs` for kind 1). The plain-text editor commands (v1) were removed with the editor;
// the rich editor has its own protocol, `web/rich-editor-protocol.mjs`.
import { decodeDisplayList } from "./display-list.mjs";

export const EDITOR_LIMITS = Object.freeze({
  maxTextBytes: 128 * 1024,
  maxStateBytes: 32 * 1024 * 1024,
  maxDocxBytes: 8 * 1024 * 1024,
  maxPreviewPages: 256,
  maxPreviewDisplayBytes: 26 * 1024 * 1024,
  maxImageBytes: 8 * 1024 * 1024,
  maxImageAltBytes: 512,
});

const decoder = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });
const MAX_PREVIEW_GLYPHS = 131_072;
const MAX_PREVIEW_ATLAS_ENTRIES = 4096;

export class EditorProtocolError extends Error {
  constructor(code, message) { super(message); this.name = "EditorProtocolError"; this.code = code; }
}

function fail(code, message) { throw new EditorProtocolError(code, message); }

function decodePreviewAtlas(bytes) {
  let atlas;
  try { atlas = decodeDisplayList(bytes); }
  catch { fail("INVALID_RESPONSE", "Invalid preview glyph atlas."); }
  const bounds = atlas.page.bounds;
  if (Math.abs(bounds.width - 595.276) > 0.02 || Math.abs(bounds.height - 841.89) > 0.02
      || atlas.commands.length > MAX_PREVIEW_ATLAS_ENTRIES
      || atlas.commands.some(command => command.type !== "fillPath" || command.rule !== "nonZero"
        || command.color.r !== 0 || command.color.g !== 0 || command.color.b !== 0 || command.color.a !== 1)) {
    fail("INVALID_RESPONSE", "Invalid preview glyph atlas entries.");
  }
  return atlas.commands;
}
function isBytes(value) { return value instanceof Uint8Array; }
function ascii(bytes) {
  for (const byte of bytes) if (byte > 0x7f) fail("INVALID_RESPONSE", "Expected ASCII bytes.");
  return String.fromCharCode(...bytes);
}

export function decodeEditorResponse(type, payload) {
  if (!isBytes(payload) || payload.byteLength > EDITOR_LIMITS.maxStateBytes) fail("INVALID_RESPONSE", "Invalid or oversized editor response.");
  if (type === 0x8011) {
    if (payload.byteLength > 64) fail("INVALID_RESPONSE", "Editor failure code exceeds its bound.");
    const code = ascii(payload);
    if (!/^EDITOR_[A-Z0-9_]{1,56}$/.test(code)) fail("INVALID_RESPONSE", "Invalid editor failure code.");
    return { kind: "failed", code };
  }
  if (type !== 0x8020) fail("INVALID_RESPONSE", "Unexpected editor response type.");
  if (payload.byteLength < 8) fail("TRUNCATED", "Truncated editor response.");
  const view = new DataView(payload.buffer, payload.byteOffset, payload.byteLength);
  if (ascii(payload.subarray(0, 4)) === "ATRP") {
    if (payload.length < 20 || ![1, 2].includes(view.getUint16(4, true)) || view.getUint16(6, true) !== 0) fail("INVALID_RESPONSE", "Invalid preview header.");
    const version = view.getUint16(4, true);
    const count = view.getUint32(8, true), textLength = view.getUint32(12, true), noticeCount = view.getUint32(16, true);
    if (count < 1 || count > EDITOR_LIMITS.maxPreviewPages || textLength > EDITOR_LIMITS.maxTextBytes || 20 + textLength > payload.length) fail("INVALID_RESPONSE", "Preview limits exceeded.");
    let text;
    try { text = decoder.decode(payload.subarray(20, 20 + textLength)); } catch { fail("INVALID_RESPONSE", "Invalid preview UTF-8."); }
    let offset = 20 + textLength, total = 0, glyphAtlas = null, atlasEntries = null;
    if (version === 2) {
      if (offset + 4 > payload.length) fail("TRUNCATED", "Truncated preview glyph atlas length.");
      const atlasLength = view.getUint32(offset, true); offset += 4;
      total = atlasLength;
      if (atlasLength === 0 || total > EDITOR_LIMITS.maxPreviewDisplayBytes || offset + atlasLength > payload.length) {
        fail("INVALID_RESPONSE", "Invalid preview glyph atlas length.");
      }
      glyphAtlas = payload.slice(offset, offset + atlasLength);
      atlasEntries = decodePreviewAtlas(glyphAtlas);
      offset += atlasLength;
    }
    const pages = [];
    let totalGlyphs = 0;
    for (let i = 0; i < count; i += 1) {
      if (offset + 4 > payload.length) fail("TRUNCATED", "Truncated preview page.");
      const length = view.getUint32(offset, true); offset += 4; total += length;
      if (total > EDITOR_LIMITS.maxPreviewDisplayBytes || length === 0 || offset + length > payload.length) fail("INVALID_RESPONSE", "Invalid preview page length.");
      const bytes = payload.slice(offset, offset + length);
      const page = decodeDisplayList(bytes);
      if (Math.abs(page.page.bounds.width - 595.276) > 0.02 || Math.abs(page.page.bounds.height - 841.89) > 0.02) fail("INVALID_RESPONSE", "Invalid preview page dimensions.");
      if (version === 2) {
        for (const command of page.commands) {
          if (command.type !== "drawGlyphRun") continue;
          totalGlyphs += command.glyphs.length;
          if (totalGlyphs > MAX_PREVIEW_GLYPHS) fail("INVALID_RESPONSE", "Preview glyph limit exceeded.");
          if (command.fontId !== 0n || command.glyphs.some(glyph => glyph.glyphId >= atlasEntries.length)) {
            fail("INVALID_RESPONSE", "Preview page references a missing glyph atlas entry.");
          }
        }
      }
      pages.push(bytes); offset += length;
    }
    if (offset !== payload.length) fail("INVALID_RESPONSE", "Trailing preview data.");
    return { kind: "preview", pages, text, noticeCount, ...(version === 2 ? { glyphAtlas } : {}) };
  }
  fail("INVALID_RESPONSE", "Unexpected editor response payload.");
}
