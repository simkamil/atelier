// SPDX-License-Identifier: AGPL-3.0-or-later
// US-240: ApplyCharFormat and LoadFonts against the real WASM engine behind the real client: every attribute, the word under the
// caret, mixed states, typing attributes in one undo step, the identifier of a command, the DOCX round trip of the result, and a font
// loaded on demand. `node tests/rich-editor-format-char.mjs [engine.wasm]`.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { bold, loadEngine, scenarios } from "./rich-editor-format-support.mjs";
import { decodeRichRequest, encodeRichRequest } from "../web/rich-editor-protocol.mjs";

const suite = scenarios();
const engineHost = await loadEngine();
const STAMP = { nowMs: 1_800_000_000_000n, user: "Test", version: "1" };

const select = (client, anchor, focus = anchor) => client.command({ type: "setSelection", anchor: BigInt(anchor), focus: BigInt(focus), upstream: false });
const type = (client, text) => client.command({ type: "insertText", text });
const edit = (attr, op, value) => (value === undefined ? { attr, op } : { attr, op, value });
const value = (field) => (field.state === "value" ? field.value : field.state);

async function save(client) {
  const result = await client.command({ type: "save", format: "docx", ...STAMP });
  assert.equal(result.response.type, "saveData");
  return result.response.bytes;
}

await suite.run("real WASM: bold, italic, strike and the word under the caret, one undo step each", async () => {
  const { client, format } = await engineHost.open();
  await type(client, "Hello world");
  await select(client, 1);
  const revision = client.cache.revision;
  const result = await format.applyCharFormat({ edits: [bold, edit("italic", "toggle")] });
  assert.equal(result.outcome, "applied");
  assert.equal(client.cache.revision, revision + 1n);
  await select(client, 0, 5);
  let state = await format.formatState();
  assert.equal(value(state.chars.bold), true);
  assert.equal(value(state.chars.italic), true);
  await select(client, 6, 11);
  state = await format.formatState();
  assert.equal(value(state.chars.bold), false);
  await select(client, 0, 11);
  assert.equal((await format.formatState()).chars.bold.state, "mixed");
  await format.applyCharFormat({ edits: [edit("strike", "toggle")] });
  assert.equal(value((await format.formatState()).chars.strike), true);
  await client.command({ type: "undo" });
  assert.equal(value((await format.formatState()).chars.strike), false);
  assert.equal(value((await format.formatState()).chars.bold), "mixed");
  client.dispose();
});

await suite.run("real WASM: font, size, colours, the six underlines, superscript and subscript", async () => {
  const { client, format } = await engineHost.open();
  await type(client, "Hello");
  await select(client, 0, 5);
  const red = { r: 255, g: 0, b: 0 };
  const yellow = { r: 255, g: 255, b: 0 };
  await format.applyCharFormat({ edits: [
    edit("font", "set", "Arial"), edit("size", "set", 21), edit("color", "set", red), edit("highlight", "set", yellow),
  ] });
  const state = await format.formatState();
  assert.equal(value(state.chars.font).requested, "Arial");
  assert.equal(value(state.chars.size), 21);
  assert.deepEqual(value(state.chars.color), red);
  assert.deepEqual(value(state.chars.highlight), yellow);
  for (const style of ["single", "double", "thick", "dotted", "dashed", "wave", "none"]) {
    await format.applyCharFormat({ edits: [edit("underline", "set", style)] });
    assert.equal(value((await format.formatState()).chars.underline), style);
  }
  await format.applyCharFormat({ edits: [edit("verticalAlign", "toggle", "superscript")] });
  assert.equal(value((await format.formatState()).chars.verticalAlign), "superscript");
  await format.applyCharFormat({ edits: [edit("verticalAlign", "toggle", "subscript")] });
  assert.equal(value((await format.formatState()).chars.verticalAlign), "subscript");
  await format.applyCharFormat({ edits: [edit("verticalAlign", "toggle", "subscript")] });
  assert.equal(value((await format.formatState()).chars.verticalAlign), "baseline");
  await format.applyCharFormat({ edits: [edit("color", "clear"), edit("highlight", "clear")] });
  const cleared = await format.formatState();
  assert.equal(cleared.chars.color.state, "unset");
  assert.equal(cleared.chars.highlight.state, "unset");
  client.dispose();
});

await suite.run("real WASM: size steps and the box of sizes", async () => {
  const { client, format } = await engineHost.open();
  await type(client, "ab cd");
  await select(client, 0, 2);
  await format.applyCharFormat({ edits: [edit("size", "stepUp")] });
  await select(client, 0, 5);
  assert.equal((await format.formatState()).chars.size.state, "mixed");
  await format.applyCharFormat({ edits: [edit("size", "stepUp")] });
  await select(client, 0, 2);
  assert.equal(value((await format.formatState()).chars.size), 28);
  await select(client, 3, 5);
  assert.equal(value((await format.formatState()).chars.size), 24);
  client.dispose();
});

await suite.run("real WASM: typing attributes join the typed text in one undo step", async () => {
  const { client, format } = await engineHost.open();
  await type(client, "Hello ");
  await format.applyCharFormat({ edits: [bold] });
  const pending = await format.formatState();
  assert.equal(pending.typing, true);
  assert.equal(value(pending.chars.bold), true);
  const revision = client.cache.revision;
  await type(client, "wor");
  assert.equal(client.cache.revision, revision + 1n);
  await select(client, 6, 9);
  assert.equal(value((await format.formatState()).chars.bold), true);
  await client.command({ type: "undo" });
  await select(client, 0, 6);
  assert.equal(value((await format.formatState()).chars.bold), false);
  assert.equal((await format.formatState()).chars.bold.state, "value");
  client.dispose();
});

await suite.run("real WASM: a repeated command identifier is refused and a refused command is not recorded", async () => {
  const { client, format } = await engineHost.open();
  await type(client, "Hello");
  await select(client, 0, 5);
  const command = (commandId) => ({ type: "applyCharFormat", baseRevision: client.cache.revision, commandId, scope: "wordOrTyping", edits: [bold] });
  await assert.rejects(client.command({ ...command(9), baseRevision: client.cache.revision + 5n }), (e) => e.code === "RICH_STALE_REVISION");
  assert.equal((await format.formatState()).lastCommandId, 0);
  await client.command(command(9));
  assert.equal((await format.formatState()).lastCommandId, 9);
  await assert.rejects(client.command(command(9)), (e) => e.code === "RICH_DUPLICATE_COMMAND");
  client.dispose();
});

await suite.run("real WASM: the formatting survives a DOCX save and reopening", async () => {
  const { client, format } = await engineHost.open();
  await type(client, "Hello");
  await select(client, 0, 5);
  await format.applyCharFormat({ edits: [
    bold, edit("italic", "toggle"), edit("strike", "toggle"), edit("underline", "set", "wave"), edit("font", "set", "Arial"),
    edit("size", "set", 29), edit("color", "set", { r: 0, g: 112, b: 192 }), edit("highlight", "set", { r: 255, g: 255, b: 0 }),
    edit("verticalAlign", "toggle", "superscript"),
  ] });
  const bytes = await save(client);
  client.dispose();
  const reopened = await engineHost.open({ kind: "docx", profile: "document", data: bytes });
  await select(reopened.client, 0, 5);
  const state = await reopened.format.formatState();
  assert.equal(value(state.chars.bold), true);
  assert.equal(value(state.chars.italic), true);
  assert.equal(value(state.chars.strike), true);
  assert.equal(value(state.chars.underline), "wave");
  assert.equal(value(state.chars.font).requested, "Arial");
  assert.equal(value(state.chars.size), 29);
  assert.deepEqual(value(state.chars.color), { r: 0, g: 112, b: 192 });
  assert.deepEqual(value(state.chars.highlight), { r: 255, g: 255, b: 0 });
  assert.equal(value(state.chars.verticalAlign), "superscript");
  reopened.client.dispose();
});

await suite.run("real WASM: a font loaded on demand replaces the substitute and is reported", async () => {
  const { client, format } = await engineHost.open();
  await type(client, "Hello");
  await select(client, 0, 5);
  await format.applyCharFormat({ edits: [edit("font", "set", "Arial")] });
  assert.deepEqual({ ...value((await format.formatState()).chars.font) }, { requested: "Arial", used: "Carlito", substituted: true });
  const styles = ["Regular", "Bold", "Italic", "BoldItalic"];
  const fonts = await Promise.all(styles.map(async (style) => new Uint8Array(await readFile(new URL(`../data/fonts/liberation/LiberationSans-${style}.ttf`, import.meta.url)))));
  const revision = client.cache.revision;
  await format.loadFonts({ fonts });
  assert.equal(client.cache.revision, revision, "a layout change, not a document change");
  assert.deepEqual({ ...value((await format.formatState()).chars.font) }, { requested: "Arial", used: "Liberation Sans", substituted: true });
  await format.loadFonts({ fonts });
  const list = await format.listFonts();
  assert.equal(list.families.find((f) => f.name === "Arial").substitute, "Liberation Sans");
  // Something that is no font is refused and changes nothing.
  await assert.rejects(format.loadFonts({ fonts: [new Uint8Array([1, 2, 3, 4])] }), (e) => e.code === "RICH_INVALID_BODY");
  assert.equal(value((await format.formatState()).chars.font).used, "Liberation Sans");
  client.dispose();
});

await suite.run("the LoadFonts codec round-trips and refuses malformed bodies", async () => {
  const font = new Uint8Array([0, 1, 0, 0, 7, 7]);
  const frame = encodeRichRequest({ docId: 1, requestId: 3, command: { type: "loadFonts", fonts: [font, font.slice(0, 4)] } });
  const decoded = decodeRichRequest(frame).command;
  assert.equal(decoded.type, "loadFonts");
  assert.deepEqual(decoded.fonts.map((f) => [...f]), [[...font], [...font.slice(0, 4)]]);
  assert.throws(() => encodeRichRequest({ docId: 1, requestId: 3, command: { type: "loadFonts", fonts: [] } }));
  assert.throws(() => encodeRichRequest({ docId: 1, requestId: 3, command: { type: "loadFonts", fonts: [new Uint8Array(0)] } }));
  assert.throws(() => encodeRichRequest({ docId: 1, requestId: 3, command: { type: "loadFonts", fonts: new Array(9).fill(font) } }));
  // A count of zero, a zero length and a truncated file are refused by the decoder.
  const body = (bytes) => {
    const copy = frame.slice(0, 20);
    return Uint8Array.from([...copy, ...bytes]);
  };
  const bad = [[0], [1, 0, 0, 0, 0], [1, 9, 0, 0, 0, 1, 2]];
  for (const tail of bad) {
    const raw = body(tail);
    new DataView(raw.buffer).setUint32(16, tail.length, true);
    assert.throws(() => decodeRichRequest(raw), `refused body ${tail}`);
  }
});

suite.finish("rich-editor-format-char", 8);
