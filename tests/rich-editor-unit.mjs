// SPDX-License-Identifier: AGPL-3.0-or-later
// R7a : fonctions pures de l'éditeur v2 de l'interface et choix du moteur. `node --test tests/rich-editor-unit.mjs`.
import assert from "node:assert/strict";
import test from "node:test";
import { chooseEngine, ENGINE_STORAGE_KEY } from "../web/engine-choice.mjs";
import { countDocxPictures, EditorError, mirrorWindow, RICH_CAPABILITIES, sanitizeEditText, toEditorError } from "../web/rich-editor.mjs";
import { RichClientError } from "../web/rich-editor-client.mjs";
import { RichProtocolError } from "../web/rich-editor-protocol.mjs";

test("engine choice: v1 by default, the URL wins, then the persisted option", () => {
  const storage = value => ({ getItem: key => (key === ENGINE_STORAGE_KEY ? value : null) });
  assert.equal(chooseEngine({ search: "", storage: null }), "v1");
  assert.equal(chooseEngine({ search: "?engine=v2", storage: null }), "v2");
  assert.equal(chooseEngine({ search: "?a=1&engine=v2", storage: storage(null) }), "v2");
  assert.equal(chooseEngine({ search: "", storage: storage("v2") }), "v2");
  assert.equal(chooseEngine({ search: "?engine=v1", storage: storage("v2") }), "v1", "the URL forces v1 even when v2 is persisted");
  assert.equal(chooseEngine({ search: "?engine=v3", storage: storage("v1") }), "v1", "an unknown value falls back to v1");
  assert.equal(chooseEngine({ search: "?engine=v3", storage: storage("v2") }), "v2", "an unknown URL value defers to the persisted option");
  assert.equal(chooseEngine({ search: "", storage: storage("v3") }), "v1");
  assert.equal(chooseEngine({ search: "", storage: { getItem() { throw new Error("blocked"); } } }), "v1", "blocked storage keeps v1");
});

test("edit text is normalised to what the engine accepts", () => {
  assert.equal(sanitizeEditText("a\r\nb\rc\nd"), "a\nb\nc\nd");
  assert.equal(sanitizeEditText("x\u0000y\u0007z\u007f\ufffc!"), "xyz!");
  assert.equal(sanitizeEditText("tab\there"), "tab\there");
  assert.equal(sanitizeEditText("\ud800 seul"), " seul");
  assert.equal(sanitizeEditText("𝄞 é"), "𝄞 é");
});

test("client and codec failures map to the codes the interface knows", () => {
  const code = (error, context) => toEditorError(error, context).code;
  assert.equal(code(new RichClientError("RICH_READ_ONLY")), "EDITOR_READ_ONLY");
  assert.equal(code(new RichClientError("RICH_STALE_REVISION")), "EDITOR_STALE_EDIT");
  assert.equal(code(new RichClientError("RICH_STALE_REVISION"), "save"), "EDITOR_STALE_SAVE");
  assert.equal(code(new RichClientError("RICH_IMAGE_INVALID")), "EDITOR_IMAGE_UNSUPPORTED");
  assert.equal(code(new RichClientError("RICH_LIMIT"), "image"), "EDITOR_IMAGE_LIMIT");
  assert.equal(code(new RichClientError("RICH_LIMIT")), "RICH_LIMIT");
  assert.equal(code(new RichClientError("CLIENT_CLOSED")), "EDITOR_CLOSED");
  assert.equal(code(new RichProtocolError("RICH_INVALID_TEXT")), "EDITOR_UNSUPPORTED_INPUT");
  assert.equal(code(new RichClientError("RICH_REVISIONS_UNSUPPORTED")), "RICH_REVISIONS_UNSUPPORTED");
  assert.equal(code(new Error("boom")), "EDITOR_FAILED");
  const typed = new EditorError("EDITOR_BUSY");
  assert.equal(toEditorError(typed), typed);
  const abort = new DOMException("x", "AbortError");
  assert.equal(toEditorError(abort), abort, "cancellation stays an AbortError");
});

/** Minimal ZIP: stored entries, local headers skipped (only the central directory is read). */
function zipWith(names) {
  const central = [];
  for (const name of names) {
    const bytes = Buffer.from(name);
    const header = Buffer.alloc(46 + bytes.length);
    header.writeUInt32LE(0x02014b50, 0); header.writeUInt16LE(bytes.length, 28); bytes.copy(header, 46);
    central.push(header);
  }
  const directory = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0); end.writeUInt16LE(names.length, 8); end.writeUInt16LE(names.length, 10);
  end.writeUInt32LE(directory.length, 12); end.writeUInt32LE(0, 16);
  return new Uint8Array(Buffer.concat([directory, end]));
}

test("pictures of a DOCX package are counted from the ZIP directory", () => {
  assert.equal(countDocxPictures(zipWith(["[Content_Types].xml", "word/document.xml"])), 0);
  assert.equal(countDocxPictures(zipWith(["word/document.xml", "word/media/image1.png", "word/media/image2.jpeg", "word/_rels/document.xml.rels"])), 2);
  assert.equal(countDocxPictures(new Uint8Array([1, 2, 3])), 0, "garbage is zero, never an exception");
  assert.equal(countDocxPictures(new Uint8Array(0)), 0);
});

test("the mirror takes role and inline kind numbers, the codec names them", () => {
  const window = mirrorWindow({ revision: 3n, items: [{ paraStart: 0n, paraEnd: 3n, role: "heading", level: 1, label: "", text: "\ufffc", inlines: [{ offset: 0n, kind: "symbol", value: "§" }] },
    { paraStart: 4n, paraEnd: 4n, role: "opaque", level: 0, label: "", text: "", inlines: [] }] });
  assert.equal(window.items[0].role, 1);
  assert.equal(window.items[0].inlines[0].kind, 3);
  assert.equal(window.items[1].role, 4);
  assert.equal(window.revision, 3n);
});

test("capabilities are frozen and list what v2 offers", () => {
  assert.ok(Object.isFrozen(RICH_CAPABILITIES));
  assert.deepEqual(Object.keys(RICH_CAPABILITIES).sort(), ["preview", "properties", "replace", "search", "statistics", "textDragDrop"]);
  // R7b-2 and R7c: search, replace, statistics, properties and text drag and drop work in v2; only the DOCX preview is absent.
  for (const name of ["search", "replace", "statistics", "properties", "textDragDrop"]) assert.equal(RICH_CAPABILITIES[name], true, name);
  assert.equal(RICH_CAPABILITIES.preview, false, "preview");
});

test("R7b-2: the codes of queries and replacements keep the names of the plain editor", () => {
  const code = (error, context) => toEditorError(error, context).code;
  assert.equal(code({ code: "RICH_STALE_REVISION" }, "query"), "EDITOR_STALE_QUERY");
  assert.equal(code({ code: "RICH_STALE_REVISION" }, "save"), "EDITOR_STALE_SAVE");
  assert.equal(code({ code: "RICH_STALE_REVISION" }), "EDITOR_STALE_EDIT");
  assert.equal(code({ code: "RICH_STALE_MATCH" }), "EDITOR_STALE_MATCH");
  assert.equal(code({ code: "RICH_EMPTY_QUERY" }), "EDITOR_EMPTY_QUERY");
  assert.equal(code({ code: "RICH_TOO_MANY_MATCHES" }), "EDITOR_TOO_MANY_MATCHES");
  assert.equal(code({ code: "RICH_READ_ONLY" }), "EDITOR_READ_ONLY");
  assert.equal(code(new RichProtocolError("RICH_QUERY_TOO_LARGE")), "MESSAGE_TOO_LARGE", "the codec refuses before sending");
  assert.equal(code({ code: "RICH_PROPERTIES_FAILED" }), "RICH_PROPERTIES_FAILED");
});
