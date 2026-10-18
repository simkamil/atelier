// SPDX-License-Identifier: AGPL-3.0-or-later
// Document properties of a DOCX (US-328): engine opcodes 26 (read) and 27 (apply).
// The wire format is described in crates/core-wasm/src/properties.rs.
import { decodeMessage, encodeMessage, MessageType } from "./wasm-protocol.mjs";

export const PropertiesOpcode = Object.freeze({ Read: 26, Apply: 27 });
/** Field order of the wire format; the six first-listed keys that the dialog edits are in `EDITABLE_FIELDS`. */
export const PROPERTY_FIELDS = Object.freeze(["title", "subject", "author", "manager", "company", "category", "keywords", "comments", "hyperlinkBase", "contentStatus", "language"]);
export const EDITABLE_FIELDS = Object.freeze(["title", "subject", "author", "keywords", "comments", "category"]);
export const MAX_DOCX_BYTES = 8 * 1024 * 1024;
const MAX_FIELD_BYTES = 64 * 1024;
const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });
// Ids of this module stay far from the editor's counter (which starts at 2).
let nextId = 1n << 40n;

export class PropertiesProtocolError extends Error {
  constructor(code, message) { super(message); this.name = "PropertiesProtocolError"; this.code = code; }
}
const fail = (code, message) => { throw new PropertiesProtocolError(code, message); };

function command(opcode, body) {
  const payload = new Uint8Array(8 + body.byteLength);
  payload.set([0x41, 0x54, 0x45, 0x44], 0);
  const view = new DataView(payload.buffer);
  view.setUint16(4, 1, true);
  view.setUint16(6, opcode, true);
  payload.set(body, 8);
  const requestId = nextId++;
  return { requestId, frame: encodeMessage({ type: MessageType.Editor, requestId, payload }) };
}

function checkDocx(bytes) {
  if (!(bytes instanceof Uint8Array) || bytes.byteLength > MAX_DOCX_BYTES) fail("INVALID_ARGUMENT", "docx must be a Uint8Array of at most 8 MiB.");
}

export function encodePropertiesRead(docx) {
  checkDocx(docx);
  return command(PropertiesOpcode.Read, docx);
}

/** `changes` maps property names (see PROPERTY_FIELDS) to their new text; an empty text removes the property. */
export function encodePropertiesApply(docx, changes) {
  checkDocx(docx);
  if (changes === null || typeof changes !== "object") fail("INVALID_ARGUMENT", "changes must be an object.");
  for (const name of Object.keys(changes)) if (!PROPERTY_FIELDS.includes(name)) fail("INVALID_ARGUMENT", `unknown property ${name}.`);
  const parts = [];
  const head = new Uint8Array(4);
  new DataView(head.buffer).setUint32(0, docx.byteLength, true);
  parts.push(head, docx);
  for (const name of PROPERTY_FIELDS) {
    const value = changes[name];
    if (value !== undefined && typeof value !== "string") fail("INVALID_ARGUMENT", `property ${name} must be a string.`);
    const bytes = value === undefined ? new Uint8Array(0) : encoder.encode(value);
    if (bytes.byteLength > MAX_FIELD_BYTES) fail("INVALID_ARGUMENT", `property ${name} is too large.`);
    const field = new Uint8Array(5 + bytes.byteLength);
    field[0] = value === undefined ? 0 : 1;
    new DataView(field.buffer).setUint32(1, bytes.byteLength, true);
    field.set(bytes, 5);
    parts.push(field);
  }
  const body = new Uint8Array(parts.reduce((sum, part) => sum + part.byteLength, 0));
  let offset = 0;
  for (const part of parts) { body.set(part, offset); offset += part.byteLength; }
  return command(PropertiesOpcode.Apply, body);
}

class Cursor {
  constructor(bytes) { this.bytes = bytes; this.view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength); this.at = 0; }
  need(n) { if (this.at + n > this.bytes.byteLength) fail("TRUNCATED", "Truncated properties response."); }
  u8() { this.need(1); return this.view.getUint8(this.at++); }
  u32() { this.need(4); const v = this.view.getUint32(this.at, true); this.at += 4; return v; }
  u64() { this.need(8); const v = this.view.getBigUint64(this.at, true); this.at += 8; return Number(v); }
  text() {
    const length = this.u32(); this.need(length);
    try { const text = decoder.decode(this.bytes.subarray(this.at, this.at + length)); this.at += length; return text; }
    catch { return fail("INVALID_RESPONSE", "Invalid properties UTF-8."); }
  }
}

function decodeRead(payload) {
  const c = new Cursor(payload);
  c.at = 8;
  const values = PROPERTY_FIELDS.map(() => c.text());
  const limits = PROPERTY_FIELDS.map(() => c.u32());
  const date = () => { const kind = c.u8(); const text = c.text(); return kind === 0 ? null : { valid: kind === 1, text }; };
  const created = date(), modified = date(), lastPrinted = date();
  const optional = () => { const present = c.u8(); const text = c.text(); return present ? text : null; };
  const lastModifiedBy = optional(), revision = optional(), application = optional(), appVersion = optional(), template = optional();
  const number = () => { const present = c.u8(); const value = c.u64(); return present ? value : null; };
  const totalTime = number();
  const statistics = { pages: number(), words: number(), characters: number(), charactersWithSpaces: number(), lines: number(), paragraphs: number() };
  const noticeCount = c.u32();
  if (c.at !== payload.byteLength) fail("INVALID_RESPONSE", "Trailing properties data.");
  return {
    kind: "snapshot",
    editable: Object.fromEntries(PROPERTY_FIELDS.map((name, index) => [name, values[index]])),
    limits: Object.fromEntries(PROPERTY_FIELDS.map((name, index) => [name, limits[index]])),
    created, modified, lastPrinted, lastModifiedBy, revision, application, appVersion, template, totalTime, statistics, noticeCount,
  };
}

function decodeApply(payload) {
  const view = new DataView(payload.buffer, payload.byteOffset, payload.byteLength);
  if (payload.byteLength < 8 || view.getUint16(4, true) !== 1) fail("INVALID_RESPONSE", "Invalid properties apply header.");
  const status = view.getUint16(6, true);
  if (status === 1) {
    if (payload.byteLength !== 20) fail("INVALID_RESPONSE", "Invalid too-long response.");
    const field = PROPERTY_FIELDS[view.getUint32(8, true)];
    if (field === undefined) fail("INVALID_RESPONSE", "Unknown property in too-long response.");
    return { kind: "tooLong", field, max: view.getUint32(12, true), length: view.getUint32(16, true) };
  }
  if (status !== 0 || payload.byteLength < 12) fail("INVALID_RESPONSE", "Invalid properties apply status.");
  return { kind: "applied", changed: view.getUint32(8, true), bytes: payload.slice(12) };
}

/** Decodes a Failed/EditorResult reply of opcodes 26 and 27. */
export function decodePropertiesResponse(type, payload) {
  if (type === MessageType.Failed) {
    const code = decoder.decode(payload.subarray(0, Math.min(payload.byteLength, 64)));
    return { kind: "failed", code };
  }
  if (type !== MessageType.EditorResult || payload.byteLength < 8) fail("INVALID_RESPONSE", "Unexpected properties response.");
  const magic = String.fromCharCode(...payload.subarray(0, 4));
  if (magic === "ATPP" && new DataView(payload.buffer, payload.byteOffset).getUint16(4, true) === 1) return decodeRead(payload);
  if (magic === "ATPA") return decodeApply(payload);
  return fail("INVALID_RESPONSE", "Unknown properties response.");
}

async function exchange(client, { requestId, frame }, expected, options) {
  const reply = decodeMessage(await client.request(frame, options), "event");
  if (reply.requestId !== requestId) fail("RESPONSE_MISMATCH", "Properties response for another request.");
  const response = decodePropertiesResponse(reply.type, reply.payload);
  if (response.kind === "failed") throw Object.assign(new Error(response.code), { code: response.code });
  if (response.kind !== expected && !(expected === "applied" && response.kind === "tooLong")) fail("INVALID_RESPONSE", "Unexpected properties response kind.");
  return response;
}

/** Reads the property snapshot of a DOCX through the engine client. */
export const readProperties = (client, docx, options) => exchange(client, encodePropertiesRead(docx), "snapshot", options);
/** Applies changes; resolves to `{ kind: "applied", changed, bytes }` or `{ kind: "tooLong", field, max, length }`. */
export const applyProperties = (client, docx, changes, options) => exchange(client, encodePropertiesApply(docx, changes), "applied", options);

/** Stamp prefix of an export-as-save job (staged kind 3): see crates/core-wasm/src/staged.rs. */
export function encodeSaveStamp({ unixSeconds, user = "", appVersion = "" }) {
  const userBytes = encoder.encode(user), versionBytes = encoder.encode(appVersion);
  if (!Number.isSafeInteger(unixSeconds) || userBytes.byteLength > 1024 || versionBytes.byteLength > 1024) fail("INVALID_ARGUMENT", "Invalid save stamp.");
  const out = new Uint8Array(12 + userBytes.byteLength + versionBytes.byteLength);
  const view = new DataView(out.buffer);
  view.setBigInt64(0, BigInt(unixSeconds), true);
  view.setUint16(8, userBytes.byteLength, true); out.set(userBytes, 10);
  view.setUint16(10 + userBytes.byteLength, versionBytes.byteLength, true);
  out.set(versionBytes, 12 + userBytes.byteLength);
  return out;
}
