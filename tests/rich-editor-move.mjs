// SPDX-License-Identifier: AGPL-3.0-or-later
// R7c: the codec of MoveText (text drag and drop) against tests/rich-editor-move-vectors.txt (written by the Rust
// tests of rich_protocol/move_tests.rs), and the client's handling of its State reply.
// `node tests/rich-editor-move.mjs`
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  decodeRichReply, decodeRichRequest, encodeRichReply, encodeRichRequest, RichErrorCode, RichProtocolError,
} from "../web/rich-editor-protocol.mjs";

const here = (p) => new URL(`./${p}`, import.meta.url);
const hex = (b) => Buffer.from(b).toString("hex");
const fromHex = (h) => Uint8Array.from(Buffer.from(h, "hex"));
let checks = 0;

const rows = readFileSync(here("rich-editor-move-vectors.txt"), "utf8")
  .split(/\r?\n/).filter((l) => l && !l.startsWith("#")).map((l) => l.split("\t"));
const counts = { request: 0, reply: 0, "bad-request": 0 };
for (const [kind, name, frameHex, expected] of rows) {
  counts[kind] += 1;
  const bytes = fromHex(frameHex);
  if (kind === "request") {
    const d = decodeRichRequest(bytes);
    const c = d.command;
    assert.equal(c.type, "moveText", name);
    for (const field of ["start", "end", "target"]) assert.equal(typeof c[field], "bigint", `${name} ${field} is a BigInt`);
    assert.equal(typeof c.copy, "boolean", `${name} copy is a boolean`);
    assert.equal(`doc=${d.docId} req=${d.requestId} MoveText start=${c.start} end=${c.end} target=${c.target} copy=${c.copy ? 1 : 0}`, expected, name);
    assert.equal(hex(encodeRichRequest(d)), frameHex, `${name} re-encode`);
    for (let cut = 0; cut < bytes.length; cut += 1) assert.throws(() => decodeRichRequest(bytes.subarray(0, cut)), RichProtocolError, `${name} cut ${cut}`);
  } else if (kind === "reply") {
    const d = decodeRichReply(bytes);
    assert.equal(`doc=${d.docId} req=${d.requestId} Error code=${d.response.code}`, expected, name);
    assert.equal(hex(encodeRichReply(d)), frameHex, `${name} re-encode`);
  } else {
    assert.throws(() => decodeRichRequest(bytes), (e) => e instanceof RichProtocolError && e.code === expected, name);
  }
  checks += 1;
}
assert.deepEqual(counts, { request: 3, reply: 1, "bad-request": 5 });
assert.equal(RichErrorCode.RICH_MOVE_UNSUPPORTED, 50);
// The encoder refuses what the decoder refuses.
assert.throws(() => encodeRichRequest({ docId: 1, requestId: 1, command: { type: "moveText", start: -1n, end: 1n, target: 0n, copy: false } }), RichProtocolError);
checks += 2;
console.log(JSON.stringify({ status: "passed", checks }));
