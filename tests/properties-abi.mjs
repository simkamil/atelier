// SPDX-License-Identifier: AGPL-3.0-or-later
// US-328: the JS codecs against the real optimized WASM (opcodes 26/27 and the stamped save export).
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { decodeMessage, decodeStagedExport, decodeStagedProgress, encodeStagedBegin, encodeStagedStep, StagedKind, MessageType } from "../web/wasm-protocol.mjs";
import { applyProperties, encodePropertiesApply, encodePropertiesRead, encodeSaveStamp, readProperties } from "../web/properties-protocol.mjs";

const wasm = await readFile(process.argv[2] ?? "data/engine/atelier_core_wasm.wasm");
const { memory, alloc, dispatch, free } = new WebAssembly.Instance(await WebAssembly.compile(wasm)).exports;
const basic = new Uint8Array(await readFile("tests/docx-reference/basic.docx"));

function invoke(frame) {
  const ptr = alloc(frame.length) >>> 0;
  assert.notEqual(ptr, 0);
  new Uint8Array(memory.buffer).set(frame, ptr);
  const descriptorPtr = dispatch(ptr, frame.length) >>> 0;
  assert.notEqual(descriptorPtr, 0);
  const descriptor = new DataView(memory.buffer, descriptorPtr, 8);
  const [bodyPtr, bodyLen] = [descriptor.getUint32(0, true), descriptor.getUint32(4, true)];
  try {
    const body = new Uint8Array(memory.buffer, bodyPtr, bodyLen).slice();
    assert.equal(new DataView(body.buffer).getUint32(0, true), 0, "dispatch status");
    return body.slice(4);
  } finally { free(bodyPtr, bodyLen); free(descriptorPtr, 8); }
}
// The codecs only need `client.request(frame)`.
const client = { request: async frame => invoke(frame) };

const empty = await readProperties(client, basic);
assert.equal(empty.kind, "snapshot");
assert.deepEqual(Object.values(empty.editable), Array(11).fill(""));
assert.equal(empty.limits.title, 255);
assert.equal(empty.limits.comments, 4096);
assert.equal(empty.created, null);
assert.equal(empty.statistics.words, null);

const edits = { title: "Rapport « Œuvre » — 2026", author: "Élodie Müller", keywords: "un; deux", comments: "a\nb", category: "Finance", subject: "S & <T>" };
const applied = await applyProperties(client, basic, edits);
assert.equal(applied.kind, "applied");
assert.equal(applied.changed, 6);
const back = await readProperties(client, applied.bytes);
for (const [name, value] of Object.entries(edits)) assert.equal(back.editable[name], value, name);

const unchanged = await applyProperties(client, applied.bytes, edits);
assert.equal(unchanged.changed, 0);
assert.deepEqual(unchanged.bytes, applied.bytes, "no change keeps the bytes");

const tooLong = await applyProperties(client, basic, { title: "x".repeat(256) });
assert.deepEqual(tooLong, { kind: "tooLong", field: "title", max: 255, length: 256 });

await assert.rejects(readProperties(client, new Uint8Array([1, 2, 3])), { code: "EDITOR_PROPERTIES_FAILED" });
assert.throws(() => encodePropertiesApply(basic, { nonsense: "x" }), { code: "INVALID_ARGUMENT" });
assert.throws(() => encodePropertiesRead("not bytes"), { code: "INVALID_ARGUMENT" });
// Truncated command: the engine refuses it cleanly.
const { frame } = encodePropertiesApply(basic, { title: "x" });
const truncated = frame.slice(0, frame.length - 3);
new DataView(truncated.buffer).setUint32(20, truncated.length - 24, true);
const refusal = decodeMessage(invoke(truncated), "event");
assert.equal(refusal.type, MessageType.Failed);
assert.equal(new TextDecoder().decode(refusal.payload), "EDITOR_INVALID_COMMAND");

// Stamped save export (staged kind 3).
async function saveExport(docx, unixSeconds, user) {
  const stamp = encodeSaveStamp({ unixSeconds, user, appVersion: "0.1.0" });
  const begin = decodeMessage(invoke(encodeStagedBegin(900n, StagedKind.ExportDocxSave, docx, stamp)), "event");
  assert.equal(begin.type, MessageType.EditorResult);
  const id = decodeStagedProgress(begin.payload).jobId;
  for (;;) {
    const step = decodeMessage(invoke(encodeStagedStep(900n, id)), "event");
    assert.equal(step.type, MessageType.EditorResult);
    if (!decodeStagedProgress(step.payload)) return decodeStagedExport(step.payload).bytes;
  }
}
const saved = await readProperties(client, await saveExport(applied.bytes, 1_700_000_000, "Grace"));
assert.equal(saved.editable.title, edits.title, "edits survive the save");
assert.deepEqual(saved.created, { valid: true, text: "2023-11-14T22:13:20Z" });
assert.deepEqual(saved.modified, { valid: true, text: "2023-11-14T22:13:20Z" });
assert.equal(saved.lastModifiedBy, "Grace");
assert.equal(saved.revision, "1");
assert.equal(saved.application, "Atelier");
assert.equal(saved.appVersion, "0.1.0");
assert.ok(saved.statistics.words > 0 && saved.statistics.paragraphs > 0 && saved.statistics.characters > 0);
assert.throws(() => encodeStagedBegin(1n, StagedKind.ExportDocxSave, basic), { code: "INVALID_ARGUMENT" });
assert.throws(() => encodeStagedBegin(1n, StagedKind.ExportDocx, basic, stamp()), { code: "INVALID_ARGUMENT" });
function stamp() { return encodeSaveStamp({ unixSeconds: 0 }); }
console.log("properties ABI: ok");
