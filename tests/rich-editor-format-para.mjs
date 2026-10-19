// SPDX-License-Identifier: AGPL-3.0-or-later
// US-243: ApplyParaFormat and FormatMarks against the real WASM engine. Until the handler is written it answers the documented error
// (RICH_FORMAT_NOT_IMPLEMENTED) and changes nothing; the story replaces the scenario below with its own and is the only
// one to edit this file. `node tests/rich-editor-format-para.mjs [engine.wasm]`.
import assert from "node:assert/strict";
import { bold, loadEngine, scenarios } from "./rich-editor-format-support.mjs";

const suite = scenarios();
const engineHost = await loadEngine();

await suite.run("real WASM: the paragraph stubs answer RICH_FORMAT_NOT_IMPLEMENTED and leave the document alone", async () => {
  const { client, format } = await engineHost.open();
  await client.command({ type: "insertText", text: "abc" });
  const revision = client.cache.revision;
  await assert.rejects(format.applyParaFormat({ edits: [{ attr: "alignment", op: "set", value: "center" }] }), (e) => e.code === "RICH_FORMAT_NOT_IMPLEMENTED");
  await assert.rejects(format.formatMarks({ first: 0, count: 1 }), (e) => e.code === "RICH_FORMAT_NOT_IMPLEMENTED");
  assert.equal(client.cache.revision, revision);
  client.dispose();
});

suite.finish("rich-editor-format-para", 1);
