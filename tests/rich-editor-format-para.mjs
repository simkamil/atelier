// SPDX-License-Identifier: AGPL-3.0-or-later
// US-243: ApplyParaFormat and FormatMarks against the real WASM engine: every paragraph the selection touches, one undo step,
// the mixed state, the refusals (shading and borders wait for US-243-M1), the marks, and the DOCX round trip of what the ribbon writes.
// `node tests/rich-editor-format-para.mjs [engine.wasm]`.
import assert from "node:assert/strict";
import { loadEngine, scenarios } from "./rich-editor-format-support.mjs";

const suite = scenarios();
const engineHost = await loadEngine();
const select = (client, anchor, focus) => client.command({ type: "setSelection", anchor: BigInt(anchor), focus: BigInt(focus), upstream: false });
const para = (client, format, anchor, focus) => select(client, anchor, focus).then(() => format.formatState()).then((s) => s.paragraph);
/** Three paragraphs: "one" (0..3), "two" (4..7), "three" (8..13). */
async function three() {
  const host = await engineHost.open();
  await host.client.command({ type: "insertText", text: "one\ntwo\nthree" });
  return host;
}
const align = (value) => ({ attr: "alignment", op: "set", value });

await suite.run("real WASM: every touched paragraph is formatted, mixed is reported, one Ctrl+Z restores them all", async () => {
  const { client, format } = await three();
  const revision = client.cache.revision;
  await select(client, 1, 9);
  const result = await format.applyParaFormat({ edits: [align("center")] });
  assert.equal(result.outcome, "applied");
  assert.equal(client.cache.revision, revision + 1n);
  for (const at of [0, 4, 8]) assert.deepEqual((await para(client, format, at, at)).alignment, { state: "value", value: "center" });
  await para(client, format, 0, 0);
  await format.applyParaFormat({ edits: [align("right")] });
  assert.deepEqual((await para(client, format, 0, 13)).alignment, { state: "mixed" });
  await select(client, 1, 9);
  await client.command({ type: "undo" });
  assert.deepEqual((await para(client, format, 0, 0)).alignment, { state: "value", value: "center" }, "the undo restores one step only");
  await client.command({ type: "undo" });
  for (const at of [0, 4, 8]) assert.deepEqual((await para(client, format, at, at)).alignment, { state: "value", value: "left" });
  client.dispose();
});

await suite.run("real WASM: a selection that ends at the start of a paragraph leaves it alone", async () => {
  const { client, format } = await three();
  await select(client, 0, 4);
  await format.applyParaFormat({ edits: [align("justify")] });
  assert.equal((await para(client, format, 4, 4)).alignment.value, "left");
  assert.equal((await para(client, format, 0, 0)).alignment.value, "justify");
  client.dispose();
});

await suite.run("real WASM: indents step by 720 twips, line spacing and spacing are set, toggled and cleared", async () => {
  const { client, format } = await three();
  await select(client, 0, 13);
  await format.applyParaFormat({ edits: [{ attr: "indentLeft", op: "stepUp" }] });
  await format.applyParaFormat({ edits: [{ attr: "indentLeft", op: "stepUp" }] });
  let p = await para(client, format, 0, 13);
  assert.deepEqual(p.indentLeft, { state: "value", value: 1440 });
  await format.applyParaFormat({ edits: [{ attr: "indentLeft", op: "stepDown" }] });
  assert.equal((await para(client, format, 0, 13)).indentLeft.value, 720);
  await format.applyParaFormat({ edits: [{ attr: "lineSpacing", op: "set", value: { rule: "auto", value: 360 } }, { attr: "firstLine", op: "set", value: -360 }] });
  p = await para(client, format, 0, 13);
  assert.deepEqual(p.lineSpacing.value, { rule: "auto", value: 360 });
  assert.equal(p.firstLine.value, -360);
  await format.applyParaFormat({ edits: [{ attr: "spaceBefore", op: "toggle" }] });
  assert.equal((await para(client, format, 0, 13)).spaceBefore.value, 240);
  await format.applyParaFormat({ edits: [{ attr: "spaceBefore", op: "toggle" }] });
  assert.equal((await para(client, format, 0, 13)).spaceBefore.value, 0);
  await format.applyParaFormat({ edits: [{ attr: "lineSpacing", op: "clear" }] });
  assert.equal((await para(client, format, 0, 13)).lineSpacing.value.rule, "auto");
  // Nothing to change: accepted, no revision.
  const revision = client.cache.revision;
  await format.applyParaFormat({ edits: [{ attr: "indentLeft", op: "set", value: 720 }] });
  assert.equal(client.cache.revision, revision);
  client.dispose();
});

await suite.run("real WASM: shading and borders are refused whole (RICH_FORMAT_NOT_IMPLEMENTED) and change nothing", async () => {
  const { client, format } = await three();
  const revision = client.cache.revision;
  await assert.rejects(format.applyParaFormat({ edits: [align("center"), { attr: "shading", op: "clear" }] }), (e) => e.code === "RICH_FORMAT_NOT_IMPLEMENTED");
  assert.equal(client.cache.revision, revision);
  assert.equal((await format.formatState()).lastCommandId, 0);
  client.dispose();
});

await suite.run("real WASM: FormatMarks lists spaces, tabs, no-break spaces and paragraph ends of a laid out page", async () => {
  const { client, format } = await engineHost.open();
  await client.command({ type: "insertText", text: "a b\tc d\ne" });
  const { revision, marks } = await format.formatMarks({ first: 0, count: 1 });
  assert.equal(revision, client.cache.revision);
  assert.deepEqual(marks.map((m) => m.kind), ["space", "tab", "noBreakSpace", "paragraphEnd", "paragraphEnd"]);
  assert.ok(marks.every((m) => m.page === 0 && m.width >= 0 && Number.isFinite(m.x) && Number.isFinite(m.y)));
  assert.ok(marks[4].y > marks[3].y, "the second paragraph is lower");
  await assert.rejects(format.formatMarks({ first: 9, count: 1 }), (e) => e.code === "RICH_PAGE_RANGE");
  // Reading marks changes neither the revision nor the atlas.
  assert.equal(client.cache.revision, revision);
  client.dispose();
});

await suite.run("real WASM: what the ribbon writes survives a DOCX save and a reopening", async () => {
  const { client, format } = await three();
  await select(client, 0, 13);
  await format.applyParaFormat({ edits: [align("justify"), { attr: "indentLeft", op: "set", value: 1440 }, { attr: "indentRight", op: "set", value: 720 },
    { attr: "firstLine", op: "set", value: -360 }, { attr: "lineSpacing", op: "set", value: { rule: "atLeast", value: 400 } },
    { attr: "spaceBefore", op: "set", value: 120 }, { attr: "spaceAfter", op: "set", value: 360 }] });
  await select(client, 4, 4);
  await format.applyParaFormat({ edits: [align("right"), { attr: "lineSpacing", op: "set", value: { rule: "exact", value: 500 } }] });
  const before = await para(client, format, 0, 13);
  assert.equal(before.alignment.state, "mixed");
  const saved = await client.command({ type: "save", format: "docx", nowMs: 1700000000000n, user: "test", version: "test" });
  assert.equal(saved.response.type, "saveData");
  const reopened = await engineHost.open({ kind: "docx", profile: "document", data: saved.response.bytes });
  const expected = [["justify", "atLeast", 400], ["right", "exact", 500], ["justify", "atLeast", 400]];
  for (const [index, at] of [0, 4, 8].entries()) {
    const p = await para(reopened.client, reopened.format, at, at);
    assert.equal(p.alignment.value, expected[index][0], `alignment ${index}`);
    assert.deepEqual(p.lineSpacing.value, { rule: expected[index][1], value: expected[index][2] }, `line spacing ${index}`);
    assert.equal(p.indentLeft.value, 1440); assert.equal(p.indentRight.value, 720); assert.equal(p.firstLine.value, -360);
    assert.equal(p.spaceBefore.value, 120); assert.equal(p.spaceAfter.value, 360);
  }
  reopened.client.dispose(); client.dispose();
});

suite.finish("rich-editor-format-para", 6);
