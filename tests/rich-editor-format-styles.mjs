// SPDX-License-Identifier: AGPL-3.0-or-later
// US-250: ListStyles, ApplyStyle and PreviewStyle against the real WASM engine. Until the handler is written it answers the documented error
// (RICH_FORMAT_NOT_IMPLEMENTED) and changes nothing; the story replaces the scenario below with its own and is the only
// one to edit this file. `node tests/rich-editor-format-styles.mjs [engine.wasm]`.
import assert from "node:assert/strict";
import { bold, loadEngine, scenarios } from "./rich-editor-format-support.mjs";

const suite = scenarios();
const engineHost = await loadEngine();

await suite.run("real WASM: the style stubs answer RICH_FORMAT_NOT_IMPLEMENTED and leave the document alone", async () => {
  const { client, format } = await engineHost.open();
  await client.command({ type: "insertText", text: "abc" });
  const revision = client.cache.revision;
  await assert.rejects(format.applyStyle({ target: "paragraph", id: "Heading1" }), (e) => e.code === "RICH_FORMAT_NOT_IMPLEMENTED");
  await assert.rejects(format.listStyles({}), (e) => e.code === "RICH_FORMAT_NOT_IMPLEMENTED");
  await assert.rejects(format.previewStyle({ target: "paragraph", id: "Heading1" }), (e) => e.code === "RICH_FORMAT_NOT_IMPLEMENTED");
  assert.equal(client.cache.revision, revision);
  client.dispose();
});

suite.finish("rich-editor-format-styles", 1);
