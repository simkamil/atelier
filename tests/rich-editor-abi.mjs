// SPDX-License-Identifier: AGPL-3.0-or-later
// US-217: the scenarios of tests/editor-abi.mjs replayed against the real WASM
// engine in ATED v2 frames. `node tests/rich-editor-abi.mjs [engine.wasm]`
// (default: the wasm-release build of `python tools/package-engine.py`).
//
// SCENARIOS maps every scenario of tests/editor-abi.mjs to its v2 equivalent,
// or says why it has none. A scenario without equivalent keeps the US-217
// criterion "the scenarios of editor-abi.mjs pass in v2" unticked.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { decodeMessage, encodeMessage, MessageType } from "../web/wasm-protocol.mjs";
import {
  decodeRichReply, encodeRichReply, encodeRichRequest, RichProtocolError,
} from "../web/rich-editor-protocol.mjs";

const wasmPath = process.argv[2] ?? "target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm";
const bytes = await readFile(wasmPath);
const module = await WebAssembly.compile(bytes);
const { memory, alloc, dispatch, free } = new WebAssembly.Instance(module).exports;
let outerId = 1n;
let innerId = 1;
let docId = 1;

function invoke(frame) {
  const ptr = alloc(frame.length) >>> 0;
  assert.notEqual(ptr, 0, "input allocation");
  new Uint8Array(memory.buffer).set(frame, ptr);
  const descriptorPtr = dispatch(ptr, frame.length) >>> 0;
  assert.notEqual(descriptorPtr, 0, "response descriptor");
  const descriptor = new DataView(memory.buffer, descriptorPtr, 8);
  const bodyPtr = descriptor.getUint32(0, true);
  const bodyLen = descriptor.getUint32(4, true);
  try {
    const body = new Uint8Array(memory.buffer, bodyPtr, bodyLen).slice();
    return { status: new DataView(body.buffer).getUint32(0, true), frame: body.slice(4) };
  } finally {
    assert.equal(free(bodyPtr, bodyLen), 0);
    assert.equal(free(descriptorPtr, 8), 0);
  }
}

/** Sends a v2 payload and returns the decoded reply, or {failed: name} for an outer Failed. */
function sendPayload(payload) {
  const result = invoke(encodeMessage({ type: MessageType.Editor, requestId: outerId++, payload }));
  assert.equal(result.status, 0);
  const message = decodeMessage(result.frame, "event");
  assert.equal(message.requestId, outerId - 1n);
  if (message.type === MessageType.Failed) return { failed: new TextDecoder().decode(message.payload) };
  assert.equal(message.type, MessageType.EditorResult);
  return decodeRichReply(message.payload);
}

const frameOf = (command, doc = docId) => encodeRichRequest({ docId: doc, requestId: innerId++, command });
function command(cmd, doc = docId) {
  const reply = sendPayload(frameOf(cmd, doc));
  assert.equal(reply.docId, doc);
  assert.equal(reply.requestId, innerId - 1);
  return reply.response;
}
function expectState(cmd) {
  const r = command(cmd);
  assert.equal(r.type, "state", JSON.stringify(r.code ?? r.type));
  return r;
}
function expectError(payload, code, name) {
  const reply = sendPayload(payload);
  assert.equal(reply.failed ?? reply.response?.code, code, name);
}
const open = (kind, profile, data = new Uint8Array()) => command({ type: "open", kind, profile, data });
const openTxt = (text, doc) => {
  const r = command({ type: "open", kind: "txt", profile: "txt", data: typeof text === "string" ? new TextEncoder().encode(text) : text }, doc);
  assert.equal(r.type, "state");
  return r;
};
const textOf = () => {
  const r = command({ type: "textWindow", start: 0n, len: 262144 });
  assert.equal(r.type, "textWindow");
  return r.text;
};
const select = (anchor, focus) => expectState({ type: "setSelection", anchor: BigInt(anchor), focus: BigInt(focus), upstream: false });
const quiet = () => {
  const s = expectState({ type: "layout", budget: 1 });
  const { revision, anchor, focus, docLen, modified, canUndo, canRedo, focusUpstream } = s;
  return { revision, anchor, focus, docLen, modified, canUndo, canRedo, focusUpstream };
};
const u8 = (n) => new TextEncoder().encode(n);

const SCENARIOS = [];
function scenario(v1, v2, run) { SCENARIOS.push({ v1, v2, run }); }
function missing(v1, reason) { SCENARIOS.push({ v1, v2: null, reason }); }

scenario("ProbePoint 20-byte body layout (opcode 15)", "HitTest probe body layout (opcode 0x0108, 24-byte body)", () => {
  const f = frameOf({ type: "hitTest", page: 2, mode: "probe", x: 12.5, y: 34.25 });
  const v = new DataView(f.buffer);
  assert.equal(f.length, 20 + 24);
  assert.equal(v.getUint16(4, true), 2);
  assert.equal(v.getUint16(6, true), 0x0108);
  assert.equal(v.getUint32(20, true), 2);
  assert.equal(v.getFloat64(28, true), 12.5);
  assert.equal(v.getFloat64(36, true), 34.25);
});
missing("DropText byte layout, move and external kinds (opcode 21)", "v2 has no drag-and-drop command; moving text is two edits (a later story, US-243).");
scenario("ReplaceRange byte layout (opcode 17)", "ReplaceRange byte layout (opcode 0x0104)", () => {
  const f = frameOf({ type: "replaceRange", start: 14n, end: 15n, text: " :" });
  const v = new DataView(f.buffer);
  assert.equal(f.length, 20 + 8 + 8 + 4 + 3);
  assert.equal(v.getBigUint64(20, true), 14n);
  assert.equal(v.getBigUint64(28, true), 15n);
  assert.deepEqual([...f.subarray(40)], [0xc2, 0xa0, 0x3a]);
});
scenario("Load text with BOM and CRLF", "Open TXT with BOM and CRLF: one paragraph per line, BOM dropped", () => {
  const r = openTxt(u8("﻿seed\r\né"));
  assert.equal(r.revision, 0n);
  assert.equal(r.modified, false);
  assert.equal(textOf(), "seed\né");
});
missing("Export returns the loaded bytes (BOM, CRLF)", "v2 never returns the document; saving as TXT and DOCX with the formatting-loss report is US-218 (R3b).");
scenario("ABI limit: input fits, response does not; input released, candidate unpublished", "Same pressure on a v2 read-only command, then a resynchronising Layout", () => {
  openTxt("lorem ipsum ".repeat(300));
  const before = quiet();
  const held = [];
  try {
    for (const size of [64 * 1024 * 1024, 64 * 1024 * 1024 - 4096]) {
      const ptr = alloc(size) >>> 0;
      assert.notEqual(ptr, 0, "pressure allocation");
      held.push([ptr, size]);
    }
    const rejected = invoke(encodeMessage({ type: MessageType.Editor, requestId: outerId++, payload: frameOf({ type: "pages", first: 0, count: 1 }) }));
    assert.equal(rejected.status, 2, "ABI limit status");
    assert.equal(rejected.frame.length, 0);
  } finally {
    for (const [ptr, size] of held) assert.equal(free(ptr, size), 0);
  }
  assert.deepEqual(quiet(), before);
});
missing("A rejected response leaves the edit unpublished (state rolled back)", "v2 edits are applied in place, with no candidate to drop: an edit whose reply is refused by the ABI stays applied and the client resynchronises with Layout (revision in every reply). Not equivalent.");
scenario("Malformed ATLM envelope (corrupt payload length) is refused, state intact", "Same corruption on a v2 frame", () => {
  openTxt("discard me");
  select(2, 2);
  const before = quiet();
  const frame = encodeMessage({ type: MessageType.Editor, requestId: outerId++, payload: frameOf({ type: "insertText", text: "zz" }) });
  frame[8] ^= 1;
  const result = invoke(frame);
  assert.equal(result.status, 0);
  const failure = decodeMessage(result.frame, "event");
  assert.equal(failure.type, MessageType.Failed);
  assert.equal(new TextDecoder().decode(failure.payload), "EDITOR_INVALID_ENVELOPE");
  assert.deepEqual(quiet(), before);
  assert.equal(textOf(), "discard me");
});

function mutated(name, build, code, { outer = false } = {}) {
  return () => {
    const before = quiet();
    const frame = build();
    const reply = sendPayload(frame);
    assert.equal(outer ? reply.failed : reply.response.code, code, name);
    assert.deepEqual(quiet(), before, `${name}: state unchanged`);
  };
}
const withBody = (opcodeCmd, edit) => () => {
  const f = frameOf(opcodeCmd);
  edit(f, new DataView(f.buffer));
  return f;
};
const retitled = (f, body) => {
  const out = new Uint8Array(20 + body.length);
  out.set(f.subarray(0, 20));
  out.set(body, 20);
  new DataView(out.buffer).setUint32(16, body.length, true);
  return out;
};

const hit = { type: "hitTest", page: 0, mode: "setCaret", x: 72, y: 72 };
for (const [v1, v2, build, code] of [
  ["SelectPoint unknown page", "HitTest page past the page count", () => frameOf({ ...hit, page: 99 }), "RICH_PAGE_RANGE"],
  ["SelectPoint NaN coordinate", "HitTest NaN coordinate", withBody(hit, (f, v) => v.setFloat64(28, NaN, true)), "RICH_INVALID_BODY"],
  ["SelectPoint infinite coordinate", "HitTest infinite coordinate", withBody(hit, (f, v) => v.setFloat64(36, Infinity, true)), "RICH_INVALID_BODY"],
  ["SelectPoint unknown unit", "HitTest unknown mode", withBody(hit, (f) => { f[24] = 3; }), "RICH_INVALID_BODY"],
  ["SelectPoint invalid boolean", "Move extend flag 2", withBody({ type: "move", direction: "left", extend: false }, (f) => { f[21] = 2; }), "RICH_INVALID_BODY"],
  ["SelectPoint reserved field", "SetSelection reserved byte", withBody({ type: "setSelection", anchor: 0n, focus: 0n, upstream: false }, (f) => { f[38] = 1; }), "RICH_INVALID_BODY"],
  ["SelectPoint mid UTF-8 anchor", "ReplaceRange inside a scalar", () => frameOf({ type: "replaceRange", start: 4n, end: 5n, text: "x" }), "RICH_POSITION_NOT_BOUNDARY"],
  ["SelectPoint past-end anchor", "ReplaceRange past the end", () => frameOf({ type: "replaceRange", start: 0n, end: 999n, text: "x" }), "RICH_POSITION_OUT_OF_RANGE"],
  ["SelectPoint truncated body", "Move frame cut short (length field kept)", () => frameOf({ type: "move", direction: "left", extend: false }).slice(0, -1), "RICH_INVALID_ENVELOPE"],
  ["SelectPoint truncated body", "Move body one byte short (length field fixed)", () => {
    const f = frameOf({ type: "move", direction: "left", extend: false });
    return retitled(f, f.subarray(20, -1));
  }, "RICH_INVALID_BODY"],
  ["SelectPoint trailing byte", "Move body with a trailing byte", () => {
    const f = frameOf({ type: "move", direction: "left", extend: false });
    return retitled(f, Uint8Array.from([...f.subarray(20), 0]));
  }, "RICH_INVALID_BODY"],
]) scenario(v1, v2, mutated(v2, build, code));

scenario("ProbePoint changes no state; Undo after it reaches the prior edit with its selection", "HitTest probe, then Undo", () => {
  openTxt("abc");
  select(3, 2);
  const typed = expectState({ type: "insertText", text: "!" });
  assert.equal(textOf(), "ab!");
  assert.equal(typed.focus, 3n);
  select(2, 1);
  const before = quiet();
  const probe = command({ type: "hitTest", page: 0, mode: "probe", x: 54, y: 65 });
  assert.equal(probe.type, "hitResult");
  assert.equal(probe.revision, before.revision);
  assert.deepEqual(quiet(), before, "the probe preserves revision, selection and history flags");
  const undone = expectState({ type: "undo" });
  assert.equal(textOf(), "abc");
  // v1 restores the direction (anchor 3, focus 2); v2 restores the replaced range, in order.
  assert.deepEqual([undone.anchor, undone.focus], [2n, 3n]);
});
missing("DropText move: text, export, undo, redo", "No drag-and-drop command in v2 (see above). The undo and redo halves are covered by other scenarios.");
scenario("ReplaceRange (typographic spacing): one Undo restores the raw text, one Redo the spaced text", "InsertText then ReplaceRange, Undo, Redo", () => {
  openTxt("mot");
  select(3, 3);
  const raw = expectState({ type: "insertText", text: ":" });
  assert.equal(textOf(), "mot:");
  const spaced = expectState({ type: "replaceRange", start: 3n, end: 4n, text: " :" });
  assert.equal(textOf(), "mot :");
  assert.equal(spaced.revision, raw.revision + 1n);
  expectState({ type: "undo" });
  assert.equal(textOf(), "mot:");
  expectState({ type: "redo" });
  assert.equal(textOf(), "mot :");
});
missing("ReplaceRange with a stale expected revision is refused (EDITOR_STALE_EDIT)", "v2 ReplaceRange carries no expected revision (one request in flight, in order). The nearest guard, MarkSaved with a stale revision, is checked below.");
scenario("(guard nearest to the stale revision) MarkSaved with a stale revision", "MarkSaved stale revision, state intact", () => {
  openTxt("abc");
  select(1, 2);
  const before = quiet();
  const r = command({ type: "markSaved", revision: before.revision + 1n });
  assert.equal(r.code, "RICH_STALE_REVISION");
  assert.deepEqual(quiet(), before);
});
missing("DropText invalid kind and non-zero reserved byte", "No DropText in v2.");
scenario("ReplaceRange invalid UTF-8", "ReplaceRange invalid UTF-8", () => {
  openTxt("abc");
  select(1, 2);
  const before = quiet();
  const f = frameOf({ type: "replaceRange", start: 0n, end: 1n, text: "x" });
  f[f.length - 1] = 0xff;
  expectError(f, "RICH_INVALID_UTF8", "invalid replacement UTF-8");
  assert.deepEqual(quiet(), before);
});
scenario("ReplaceRange invalid offset", "ReplaceRange offset past the end", () => {
  openTxt("abc");
  const before = quiet();
  expectError(frameOf({ type: "replaceRange", start: 99n, end: 100n, text: "x" }), "RICH_POSITION_OUT_OF_RANGE", "offset");
  assert.deepEqual(quiet(), before);
});
scenario("ReplaceRange truncated payload", "ReplaceRange body one byte short", () => {
  openTxt("abc");
  const before = quiet();
  const f = frameOf({ type: "replaceRange", start: 0n, end: 0n, text: "" });
  expectError(retitled(f, f.subarray(20, -1)), "RICH_INVALID_BODY", "truncated");
  assert.deepEqual(quiet(), before);
});
scenario("Malformed ATPT (point geometry) response is rejected by the decoder", "Malformed v2 replies are rejected by the decoder (HitResult reserved byte)", () => {
  const hitReply = encodeRichReply({ docId: 1, requestId: 1, response: { type: "hitResult", revision: 1n, page: 0, position: 0n, upstream: false } });
  hitReply[hitReply.length - 1] = 1;
  assert.throws(() => decodeRichReply(hitReply), (e) => e instanceof RichProtocolError && e.code === "RICH_INVALID_BODY");
});
scenario("PreviewDocx on the real WASM leaves the TXT state unchanged; a malformed DOCX is refused", "Open DOCX of tests/docx-reference/basic.docx; a malformed Open keeps the active document", async () => {
  openTxt("abc");
  select(1, 1);
  const before = quiet();
  const docx = new Uint8Array(await readFile("tests/docx-reference/basic.docx"));
  const failed = command({ type: "open", kind: "docx", profile: "document", data: Uint8Array.from([1, 2, 3]) }, 2);
  assert.equal(failed.code, "RICH_OPEN_FAILED");
  assert.deepEqual(quiet(), before, "failed Open preserves the active document");
  const opened = command({ type: "open", kind: "docx", profile: "document", data: docx }, 2);
  assert.equal(opened.type, "state");
  assert.equal(opened.pages.length, 1);
  assert.ok(opened.window.text.startsWith("Référence de titre"));
  docId = 2;
  assert.ok(opened.docLen > 10n);
  // The previous document is gone: id 1 is unknown now.
  const gone = command({ type: "undo" }, 1);
  assert.equal(gone.code, "RICH_UNKNOWN_DOCUMENT");
  openTxt("fin", 1);
  docId = 1;
});

let passed = 0;
for (const s of SCENARIOS) {
  if (!s.run) continue;
  docId = 1;
  openTxt("abcé");
  select(1, 2);
  await s.run();
  passed += 1;
}

const none = SCENARIOS.filter((s) => !s.run);
console.log("v1 scenario -> v2 equivalent");
for (const s of SCENARIOS) console.log(`  ${s.run ? "ok   " : "NONE "} ${s.v1} -> ${s.v2 ?? `no equivalent: ${s.reason}`}`);
console.log(`Rich editor real WASM ABI (v2): ${passed} scenarios passed, ${none.length} v1 scenarios without v2 equivalent.`);
if (none.length) console.log("US-217 criterion 'the scenarios of editor-abi.mjs pass in v2' stays unticked.");
