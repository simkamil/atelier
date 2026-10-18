// SPDX-License-Identifier: AGPL-3.0-or-later
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { ABOUT_LICENSES } from "../web/about-licenses.mjs";
import { SYMBOL_CATALOGUE } from "../web/symbol-catalogue.mjs";
import { BUILD_INFO } from "../web/build-info.mjs";

const source = path => readFile(new URL(`../${path}`, import.meta.url), "utf8").then(text => text.replace(/\r\n/g, "\n"));
assert.equal(ABOUT_LICENSES.agpl, await source("LICENSE"), "full original AGPL text is distributed");
assert.equal(ABOUT_LICENSES.unicode, await source("data/ucd/17.0.0/LICENSE.txt"));
assert.deepEqual(ABOUT_LICENSES.fonts.map(font => font.name), ["Carlito", "Caladea", "Liberation"]);
for (const font of ABOUT_LICENSES.fonts) assert.equal(font.text, await source(`data/fonts/licenses/${font.name}-OFL.txt`));
const unicode = new Map((await source("data/ucd/17.0.0/UnicodeData.txt")).trim().split("\n").map(line => {
  const fields = line.split(";"); return [Number.parseInt(fields[0], 16), fields[1]];
}));
assert.equal(SYMBOL_CATALOGUE.font, "Carlito");
assert.equal(SYMBOL_CATALOGUE.unicodeVersion, "17.0.0");
assert(SYMBOL_CATALOGUE.symbols.length > 2000);
const used = new Set();
for (const entry of SYMBOL_CATALOGUE.symbols) {
  assert.equal(entry.name, unicode.get(entry.codePoint), `U+${entry.codePoint.toString(16)} has its original Unicode name`);
  assert(!used.has(entry.codePoint)); used.add(entry.codePoint);
  const block = SYMBOL_CATALOGUE.blocks.find(block => block.id === entry.block);
  assert(block && entry.codePoint >= block.start && entry.codePoint <= block.end);
}
for (const code of [0x2014, 0x2013, 0xa0, 0x202f, 0xa9, 0xae, 0x2122, 0xa7, 0xb6, 0x2026]) assert(used.has(code), `special U+${code.toString(16)}`);
assert(!used.has(0x2011), "the cmap catalogue must not pretend Carlito covers U+2011; the special-character path uses the tested hyphen outline fallback");
const version = (await source("Cargo.toml")).match(/\[workspace\.package\][\s\S]*?version\s*=\s*"([^"]+)"/)[1];
assert.equal(BUILD_INFO.version, version);
assert.equal(BUILD_INFO.buildDate, null, "development fallback must not invent a compilation date");
assert.equal(BUILD_INFO.sourceUrl, "https://github.com/simkamil/atelier");
console.log(JSON.stringify({ status: "passed", symbols: used.size, unicodeNamesMatched: used.size, completeLicenses: 5, version }));
