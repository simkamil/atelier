// SPDX-License-Identifier: AGPL-3.0-or-later
// R7b-2: the frozen vectors of the search, replace, statistics and properties messages of ATED v2,
// written by the Rust tests (crates/core-wasm/src/rich_protocol/find_tests.rs, ATELIER_REGEN_VECTORS=1)
// and checked here against the JS codec: every frame decodes to the described fields, re-encodes to the
// same bytes, refusals carry the same stable code, every truncation is a typed error.
// Run by tests/rich-editor-protocol.mjs and tests/rich-editor-find.mjs.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  decodeRichReply, decodeRichRequest, encodeRichReply, encodeRichRequest, PROPERTY_FIELDS, RICH_LIMITS, RichProtocolError,
} from "../web/rich-editor-protocol.mjs";

const here = (p) => new URL(`./${p}`, import.meta.url);
const encoder = new TextEncoder();
const hex = (bytes) => Buffer.from(bytes).toString("hex");
const fromHex = (h) => Uint8Array.from(Buffer.from(h, "hex"));
const text = (s) => hex(encoder.encode(s));
const FLAGS = ["matchCase", "wholeWord", "ignoreAccents", "matchNbsp"];
const flagsByte = (flags) => FLAGS.reduce((byte, name, bit) => byte | (flags[name] ? 1 << bit : 0), 0);
const orNone = (parts) => (parts.length ? parts.join(";") : "none");
const counts = (c) => [c.words, c.characters, c.charactersWithSpaces, c.paragraphs, c.lines, c.pages].join(",");
const stateText = (s) => `revision=${s.revision} anchor=${s.anchor} focus=${s.focus} doc_len=${s.docLen} modified=${s.modified ? 1 : 0}`;
const date = (d) => (d === null ? "none" : `${d.valid ? 1 : 0}:${text(d.text)}`);
const number = (n) => (n === null ? "none" : String(n));

function describeRequest(c) {
  switch (c.type) {
    case "search": return `Search revision=${c.revision} flags=${flagsByte(c.flags)} query=${text(c.query)}`;
    case "statistics": return "Statistics";
    case "searchRects": return `SearchRects revision=${c.revision} first=${c.firstPage} pages=${c.pageCount} ranges=${orNone(c.ranges.map((r) => `${r.start}-${r.end}`))}`;
    case "replace": return `Replace revision=${c.revision} flags=${flagsByte(c.flags)} range=${c.start}-${c.end} query=${text(c.query)} replacement=${text(c.replacement)}`;
    case "replaceAll": return `ReplaceAll revision=${c.revision} flags=${flagsByte(c.flags)} query=${text(c.query)} replacement=${text(c.replacement)}`;
    case "propertiesRead": return "PropertiesRead";
    case "propertiesApply":
      return `PropertiesApply changes=${orNone(PROPERTY_FIELDS.flatMap((name, i) => (name in c.changes ? [`${i}:${text(c.changes[name])}`] : [])))}`;
    default: throw new Error(`not an R7b-2 command: ${c.type}`);
  }
}

function describeReply(r) {
  switch (r.type) {
    case "error": return `Error code=${r.code}`;
    case "searchResult":
      return `SearchResult revision=${r.revision} total=${r.total} flags=${(r.truncated ? 1 : 0) | (r.totalCapped ? 2 : 0)} items=${orNone(r.items.map((i) => `${i.start}-${i.end}/${text(i.excerpt.text)}/${i.excerpt.start}-${i.excerpt.end}/${(i.excerpt.truncatedBefore ? 1 : 0) | (i.excerpt.truncatedAfter ? 2 : 0)}`))}`;
    case "statistics":
      return `Statistics revision=${r.revision} anchor=${r.anchor} focus=${r.focus} complete=${r.layoutComplete ? 1 : 0} document=${counts(r.document)} selection=${counts(r.selection)}`;
    case "searchRects":
      return `SearchRects revision=${r.revision} rects=${orNone(r.rects.map((x) => `${x.item}:${x.page}:${x.x}:${x.y}:${x.width}:${x.height}`))}`;
    case "replaced": return `Replaced count=${r.count} ${stateText(r.state)}`;
    case "properties": {
      const s = r.snapshot;
      return `Properties revision=${r.revision} editable=${PROPERTY_FIELDS.map((n) => text(s.editable[n])).join(",")} limits=${PROPERTY_FIELDS.map((n) => s.limits[n]).join(",")} `
        + `dates=${[s.created, s.modified, s.lastPrinted].map(date).join(",")} strings=${[s.lastModifiedBy, s.revision, s.application, s.appVersion, s.template].map((v) => (v === null ? "none" : `1:${text(v)}`)).join(",")} `
        + `total_time=${number(s.totalTime)} statistics=${["pages", "words", "characters", "charactersWithSpaces", "lines", "paragraphs"].map((n) => number(s.statistics[n])).join(",")} notices=${s.noticeCount}`;
    }
    case "propertiesApplied":
      return `PropertiesApplied status=${r.status === "applied" ? 0 : 1} changed=${r.changed} field=${r.field === null ? 0 : PROPERTY_FIELDS.indexOf(r.field)} max=${r.max} len=${r.length} ${stateText(r.state)}`;
    default: throw new Error(`not an R7b-2 reply: ${r.type}`);
  }
}

/** Checks every vector of tests/rich-editor-find-vectors.txt; returns the number of rows. */
export function verifyFindVectors() {
  const rows = readFileSync(here("rich-editor-find-vectors.txt"), "utf8")
    .split(/\r?\n/).filter((l) => l && !l.startsWith("#")).map((l) => l.split("\t"));
  const counted = { request: 0, reply: 0, "bad-request": 0, "bad-reply": 0 };
  for (const [kind, name, frameHex, expected] of rows) {
    counted[kind] += 1;
    const bytes = fromHex(frameHex);
    if (kind === "request") {
      const d = decodeRichRequest(bytes);
      assert.equal(`doc=${d.docId} req=${d.requestId} ${describeRequest(d.command)}`, expected, name);
      assert.equal(hex(encodeRichRequest(d)), frameHex, `${name} re-encode`);
    } else if (kind === "reply") {
      const d = decodeRichReply(bytes);
      assert.equal(`doc=${d.docId} req=${d.requestId} ${describeReply(d.response)}`, expected, name);
      assert.equal(hex(encodeRichReply(d)), frameHex, `${name} re-encode`);
    } else {
      const decode = kind === "bad-request" ? decodeRichRequest : decodeRichReply;
      assert.throws(() => decode(bytes), (e) => e instanceof RichProtocolError && e.code === expected, name);
    }
  }
  assert.ok(counted.request >= 8 && counted.reply >= 14 && counted["bad-request"] >= 25 && counted["bad-reply"] >= 16, JSON.stringify(counted));
  for (const [kind, , frameHex] of rows.filter((r) => r[0] === "request" || r[0] === "reply")) {
    const decode = kind === "request" ? decodeRichRequest : decodeRichReply;
    const bytes = fromHex(frameHex);
    for (let cut = 0; cut < bytes.length; cut += 1) assert.throws(() => decode(bytes.subarray(0, cut)), RichProtocolError);
  }
  // Limits written by the encoders and refused by them (never a frame the decoder would reject).
  const request = (command) => encodeRichRequest({ docId: 1, requestId: 1, command });
  const refused = (command, code) => assert.throws(() => request(command), (e) => e instanceof RichProtocolError && e.code === code, code);
  refused({ type: "search", revision: 0n, query: "a".repeat(RICH_LIMITS.maxQueryBytes + 1) }, "RICH_QUERY_TOO_LARGE");
  refused({ type: "replaceAll", revision: 0n, query: "", replacement: "x" }, "RICH_EMPTY_QUERY");
  refused({ type: "replaceAll", revision: 0n, query: "a￼", replacement: "x" }, "RICH_INVALID_TEXT");
  refused({ type: "replaceAll", revision: 0n, query: "a", replacement: "x\r" }, "RICH_INVALID_TEXT");
  refused({ type: "replace", revision: 0n, start: 4n, end: 2n, query: "a", replacement: "x" }, "RICH_INVALID_RANGE");
  refused({ type: "search", revision: 0n, query: "\ud800" }, "RICH_INVALID_UTF8");
  refused({ type: "search", revision: 0n, flags: { fuzzy: true }, query: "a" }, "RICH_INVALID_BODY");
  refused({ type: "search", revision: 0, query: "a" }, "RICH_INVALID_BODY");
  refused({ type: "searchRects", revision: 0n, firstPage: 0, pageCount: 17, ranges: [{ start: 0n, end: 1n }] }, "RICH_TOO_MANY_PAGES");
  refused({ type: "searchRects", revision: 0n, firstPage: 0, pageCount: 1, ranges: [] }, "RICH_INVALID_BODY");
  refused({ type: "propertiesApply", changes: { nickname: "x" } }, "RICH_INVALID_BODY");
  refused({ type: "propertiesApply", changes: { title: "a".repeat(RICH_LIMITS.maxPropertyBytes + 1) } }, "RICH_FRAME_TOO_LARGE");
  assert.ok(request({ type: "search", revision: 0n, query: "a".repeat(RICH_LIMITS.maxQueryBytes) }));
  assert.ok(request({ type: "propertiesApply", changes: {} }));
  return rows.length;
}
