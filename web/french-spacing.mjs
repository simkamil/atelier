// SPDX-License-Identifier: AGPL-3.0-or-later

const NBSP = "\u00a0";
const NNBSP = "\u202f";
const PUNCTUATION = new Set([":", ";", "!", "?"]);
const PUNCTUATION_RUN = new Set([":", ";", "!", "?"]);
const PARAGRAPH_BREAK = new Set(["\r", "\n", "\u2028", "\u2029", "\t"]);

/**
 * Return a replacement for the just-typed scalar when French spacing applies.
 * The snapshot is taken after the raw insertion; offsets in both the snapshot
 * and returned range are UTF-8 byte offsets, as in the editor model.
 */
export function frenchSpacingEdit(
  snapshot,
  insertedText,
  inputType = "insertText",
  enabled = true,
) {
  if (!enabled || inputType !== "insertText" || typeof insertedText !== "string") return null;
  if (!isWellFormedUnicode(insertedText) || [...insertedText].length !== 1) return null;
  if (!snapshot || typeof snapshot.text !== "string" || !isWellFormedUnicode(snapshot.text)) return null;
  if (!Number.isInteger(snapshot.anchor) || snapshot.anchor !== snapshot.focus) return null;

  const text = snapshot.text;
  const caret = utf8IndexAtUtf8Offset(text, snapshot.focus);
  if (caret === null || !text.slice(0, caret).endsWith(insertedText)) return null;
  const insertedStart = caret - insertedText.length;
  const byteAt = index => utf8Length(text.slice(0, index));

  if (insertedText === " " && text.slice(0, caret).endsWith(`«${NBSP} `)) {
    // The opening quote already supplied this spacing; remove only the extra
    // ASCII space the user just entered.
    return { start: byteAt(caret - 1), end: snapshot.focus, text: "" };
  }

  if (insertedText === "«") {
    // Replace the new quote so the caret remains after the generated inner
    // space, even when the user is typing in the middle of existing text.
    return {
      start: byteAt(insertedStart),
      end: snapshot.focus,
      text: `«${NBSP}`,
    };
  }

  const closingQuote = insertedText === "»";
  if (!closingQuote && !PUNCTUATION.has(insertedText)) return null;

  const before = text.slice(0, insertedStart);
  const last = [...before].at(-1);
  if (last === undefined || PARAGRAPH_BREAK.has(last)) return null;
  if (PUNCTUATION_RUN.has(last)) return null;

  const wanted = insertedText === ":" || closingQuote ? NBSP : NNBSP;
  if (last === NBSP || last === NNBSP) return null;

  if (last === " ") {
    const spaceIndex = insertedStart - 1;
    let runStart = spaceIndex;
    while (runStart > 0 && text[runStart - 1] === " ") runStart -= 1;
    const beforeSpace = [...text.slice(0, runStart)].at(-1);
    if (beforeSpace === undefined || PARAGRAPH_BREAK.has(beforeSpace)) return null;
    return {
      start: byteAt(spaceIndex),
      end: snapshot.focus,
      text: `${wanted}${insertedText}`,
    };
  }

  // Other Unicode whitespace is deliberately preserved. Do not add spacing
  // after a tab, line break, or another whitespace character.
  if (/\s/u.test(last)) return null;
  return {
    start: byteAt(insertedStart),
    end: snapshot.focus,
    text: `${wanted}${insertedText}`,
  };
}

function utf8IndexAtUtf8Offset(text, offset) {
  if (offset < 0) return null;
  let bytes = 0;
  for (let index = 0; index < text.length;) {
    if (bytes === offset) return index;
    const codePoint = text.codePointAt(index);
    const width = codePoint > 0xffff ? 2 : 1;
    bytes += codePoint <= 0x7f ? 1 : codePoint <= 0x7ff ? 2 : codePoint <= 0xffff ? 3 : 4;
    index += width;
    if (bytes > offset) return null;
  }
  return bytes === offset ? text.length : null;
}

function utf8Length(text) {
  let bytes = 0;
  for (let index = 0; index < text.length;) {
    const codePoint = text.codePointAt(index);
    bytes += codePoint <= 0x7f ? 1 : codePoint <= 0x7ff ? 2 : codePoint <= 0xffff ? 3 : 4;
    index += codePoint > 0xffff ? 2 : 1;
  }
  return bytes;
}

function isWellFormedUnicode(text) {
  for (let index = 0; index < text.length; index += 1) {
    const unit = text.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = text.charCodeAt(index + 1);
      if (!(next >= 0xdc00 && next <= 0xdfff)) return false;
      index += 1;
    } else if (unit >= 0xdc00 && unit <= 0xdfff) {
      return false;
    }
  }
  return true;
}
