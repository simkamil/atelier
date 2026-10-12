// SPDX-License-Identifier: AGPL-3.0-or-later
import assert from "node:assert/strict";
import { frenchSpacingEdit } from "../web/french-spacing.mjs";

function state(text, caret = Buffer.byteLength(text)) {
  return { text, anchor: caret, focus: caret };
}

function apply(text, edit) {
  assert.ok(edit);
  const bytes = Buffer.from(text, "utf8");
  return Buffer.concat([
    bytes.subarray(0, edit.start),
    Buffer.from(edit.text, "utf8"),
    bytes.subarray(edit.end),
  ]).toString("utf8");
}

function editAfter(text, inserted, inputType = "insertText", enabled = true) {
  return frenchSpacingEdit(state(text), inserted, inputType, enabled);
}

for (const [punctuation, spacing] of [[":", "\u00a0"], [";", "\u202f"], ["!", "\u202f"], ["?", "\u202f"]]) {
  for (const [prefix, expectedPrefix] of [["mot", `mot${spacing}`], ["mot ", `mot${spacing}`]]) {
    const text = `${prefix}${punctuation}`;
    assert.equal(apply(text, editAfter(text, punctuation)), `${expectedPrefix}${punctuation}`);
  }
}

for (const punctuation of [":", ";", "!", "?"]) {
  const expected = punctuation === ":" ? "\u00a0" : "\u202f";
  assert.equal(editAfter(`mot${expected}${punctuation}`, punctuation), null, "preserves correct spacing");
  assert.equal(editAfter(`mot\u2009${punctuation}`, punctuation), null, "preserves other Unicode spaces");
}

for (const boundary of ["\r", "\n", "\r\n", "\u2028", "\u2029", "\t"]) {
  for (const punctuation of [":", ";", "!", "?", "»"]) {
    assert.equal(editAfter(`mot${boundary}${punctuation}`, punctuation), null, `does not cross ${JSON.stringify(boundary)}`);
    assert.equal(editAfter(`mot${boundary} ${punctuation}`, punctuation), null, `does not rewrite line-leading space after ${JSON.stringify(boundary)}`);
  }
}

assert.equal(editAfter(":" , ":"), null, "does not add space at document start");
assert.equal(editAfter("mot?!", "!"), null, "does not alter a punctuation sequence");
assert.equal(editAfter("mot ?!", "!"), null, "does not alter a punctuation sequence after a space");
for (const punctuation of [":", ";", "!", "?"]) {
  const spacing = punctuation === ":" ? "\u00a0" : "\u202f";
  const text = `«mot»${punctuation}`;
  assert.equal(apply(text, editAfter(text, punctuation)), `«mot»${spacing}${punctuation}`, "spaces punctuation after a closing quote");
}
{
  const text = "mot   :";
  assert.equal(apply(text, editAfter(text, ":")), "mot  \u00a0:", "converts only the adjacent ASCII space");
}
for (const prefix of ["\n  ", "\r\n   ", "\u2028  ", "\u2029   ", "\t  "]) {
  assert.equal(editAfter(`${prefix}:`, ":"), null, "preserves leading ASCII-space runs after a boundary");
}

{
  const text = "«mot";
  const caret = Buffer.byteLength("«");
  const edit = frenchSpacingEdit({ text, anchor: caret, focus: caret }, "«");
  assert.equal(apply(text, edit), "«\u00a0mot");
}
{
  const text = "«\u00a0 ";
  const edit = editAfter(text, " ");
  assert.deepEqual(edit, { start: Buffer.byteLength("«\u00a0"), end: Buffer.byteLength(text), text: "" });
  assert.equal(apply(text, edit), "«\u00a0");
}
assert.equal(apply("«mot»", editAfter("«mot»", "»")), "«mot\u00a0»");
assert.equal(apply("«mot »", editAfter("«mot »", "»")), "«mot\u00a0»");
assert.equal(editAfter("«\u00a0»", "»"), null, "preserves correct closing quote spacing");
assert.equal(editAfter("»", "»"), null, "does not add a quote space at document start");

{
  const text = "Cafe\u0301🙂?";
  const edit = editAfter(text, "?");
  assert.equal(edit.start, Buffer.byteLength("Cafe\u0301🙂"));
  assert.equal(edit.end, Buffer.byteLength(text));
  assert.equal(apply(text, edit), "Cafe\u0301🙂\u202f?");
}
assert.equal(frenchSpacingEdit({ text: "é?", anchor: 2, focus: 2 }, "?"), null, "rejects an offset inside a UTF-8 scalar");
assert.equal(frenchSpacingEdit({ text: "mot?", anchor: 2, focus: 2 }, "?"), null, "rejects a suffix mismatch");
assert.equal(frenchSpacingEdit({ text: "mot?", anchor: 2, focus: 4 }, "?"), null, "rejects a noncollapsed caret");
assert.equal(frenchSpacingEdit({ text: "mot?", anchor: 3, focus: 3 }, "?"), null, "rejects a caret before inserted text");

for (const inputType of ["insertFromPaste", "insertFromDrop", "insertFromYank", "insertReplacementText", "insertCompositionText"]) {
  assert.equal(editAfter("mot?", "?", inputType), null, `exempts ${inputType}`);
}
assert.equal(editAfter("mot?!", "?!"), null, "exempts multi-scalar insertion");
assert.equal(editAfter("mot?", "?", "insertText", false), null, "respects disabled option");
assert.equal(editAfter("mot\ud800?", "?"), null, "rejects malformed UTF-16 input");

console.log("French spacing helper: punctuation, quotes, UTF-8 offsets, boundaries, and exemptions passed.");
