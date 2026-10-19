// SPDX-License-Identifier: AGPL-3.0-or-later
// Shared by the tests of the formatting family (F0): the real WASM engine behind the real client, and a scenario runner.
// F0 owns this file; the story tests (rich-editor-format-char.mjs, -para.mjs, -styles.mjs) only import it.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { decodeMessage, encodeMessage, MessageType } from "../web/wasm-protocol.mjs";
import { createRichEditorClient } from "../web/rich-editor-client.mjs";
import { createFormatClient } from "../web/rich-editor-format.mjs";

globalThis.Path2D ??= class { moveTo() {} lineTo() {} quadraticCurveTo() {} bezierCurveTo() {} closePath() {} };
export const turn = () => new Promise((resolve) => setImmediate(resolve));
export const bold = { attr: "bold", op: "toggle" };

/** Runs scenarios in order and counts them. */
export function scenarios() {
  const out = { checks: 0 };
  out.run = async (name, work) => { await work(); out.checks += 1; console.log(`ok - ${name}`); };
  out.finish = (label, minimum) => {
    console.log(`${label}: ${out.checks} checks passed`);
    assert.ok(out.checks >= minimum, "fewer scenarios ran than expected");
  };
  return out;
}

/** Loads the engine (default: the wasm-release build) and returns a transport and a client factory. */
export async function loadEngine(path = process.argv[2] ?? "target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm") {
  const module = await WebAssembly.compile(await readFile(path));
  const { memory, alloc, dispatch, free } = new WebAssembly.Instance(module).exports;
  let outerId = 1n;
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
  const transport = {
    async request(frame) {
      const result = invoke(encodeMessage({ type: MessageType.Editor, requestId: outerId++, payload: frame }));
      assert.equal(result.status, 0);
      const message = decodeMessage(result.frame, "event");
      assert.equal(message.type, MessageType.EditorResult);
      return message.payload;
    },
  };
  /** A client with a document of the given kind open and laid out; `{client, format}`. */
  async function open({ kind = "blank", profile = "document", data = new Uint8Array(0) } = {}) {
    const client = createRichEditorClient({ transport, docId: 1, yieldTurn: turn });
    await client.open({ kind, profile, data });
    await client.whenLayoutComplete();
    return { client, format: createFormatClient(client) };
  }
  return { transport, open };
}
