// SPDX-License-Identifier: AGPL-3.0-or-later
// US-240: ApplyCharFormat against the real WASM engine. Until the handler is written it answers the documented error
// (RICH_FORMAT_NOT_IMPLEMENTED) and changes nothing; the story replaces the scenario below with its own and is the only
// one to edit this file. `node tests/rich-editor-format-char.mjs [engine.wasm]`.
import assert from "node:assert/strict";
import { bold, loadEngine, scenarios } from "./rich-editor-format-support.mjs";

const suite = scenarios();
const engineHost = await loadEngine();

await suite.run("real WASM: the character stub answers RICH_FORMAT_NOT_IMPLEMENTED and leaves the document alone", async () => {
  const { client, format } = await engineHost.open();
  await client.command({ type: "insertText", text: "abc" });
  const revision = client.cache.revision;
  await assert.rejects(format.applyCharFormat({ edits: [bold] }), (e) => e.code === "RICH_FORMAT_NOT_IMPLEMENTED" && e.outcome === undefined);
  // A command refused or not implemented is never recorded: the same identifier is not a duplicate.
  const repeat = { type: "applyCharFormat", baseRevision: revision, commandId: 9, scope: "wordOrTyping", edits: [bold] };
  for (let i = 0; i < 2; i += 1) await assert.rejects(client.command(repeat), (e) => e.code === "RICH_FORMAT_NOT_IMPLEMENTED");
  assert.equal((await format.formatState()).lastCommandId, 0);
  assert.equal(client.cache.revision, revision);
  client.dispose();
});

suite.finish("rich-editor-format-char", 1);
