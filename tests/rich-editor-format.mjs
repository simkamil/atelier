// SPDX-License-Identifier: AGPL-3.0-or-later
// F0: tests of the formatting family of ATED v2 (docs/us-240-format-protocol.md).
//   1. the frozen vectors (tests/rich-editor-format-vectors.txt) against the JS codec;
//   2. the format client (web/rich-editor-format.mjs) on a simulated engine: revision guard read when the command's
//      turn comes, lost reply, cancellation, refusals, stubs;
//   3. the real WASM engine: FormatState and ListFonts end to end through the real client. The stubs of each story are
//      tested in that story's own file (rich-editor-format-char.mjs, -para.mjs, -styles.mjs).
// `node tests/rich-editor-format.mjs [engine.wasm]` (default: the wasm-release build, see tools/package-engine.py).
import assert from "node:assert/strict";
import { decodeRichRequest, encodeRichReply } from "../web/rich-editor-protocol.mjs";
import { createRichEditorClient } from "../web/rich-editor-client.mjs";
import { createFormatClient, NO_REVISION_GUARD } from "../web/rich-editor-format.mjs";
import { verifyFormatVectors } from "./rich-editor-format-vectors.mjs";
import { bold, loadEngine, scenarios, turn } from "./rich-editor-format-support.mjs";

const abortError = () => new DOMException("The operation was aborted.", "AbortError");
const suite = scenarios();
const scenario = suite.run;

// ------------------------------------------------------------------ 1. vectors

const rows = verifyFormatVectors();
console.log(`ok - ${rows} frozen vectors of the formatting family`);
suite.checks += 1;

// ------------------------------------------------------------------ 2. simulated engine

const unset = { state: "unset" };
const value = (v) => ({ state: "value", value: v });
const formatStateReply = (revision, over = {}) => ({
  type: "formatState", revision, anchor: 0n, focus: 0n, lastCommandId: 0, collapsed: true, readOnly: false, partial: false, typing: false, plain: false,
  chars: { font: unset, size: value(22), bold: value(false), italic: value(false), underline: value("none"), strike: value(false), verticalAlign: value("baseline"), color: unset, highlight: unset, style: unset },
  paragraph: { alignment: value("left"), indentLeft: value(0), indentRight: value(0), firstLine: value(0), spaceBefore: value(0), spaceAfter: value(160), lineSpacing: value({ rule: "auto", value: 259 }), shading: unset, borders: { top: unset, left: unset, bottom: unset, right: unset, between: unset, bar: unset }, style: unset },
  ...over,
});

/** Engine double with the revision rules of the contract: guard, read-only, one revision per applied command. */
function makeEngine() {
  const e = { rev: 0n, last: 0, readOnly: false, notImplemented: false, typingOnly: false, applied: [], log: [] };
  const state = () => ({
    type: "state", revision: e.rev, anchor: 0n, focus: 0n, focusUpstream: false, docLen: 10n, modified: e.rev > 0n, canUndo: e.rev > 0n, canRedo: false,
    layoutComplete: true, pageCount: 1, caret: null, invalidated: [], atlas: { epoch: 1, first: 0, entries: [] }, pages: [], window: null,
  });
  e.handle = ({ command: c }) => {
    e.log.push(c.type);
    switch (c.type) {
      case "open": e.rev = 0n; return state();
      case "layout": case "resync": return state();
      case "insertText": e.rev += 1n; return state();
      case "formatState": return formatStateReply(e.rev, { readOnly: e.readOnly, lastCommandId: e.last });
      case "applyCharFormat": case "applyParaFormat": case "applyStyle":
        if (e.readOnly) return { type: "error", code: "RICH_READ_ONLY" };
        if (e.notImplemented) return { type: "error", code: "RICH_FORMAT_NOT_IMPLEMENTED" };
        if (c.commandId !== 0 && c.commandId === e.last) return { type: "error", code: "RICH_DUPLICATE_COMMAND" };
        if (c.baseRevision !== NO_REVISION_GUARD && c.baseRevision !== e.rev) return { type: "error", code: "RICH_STALE_REVISION" };
        e.applied.push({ type: c.type, at: e.rev, id: c.commandId });
        e.last = c.commandId;
        // Typing attributes only: accepted, nothing changes, no revision.
        if (!e.typingOnly) e.rev += 1n;
        return state();
      case "previewStyle":
        if (c.show && c.id === "Missing") return { type: "error", code: "RICH_UNKNOWN_STYLE" };
        return state();
      case "listStyles": return { type: "styleList", revision: e.rev, styles: [] };
      default: return state();
    }
  };
  return e;
}

function makeTransport(engine) {
  const t = { gate: null, loseReply: null, loseRequest: null, calls: 0 };
  t.request = async (frame, { signal } = {}) => {
    t.calls += 1;
    if (signal?.aborted) throw abortError();
    const req = decodeRichRequest(frame);
    if (t.loseRequest?.(req.command)) throw new Error("request lost");
    const response = engine.handle(req);
    if (t.gate?.when(req.command)) {
      const gate = t.gate;
      gate.hit = true;
      await new Promise((resolve, reject) => { gate.release = resolve; signal?.addEventListener("abort", () => reject(abortError()), { once: true }); });
    }
    if (t.loseReply?.(req.command)) throw new Error("reply lost");
    return encodeRichReply({ docId: 1, requestId: req.requestId, response });
  };
  return t;
}

async function setup() {
  const engine = makeEngine();
  const transport = makeTransport(engine);
  const client = createRichEditorClient({ transport, docId: 1, yieldTurn: turn });
  await client.open({ kind: "blank", profile: "document", data: new Uint8Array(0) });
  await client.whenLayoutComplete();
  return { engine, transport, client, format: createFormatClient(client) };
}


await scenario("FormatState never reaches the page cache and never changes the revision", async () => {
  const { engine, client, format } = await setup();
  const before = client.cache.revision;
  const state = await format.formatState();
  assert.equal(state.type, "formatState");
  assert.equal(client.cache.revision, before);
  assert.equal(engine.rev, 0n);
  assert.equal((await format.listStyles({ filter: "paragraph", usedOnly: true })).styles.length, 0);
});

await scenario("a changing command carries the revision of its turn, not the one of its call", async () => {
  const { engine, transport, client, format } = await setup();
  transport.gate = { when: (c) => c.type === "insertText" };
  const typed = client.command({ type: "insertText", text: "a" });
  await turn(); await turn();
  // Queued behind the keystroke, built before it was applied.
  const applying = format.applyCharFormat({ edits: [bold] });
  await turn();
  transport.gate.release();
  await typed;
  const result = await applying;
  assert.equal(result.outcome, "applied");
  assert.deepEqual(engine.applied, [{ type: "applyCharFormat", at: 1n, id: 1 }]);
});

await scenario("a lost reply: the outcome is the engine's last accepted identifier, and a repeat is refused", async () => {
  const { engine, transport, client, format } = await setup();
  transport.loseReply = (c) => c.type === "applyCharFormat";
  let id;
  await assert.rejects(format.applyCharFormat({ edits: [bold] }), (e) => { id = e.commandId; return e.code === "CLIENT_TRANSPORT" && e.outcome === "applied"; });
  transport.loseReply = null;
  assert.equal(engine.rev, 1n);
  // The client resynchronised before the read-back; its revision is the engine's.
  assert.equal(client.cache.revision, 1n);
  // A blind repeat with the same identifier is refused whatever its guard says, so a toggle cannot flip twice.
  const repeat = { type: "applyCharFormat", baseRevision: NO_REVISION_GUARD, commandId: id, scope: "wordOrTyping", edits: [bold] };
  await assert.rejects(client.command(repeat), (e) => e.code === "RICH_DUPLICATE_COMMAND");
  await assert.rejects(client.command({ ...repeat, baseRevision: 0n }), (e) => e.code === "RICH_DUPLICATE_COMMAND");
  assert.equal(engine.rev, 1n);
  assert.equal(engine.applied.length, 1);
});

await scenario("a keystroke between a lost reply and its read-back does not fool the outcome", async () => {
  const { engine, transport, client, format } = await setup();
  // The command is lost before the engine sees it; a keystroke raises the revision meanwhile.
  transport.loseRequest = (c) => c.type === "applyCharFormat";
  const failing = format.applyCharFormat({ edits: [bold] });
  const typed = client.command({ type: "insertText", text: "a" });
  await assert.rejects(failing, (e) => e.outcome === "not-applied");
  await typed;
  transport.loseRequest = null;
  assert.ok(engine.rev > 0n && engine.applied.length === 0);
  // The reply is lost after the engine applied it; a keystroke follows.
  transport.loseReply = (c) => c.type === "applyCharFormat";
  await assert.rejects(format.applyCharFormat({ edits: [bold] }), (e) => e.outcome === "applied");
  transport.loseReply = null;
  assert.equal(engine.applied.length, 1);
});

await scenario("a command that only sets typing attributes (no revision) is still settled", async () => {
  const { engine, transport, format } = await setup();
  engine.typingOnly = true;
  transport.loseReply = (c) => c.type === "applyCharFormat";
  await assert.rejects(format.applyCharFormat({ edits: [bold] }), (e) => e.outcome === "applied");
  assert.equal(engine.rev, 0n);
  assert.equal(engine.applied.length, 1);
});

await scenario("a lost request: not applied, and the caller may repeat", async () => {
  const { engine, transport, format } = await setup();
  transport.loseRequest = (c) => c.type === "applyParaFormat";
  await assert.rejects(format.applyParaFormat({ edits: [{ attr: "alignment", op: "set", value: "center" }] }), (e) => e.code === "CLIENT_TRANSPORT" && e.outcome === "not-applied");
  transport.loseRequest = null;
  assert.equal(engine.rev, 0n);
  const again = await format.applyParaFormat({ edits: [{ attr: "alignment", op: "set", value: "center" }] });
  assert.equal(again.outcome, "applied");
  assert.equal(engine.rev, 1n);
});

await scenario("a cancelled command resolves its outcome the same way", async () => {
  const { engine, transport, format } = await setup();
  const controller = new AbortController();
  transport.gate = { when: (c) => c.type === "applyStyle" };
  const pending = format.applyStyle({ target: "paragraph", id: "Heading1", signal: controller.signal });
  while (!transport.gate.hit) await turn();
  controller.abort();
  transport.gate.release();
  await assert.rejects(pending, (e) => e.name === "AbortError" && ["applied", "not-applied"].includes(e.outcome));
  assert.equal(engine.rev, engine.applied.length === 1 ? 1n : 0n);
});

await scenario("refusals pass through untouched: read-only, stale, unknown style, not implemented", async () => {
  const { engine, format } = await setup();
  engine.readOnly = true;
  await assert.rejects(format.applyCharFormat({ edits: [bold] }), (e) => e.code === "RICH_READ_ONLY" && e.outcome === undefined);
  assert.equal((await format.formatState()).readOnly, true);
  engine.readOnly = false;
  engine.notImplemented = true;
  await assert.rejects(format.applyParaFormat({ edits: [{ attr: "alignment", op: "clear" }] }), (e) => e.code === "RICH_FORMAT_NOT_IMPLEMENTED");
  await assert.rejects(format.previewStyle({ target: "paragraph", id: "Missing" }), (e) => e.code === "RICH_UNKNOWN_STYLE");
  assert.equal(engine.rev, 0n);
});

await scenario("a preview is a State and is cancelled by an explicit command", async () => {
  const { engine, format } = await setup();
  const shown = await format.previewStyle({ target: "paragraph", id: "Heading1" });
  assert.equal(shown.response.type, "state");
  await format.cancelPreview();
  assert.deepEqual(engine.log.filter((t) => t === "previewStyle").length, 2);
  assert.equal(engine.rev, 0n);
});

// ------------------------------------------------------------------ 3. real WASM

const engineHost = await loadEngine();
const realClient = () => engineHost.open();

await scenario("real WASM: FormatState of a caret, of a selection and of a paragraph range", async () => {
  const { client, format } = await realClient();
  await client.command({ type: "insertText", text: "Hello world\nsecond" });
  await client.command({ type: "setSelection", anchor: 0n, focus: 5n, upstream: false });
  const s = await format.formatState();
  assert.equal(s.revision, client.cache.revision);
  assert.equal(s.collapsed, false);
  assert.equal(s.readOnly, false);
  assert.equal(s.plain, false);
  assert.equal(s.partial, false);
  assert.equal(s.typing, false);
  assert.equal(s.lastCommandId, 0);
  assert.equal(s.chars.font.state, "value");
  assert.equal(typeof s.chars.font.value.requested, "string");
  assert.equal(typeof s.chars.font.value.substituted, "boolean");
  assert.equal(s.chars.size.state, "value");
  assert.ok(Number.isInteger(s.chars.size.value) && s.chars.size.value >= 2);
  assert.deepEqual(s.chars.bold, { state: "value", value: false });
  assert.deepEqual(s.chars.underline, { state: "value", value: "none" });
  assert.equal(s.chars.color.state, "unset");
  assert.deepEqual(s.paragraph.alignment, { state: "value", value: "left" });
  assert.equal(s.paragraph.style.state, "value");
  for (const side of ["top", "left", "bottom", "right", "between", "bar"]) assert.equal(s.paragraph.borders[side].state, "unset");
  await client.command({ type: "setSelection", anchor: 3n, focus: 3n, upstream: false });
  assert.equal((await format.formatState()).collapsed, true);
  await client.command({ type: "setSelection", anchor: 2n, focus: 15n, upstream: false });
  assert.equal((await format.formatState()).chars.bold.state, "value");
  // Reading the state changes nothing: same revision, same answer.
  const again = await format.formatState();
  assert.equal(again.revision, s.revision);
  client.dispose();
});

await scenario("real WASM: ListFonts answers what the editor draws, the Word aliases and the document fonts", async () => {
  const { client, format } = await realClient();
  await client.command({ type: "insertText", text: "abc" });
  const list = await format.listFonts();
  assert.equal(list.revision, client.cache.revision);
  assert.equal(typeof list.themeMajor, "string");
  const carlito = list.families.find((f) => f.name === "Carlito");
  assert.ok(carlito && carlito.bold && carlito.italic && !carlito.substituted && carlito.substitute === "");
  const calibri = list.families.find((f) => f.name === "Calibri");
  assert.ok(calibri && calibri.substituted && calibri.substitute === "Carlito");
  for (const f of list.families) assert.equal(f.substituted, f.substitute !== "");
  client.dispose();
});

await scenario("real WASM: a plain-text document reports it and refuses formatting", async () => {
  const { client, format } = await engineHost.open({ kind: "txt", profile: "txt", data: new TextEncoder().encode("plain text") });
  assert.equal((await format.formatState()).plain, true);
  await assert.rejects(format.applyCharFormat({ edits: [bold] }), (e) => e.code === "RICH_FORMAT_PLAIN_TEXT" && e.outcome === undefined);
  client.dispose();
});

suite.finish("rich-editor-format", 12);
