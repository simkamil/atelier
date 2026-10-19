// SPDX-License-Identifier: AGPL-3.0-or-later
// US-250: ListStyles, ApplyStyle and PreviewStyle against the real WASM engine, through the real client and the typed format client
// (the product's own paths: web/rich-editor-client.mjs, web/rich-editor-format.mjs). `node tests/rich-editor-format-styles.mjs [engine.wasm]`.
import assert from "node:assert/strict";
import { loadEngine, scenarios } from "./rich-editor-format-support.mjs";

const suite = scenarios();
const engineHost = await loadEngine();
const ids = (styles) => styles.map((s) => s.id);
const CHARACTER_STYLES = ["atelier.emphasis", "atelier.hyperlink", "atelier.strong"];

await suite.run("real WASM: the list of a blank document is ordered, flagged and described like Word's gallery", async () => {
  const { client, format } = await engineHost.open();
  const { styles, revision } = await format.listStyles({});
  assert.equal(revision, client.cache.revision);
  assert.equal(styles[0].id, "atelier.normal");
  assert.deepEqual({ builtIn: styles[0].builtIn, used: styles[0].used, current: styles[0].current, isDefault: styles[0].isDefault, quick: styles[0].quick },
    { builtIn: true, used: true, current: true, isDefault: true, quick: true });
  const h1 = styles.find((s) => s.id === "atelier.heading.1");
  assert.equal(h1.headingLevel, 1);
  assert.equal(h1.name, "heading 1");
  assert.equal(h1.target, "paragraph");
  assert.equal(h1.sample.halfPoints, 32);
  assert.equal(h1.sample.bold, true);
  assert.deepEqual(styles.filter((s) => s.target === "character").map((s) => s.id).sort(), CHARACTER_STYLES);
  assert.equal(styles.findIndex((s) => s.target === "character") > styles.findIndex((s) => s.id === "atelier.quote.intense"), true, "character styles after paragraph styles");
  assert.ok(!ids(styles).includes("atelier.table.grid"), "no table style");
  assert.deepEqual(ids((await format.listStyles({ filter: "character" })).styles).sort(), CHARACTER_STYLES);
  assert.deepEqual(ids((await format.listStyles({ usedOnly: true })).styles), ["atelier.normal"]);
  client.dispose();
});

await suite.run("real WASM: a paragraph style keeps the direct formatting, is one undo step and shows as current", async () => {
  const { client, format } = await engineHost.open();
  await client.command({ type: "insertText", text: "Hello" });
  await client.command({ type: "setSelection", anchor: 0n, focus: 5n, upstream: false });
  await format.applyCharFormat({ edits: [{ attr: "italic", op: "toggle" }] });
  const before = client.cache.revision;
  const result = await format.applyStyle({ target: "paragraph", id: "atelier.heading.2" });
  assert.equal(result.outcome, "applied");
  assert.equal(client.cache.revision, before + 1n);
  const state = await format.formatState();
  assert.deepEqual(state.paragraph.style, { state: "value", value: "atelier.heading.2" });
  assert.deepEqual(state.chars.italic, { state: "value", value: true }, "direct italic kept");
  const listed = (await format.listStyles({})).styles;
  assert.equal(listed.find((s) => s.id === "atelier.heading.2").current, true);
  assert.equal(listed.find((s) => s.id === "atelier.normal").current, false);
  await client.command({ type: "undo" });
  assert.equal((await format.formatState()).paragraph.style.value, "atelier.normal", "one undo restores the style");
  assert.deepEqual((await format.formatState()).chars.italic, { state: "value", value: true });
  client.dispose();
});

await suite.run("real WASM: refusals of ApplyStyle and PreviewStyle", async () => {
  const { client, format } = await engineHost.open();
  await client.command({ type: "insertText", text: "abc" });
  const revision = client.cache.revision;
  await assert.rejects(format.applyStyle({ target: "paragraph", id: "nope" }), (e) => e.code === "RICH_UNKNOWN_STYLE");
  await assert.rejects(format.applyStyle({ target: "paragraph", id: "atelier.emphasis" }), (e) => e.code === "RICH_UNKNOWN_STYLE");
  await assert.rejects(format.previewStyle({ target: "paragraph", id: "nope" }), (e) => e.code === "RICH_UNKNOWN_STYLE");
  assert.equal(client.cache.revision, revision);
  client.dispose();
});

await suite.run("real WASM: a preview changes no revision, cancels idempotently and is ended by a keystroke or by an application", async () => {
  const { client, format } = await engineHost.open();
  await client.command({ type: "insertText", text: "abc" });
  const revision = client.cache.revision;
  const shown = await format.previewStyle({ target: "paragraph", id: "atelier.title" });
  assert.equal(shown.response.type, "state");
  assert.equal(shown.response.revision, revision);
  assert.equal(client.cache.revision, revision);
  assert.equal((await format.formatState()).paragraph.style.value, "atelier.normal", "the model keeps its style");
  await format.cancelPreview();
  await format.cancelPreview();
  assert.equal(client.cache.revision, revision);
  // A keystroke ends a preview: nothing is left to cancel, and the document is the typed one.
  await format.previewStyle({ target: "paragraph", id: "atelier.title" });
  await client.command({ type: "insertText", text: "d" });
  assert.equal(client.cache.revision, revision + 1n);
  await format.cancelPreview();
  // An application ends it too and is the only change.
  await format.previewStyle({ target: "paragraph", id: "atelier.title" });
  await format.applyStyle({ target: "paragraph", id: "atelier.heading.1" });
  assert.equal(client.cache.revision, revision + 2n);
  // Ctrl+Z after a preview undoes the application, not a ghost.
  await format.previewStyle({ target: "paragraph", id: "atelier.title" });
  await client.command({ type: "undo" });
  assert.equal((await format.formatState()).paragraph.style.value, "atelier.normal");
  client.dispose();
});

await suite.run("real WASM: a lost preview reply is repaired by the cancel (the engine kept the preview)", async () => {
  const { client, format } = await engineHost.open();
  await client.command({ type: "insertText", text: "abc" });
  const revision = client.cache.revision;
  const real = engineHost.transport.request;
  let lose = true;
  engineHost.transport.request = async (frame) => {
    const reply = await real(frame);
    if (lose) { lose = false; throw new Error("reply lost"); }
    return reply;
  };
  try {
    await assert.rejects(format.previewStyle({ target: "paragraph", id: "atelier.title" }), (e) => e.code === "CLIENT_TRANSPORT");
  } finally { engineHost.transport.request = real; }
  // The client resynchronises on its next command; cancelling is safe whatever the engine holds.
  await format.cancelPreview();
  await format.cancelPreview();
  assert.equal(client.cache.revision, revision);
  const result = await format.applyStyle({ target: "paragraph", id: "atelier.heading.3" });
  assert.equal(result.outcome, "applied");
  assert.equal(client.cache.revision, revision + 1n);
  client.dispose();
});

await suite.run("real WASM: a character style over the selection or the word, and its removal", async () => {
  const { client, format } = await engineHost.open();
  await client.command({ type: "insertText", text: "Hello world" });
  await client.command({ type: "setSelection", anchor: 0n, focus: 5n, upstream: false });
  await format.applyStyle({ target: "character", id: "atelier.strong" });
  assert.deepEqual((await format.formatState()).chars.style, { state: "value", value: "atelier.strong" });
  assert.equal((await format.listStyles({})).styles.find((s) => s.id === "atelier.strong").current, true);
  await format.applyStyle({ target: "character", id: "" });
  assert.equal((await format.formatState()).chars.style.state, "unset");
  client.dispose();
});

await suite.run("real WASM: a plain-text document has no style and refuses the style commands", async () => {
  const { client, format } = await engineHost.open({ kind: "txt", profile: "txt", data: new TextEncoder().encode("plain") });
  assert.deepEqual((await format.listStyles({})).styles, [], "a plain-text document has no style");
  await assert.rejects(format.applyStyle({ target: "paragraph", id: "atelier.title" }), (e) => e.code === "RICH_FORMAT_PLAIN_TEXT");
  await assert.rejects(format.previewStyle({ target: "paragraph", id: "atelier.title" }), (e) => e.code === "RICH_FORMAT_PLAIN_TEXT");
  client.dispose();
});

suite.finish("rich-editor-format-styles", 7);
