// SPDX-License-Identifier: AGPL-3.0-or-later
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  I18nError, analyzeMessage, compileMessage, convertLength, createI18n, getLocaleData, parseCatalog,
  pluralCategory, pluralSelector, registerLocale, verifyPluralRules,
} from "../web/i18n.mjs";
import {
  analyzeHtml, analyzeScript, checkRepository, compareToBaseline, formatBaseline, looksHuman, parseBaseline, tokenize,
} from "./check-i18n.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const NN = String.fromCharCode(0x202f); // narrow no-break space: thousands, percent and unit separator
const NB = String.fromCharCode(0x00a0); // no-break space must never appear in our output
const checks = [];
function test(name, body) {
  try { body(); } catch (error) { error.message = `${name}: ${error.message}`; throw error; }
  checks.push(name);
}
const throwsCode = (body, code) => assert.throws(body, error => error instanceof I18nError && error.code === code);
const make = (catalog, options = {}) => createI18n({ catalog, ...options });

// ---------------------------------------------------------------------------------------------
// Catalogue
// ---------------------------------------------------------------------------------------------

test("catalogue: key=value, comments, blank lines, first '=' splits, CRLF and BOM", () => {
  const parsed = parseCatalog(`${String.fromCharCode(0xfeff)}# comment\r\n\r\na=1\r\nb=x=y=z\r\n  \r\nc=\r\n# d=4\r\ne= padded \r\n`);
  assert.deepEqual(parsed.problems, []);
  assert.deepEqual([...parsed.entries].map(([key, entry]) => [key, entry.value, entry.line]), [
    ["a", "1", 3], ["b", "x=y=z", 4], ["c", "", 6], ["e", " padded ", 8],
  ]);
});

test("catalogue: invalid lines and keys are reported, never skipped silently", () => {
  const parsed = parseCatalog("ok=1\nno separator\n=empty key\nbad key=1\n é=1\nlate=2\n");
  assert.deepEqual(parsed.problems.map(problem => [problem.code, problem.line]), [
    ["invalid-line", 2], ["invalid-key", 3], ["invalid-key", 4], ["invalid-key", 5],
  ]);
  assert.deepEqual([...parsed.entries.keys()], ["ok", "late"]);
});

test("catalogue: duplicate keys are reported with both lines and the last one wins", () => {
  const parsed = parseCatalog("a=first\nb=2\na=second\n");
  assert.deepEqual(parsed.problems.map(problem => [problem.code, problem.line, problem.key]), [["duplicate-key", 3, "a"]]);
  assert.match(parsed.problems[0].message, /line 1/);
  assert.equal(parsed.entries.get("a").value, "second");
});

test("catalogue: an invalid message is reported with its line", () => {
  const parsed = parseCatalog("a=ok\nb={n, plural, one {x}}\nc=trop }\nd=\\q\n");
  assert.deepEqual(parsed.problems.map(problem => [problem.code, problem.line, problem.key]), [
    ["invalid-message", 2, "b"], ["invalid-message", 3, "c"], ["invalid-message", 4, "d"],
  ]);
});

test("catalogue: the real fr-FR catalogue loads exactly like the historic loader", () => {
  const text = readFileSync(join(root, "web/i18n/fr-FR.txt"), "utf8");
  const legacy = new Map();
  for (const line of text.split(/\r?\n/)) { // loader of web/app.mjs before US-102
    if (!line || line.startsWith("#")) continue;
    const separator = line.indexOf("=");
    if (separator > 0) legacy.set(line.slice(0, separator), line.slice(separator + 1));
  }
  const parsed = parseCatalog(text);
  assert.deepEqual([...parsed.entries].map(([key, entry]) => [key, entry.value]), [...legacy]);
  assert.deepEqual(parsed.problems.map(problem => problem.code), ["duplicate-key", "duplicate-key", "duplicate-key", "duplicate-key"]);
  assert.deepEqual(parsed.problems.map(problem => problem.key), ["toolbar-label", "workspace-label", "pages-label", "canvas-label"]);
  assert.equal(parsed.entries.size, 85);
});

test("catalogue: the 80 historic keys keep their values, the examples render", () => {
  const parsed = parseCatalog(readFileSync(join(root, "web/i18n/fr-FR.txt"), "utf8"));
  assert.equal(parsed.entries.get("action-open").value, "Ouvrir…");
  assert.equal(parsed.entries.get("status-opened").value, "Document ouvert.");
  const i18n = createI18n({ catalog: parsed });
  assert.equal(i18n.t("i18n.words-count", { n: 1 }), "1 mot");
  assert.equal(i18n.t("i18n.words-count", { n: 1234 }), `1${NN}234 mots`);
  assert.equal(i18n.t("i18n.pages-count", { n: 1000000 }), `1${NN}000${NN}000 de pages`);
  assert.equal(i18n.t("i18n.page-of", { page: 3, total: 12 }), "Page 3 sur 12");
  assert.equal(i18n.t("i18n.search-occurrences", { n: 0 }), "Aucune occurrence");
  assert.equal(i18n.t("i18n.search-occurrences", { n: 2 }), "2 occurrences");
  assert.equal(i18n.t("i18n.file-too-large", { size: i18n.formatFileSize(3e6), limit: i18n.formatFileSize(131072, { binary: true }) }),
    `Le fichier (3${NN}Mo) dépasse la taille maximale de 128${NN}Kio.`);
  assert.equal(i18n.issues.length, 0);
});

// ---------------------------------------------------------------------------------------------
// Messages : échappements, paramètres
// ---------------------------------------------------------------------------------------------

test("escapes: \\n, \\\\, \\{, \\}, \\# and plain text", () => {
  const i18n = make(new Map([["a", { value: String.raw`ligne1\nligne2` }], ["b", { value: String.raw`C:\\dir` }], ["c", { value: String.raw`\{x\} et \# hors pluriel # aussi` }]]));
  assert.equal(i18n.t("a"), "ligne1\nligne2");
  assert.equal(i18n.t("b"), String.raw`C:\dir`);
  assert.equal(i18n.t("c"), "{x} et # hors pluriel # aussi");
  assert.equal(make("d={n, plural, other {\\# et #}}").t("d", { n: 2 }), "# et 2");
});

test("escapes: unknown escape and dangling backslash are syntax errors", () => {
  assert.match(analyzeMessage(String.raw`a\qb`).error, /unknown escape/);
  assert.match(analyzeMessage("fin\\").error, /dangling backslash/);
  assert.match(analyzeMessage("a } b").error, /unbalanced/);
  assert.match(analyzeMessage("a { b").error, /expected/);
  assert.match(analyzeMessage("{").error, /invalid parameter name/);
  assert.match(analyzeMessage("{1x}").error, /invalid parameter name/);
});

test("parameters: named substitution, repeated and unused", () => {
  const i18n = make("a=Bonjour {nom}, {nom} !\nb=Sans paramètre");
  assert.equal(i18n.t("a", { nom: "Camille" }), "Bonjour Camille, Camille !");
  assert.equal(i18n.t("b", { nom: "ignoré" }), "Sans paramètre");
  assert.equal(i18n.t("a", { nom: "A", autre: "B" }), "Bonjour A, A !");
  assert.deepEqual([...compileMessage("{a} {b} {n, plural, other {{c}}}").params].sort(), ["a", "b", "c", "n"]);
});

test("parameters: numbers are formatted by the locale, strings are verbatim", () => {
  const i18n = make("a={n} fichiers / {s}");
  assert.equal(i18n.t("a", { n: 1234567.5, s: "1234567.5" }), `1${NN}234${NN}567,5 fichiers / 1234567.5`);
  assert.equal(i18n.t("a", { n: 12345678901234567890n, s: "" }), `12${NN}345${NN}678${NN}901${NN}234${NN}567${NN}890 fichiers / `);
});

test("parameters: values are never interpreted (markup, braces, escapes, #)", () => {
  const i18n = make("a=<{x}>\nb={n, plural, other {# : {x}}}");
  const hostile = "<img src=x onerror=alert(1)> {y} \\n # {n, plural, other {z}}";
  assert.equal(i18n.t("a", { x: hostile }), `<${hostile}>`);
  assert.equal(i18n.t("b", { n: 3, x: "{n}" }), "3 : {n}");
});

test("parameters: missing parameter, production fallback is visible and reported", () => {
  const seen = [];
  const i18n = make("a=Page {page} sur {total}", { onIssue: issue => seen.push(issue) });
  assert.equal(i18n.t("a", { page: 2 }), "Page 2 sur {total}");
  assert.equal(i18n.t("a"), "Page {page} sur {total}");
  assert.equal(i18n.t("a", { page: null, total: undefined }), "Page {page} sur {total}");
  assert.equal(i18n.t("a", { toString: 1 }), "Page {page} sur {total}", "inherited properties are not parameters");
  assert.deepEqual(seen.map(issue => issue.message), ["missing parameter total", "missing parameter page"]);
  assert.equal(i18n.issues.length, 2, "identical issues are reported once");
});

test("parameters: missing parameter throws in development mode", () => {
  const i18n = make("a=Page {page}", { dev: true, onIssue() {} });
  throwsCode(() => i18n.t("a", {}), "missing-param");
  assert.equal(i18n.t("a", { page: 1 }), "Page 1");
  throwsCode(() => i18n.t("a", { page: {} }), "invalid-param");
});

test("parameters: console reporting in development, silence in production", () => {
  const original = console.warn;
  const lines = [];
  console.warn = text => lines.push(text);
  try {
    make("a=x", { dev: true }).t("absent");
    make("a=x").t("absent");
  } finally { console.warn = original; }
  assert.equal(lines.length, 1);
  assert.match(lines[0], /^\[i18n\] missing-key absent/);
});

test("missing key: visible marker, report, never an exception", () => {
  const seen = [];
  const i18n = make("a=x", { dev: true, onIssue: issue => seen.push(issue) });
  assert.equal(i18n.t("nope"), "⟦nope⟧");
  assert.equal(i18n.t("nope"), "⟦nope⟧");
  assert.equal(i18n.t(undefined), "⟦undefined⟧");
  assert.equal(i18n.t(42), "⟦42⟧");
  assert.deepEqual(seen.map(issue => issue.code), ["missing-key", "missing-key", "missing-key"]);
  assert.equal(i18n.has("a"), true);
  assert.equal(i18n.has("nope"), false);
  const broken = make("a={n, plural, one {x}}", { onIssue: issue => seen.push(issue) });
  assert.equal(broken.t("a", { n: 1 }), "⟦a⟧");
  assert.equal(seen.at(-1).code, "invalid-message");
  assert.equal(make("").t("__proto__"), "⟦__proto__⟧");
  assert.equal(make("").t("constructor"), "⟦constructor⟧");
});

test("catalog sources: text, parsed catalogue, Map and object", () => {
  assert.equal(make("a=1").t("a"), "1");
  assert.equal(make(parseCatalog("a=2")).t("a"), "2");
  assert.equal(make(new Map([["a", "3"]])).t("a"), "3");
  assert.equal(make({ a: "4" }).t("a"), "4");
  assert.equal(make().t("a"), "⟦a⟧");
});

// ---------------------------------------------------------------------------------------------
// Pluriels
// ---------------------------------------------------------------------------------------------

const PLURAL = "p={n, plural, one {# page} many {# de pages} other {# pages}}";

test("plural: French categories on boundary values (pinned, independent of the reference table)", () => {
  const expected = new Map([
    [0, "one"], [1, "one"], [0.5, "one"], [1.5, "one"], [1.99, "one"], [2, "other"], [2.5, "other"], [3, "other"],
    [21, "other"], [100, "other"], [999999, "other"], [1000000, "many"], [1000001, "other"], [1500000, "other"],
    [2000000, "many"], [10000000, "many"], [1000000000, "many"],
    [-0, "one"], [-1, "one"], [-1.5, "one"], [-2, "other"], [-1000000, "many"], [-1000001, "other"],
  ]);
  for (const [value, category] of expected) {
    assert.equal(pluralCategory(value, "fr-FR"), category, `category of ${value}`);
    assert.equal(pluralSelector("fr-FR")(value), category);
  }
});

test("plural: rendering for every category, '#' is the formatted number", () => {
  const i18n = make(PLURAL);
  const render = n => i18n.t("p", { n });
  assert.equal(render(0), "0 page");
  assert.equal(render(1), "1 page");
  assert.equal(render(1.5), "1,5 page");
  assert.equal(render(2), "2 pages");
  assert.equal(render(1000), `1${NN}000 pages`);
  assert.equal(render(1000000), `1${NN}000${NN}000 de pages`);
  assert.equal(render(2000000), `2${NN}000${NN}000 de pages`);
  assert.equal(render(1000001), `1${NN}000${NN}001 pages`);
  assert.equal(render(-1), "-1 page");
  assert.equal(render(-2), "-2 pages");
  assert.equal(render(-1000000), `-1${NN}000${NN}000 de pages`);
  assert.equal(render("3"), "3 pages", "numeric strings select by value");
  assert.equal(render(2n), "2 pages");
});

test("plural: a missing 'many' branch falls back to 'other'", () => {
  assert.equal(make("p={n, plural, one {# page} other {# pages}}").t("p", { n: 1000000 }), `1${NN}000${NN}000 pages`);
});

test("plural: exact values win, nesting, surrounding text and several plurals", () => {
  const i18n = make("a={n, plural, =0 {aucune} =1 {une seule} one {# (un)} other {#}} page\nb={a, plural, one {# fichier} other {# fichiers}} dans {b, plural, one {# dossier} other {# dossiers}}\nc={n, plural, one {{n, plural, one {# interne} other {#}}} other {# autres}}");
  assert.equal(i18n.t("a", { n: 0 }), "aucune page");
  assert.equal(i18n.t("a", { n: 1 }), "une seule page");
  assert.equal(i18n.t("a", { n: 1.5 }), "1,5 (un) page");
  assert.equal(i18n.t("a", { n: 5 }), "5 page");
  assert.equal(i18n.t("b", { a: 1, b: 3 }), "1 fichier dans 3 dossiers");
  assert.equal(i18n.t("c", { n: 1 }), "1 interne");
});

test("plural: decimals use the requested rounding of '#' only through the number formatter", () => {
  assert.equal(make("p={n, plural, other {# x}}").t("p", { n: 1 / 3 }), "0,333 x");
});

test("plural: invalid syntax is rejected with a precise message", () => {
  const error = source => analyzeMessage(source).error;
  assert.match(error("{n, plural, one {# a}}"), /needs an 'other'/);
  assert.match(error("{n, plural, few {a} oops {b} other {c}}"), /invalid plural selector "oops"/);
  assert.match(error("{n, plural, other {a} other {b}}"), /duplicate plural selector/);
  assert.match(error("{n, plural, other {a}"), /missing '}'/);
  assert.match(error("{n, plural, other a}"), /expected '\{'/);
  assert.match(error("{n, plural other {a}}"), /expected ','/);
  assert.match(error("{n, select, a {x} other {y}}"), /unsupported argument type "select"/);
  assert.match(error("{n,}"), /unsupported argument type ""/);
  assert.match(error("{n, plural, =x {a} other {b}}"), /invalid plural selector/);
  assert.equal(error("{n, plural, =0 {a} one {b} many {c} other {d}}"), undefined);
});

test("plural: categories French never produces are warned about", () => {
  assert.deepEqual(analyzeMessage("{n, plural, few {a} two {b} other {c}}").warnings.length, 2);
  assert.deepEqual(analyzeMessage("{n, plural, one {a} many {b} other {c}}").warnings, []);
  assert.equal(analyzeMessage("{n, plural, few {a} other {c}}", { locale: "xx-XX" }).warnings.length, 0, "unknown locale: no claim");
});

test("plural: Intl.PluralRules is verified against the reference and replaced when it disagrees", () => {
  const report = verifyPluralRules("fr-FR");
  assert.equal(report.available, true);
  assert.deepEqual(report.mismatches, [], "this Intl agrees with CLDR French rules");
  assert.equal(new Intl.PluralRules("fr-FR").select(1000000), "many", "Intl and the reference agree on many");
  registerLocale("fr-BE", { ...getLocaleData("fr-FR"), plural: () => "other" }); // deliberately wrong reference
  const bad = verifyPluralRules("fr-BE");
  assert.equal(bad.ok, false);
  assert.ok(bad.mismatches.some(m => m.value === 1 && m.intl === "one" && m.expected === "other"));
  assert.equal(pluralSelector("fr-BE")(1), "other", "the reference replaces a disagreeing Intl");
  assert.equal(verifyPluralRules("xx-invalid-locale-tag-").available, false);
});

// ---------------------------------------------------------------------------------------------
// Formatage fr-FR
// ---------------------------------------------------------------------------------------------

const fr = createI18n({ locale: "fr-FR" });
const PARIS = { timeZone: "Europe/Paris" };

test("format: numbers (grouping U+202F, decimal comma, digits, sign)", () => {
  assert.equal(fr.formatNumber(0), "0");
  assert.equal(fr.formatNumber(999), "999");
  assert.equal(fr.formatNumber(1000), `1${NN}000`);
  assert.equal(fr.formatNumber(1234.5), `1${NN}234,5`);
  assert.equal(fr.formatNumber(1234567.891), `1${NN}234${NN}567,891`);
  assert.equal(fr.formatNumber(1234567.8915), `1${NN}234${NN}567,892`);
  assert.equal(fr.formatNumber(1234.5, { decimals: 2 }), `1${NN}234,50`);
  assert.equal(fr.formatNumber(2, { decimals: 1 }), "2,0");
  assert.equal(fr.formatNumber(2, { maxDecimals: 1 }), "2");
  assert.equal(fr.formatNumber(2.25, { maxDecimals: 1 }), "2,3");
  assert.equal(fr.formatNumber(-1234.5), `-1${NN}234,5`);
  assert.equal(fr.formatNumber(-0), "0");
  assert.equal(fr.formatNumber(-0.0001, { maxDecimals: 2 }), "0");
  assert.equal(fr.formatNumber(1234567, { grouping: false }), "1234567");
  assert.equal(fr.formatNumber(12345678901234567890n), `12${NN}345${NN}678${NN}901${NN}234${NN}567${NN}890`);
  assert.equal(fr.formatNumber(NaN), "NaN");
});

test("format: no no-break space (U+00A0) or ICU-dependent separator leaks out", () => {
  const samples = [fr.formatNumber(1234567.5), fr.formatPercent(0.5), fr.formatLength(72), fr.formatFileSize(1536), fr.formatDateTime(Date.UTC(2026, 9, 13, 12, 30), PARIS), fr.formatPercent(-0.25)];
  for (const sample of samples) assert.ok(!sample.includes(NB), JSON.stringify(sample));
});

test("format: percentages (U+202F before the sign)", () => {
  assert.equal(fr.formatPercent(0), `0${NN}%`);
  assert.equal(fr.formatPercent(0.256), `26${NN}%`);
  assert.equal(fr.formatPercent(0.256, { maxDecimals: 1 }), `25,6${NN}%`);
  assert.equal(fr.formatPercent(1), `100${NN}%`);
  assert.equal(fr.formatPercent(12.5), `1${NN}250${NN}%`);
  assert.equal(fr.formatPercent(-0.5), `-50${NN}%`);
  assert.equal(fr.formatPercent(0.5, { decimals: 1 }), `50,0${NN}%`);
});

test("format: dates, times and date-times", () => {
  const moment = Date.UTC(2026, 9, 13, 12, 30);
  assert.equal(fr.formatDate(moment, PARIS), "13 octobre 2026");
  assert.equal(fr.formatDate(new Date(moment), { ...PARIS, style: "short" }), "13/10/2026");
  assert.equal(fr.formatTime(moment, PARIS), "14:30");
  assert.equal(fr.formatDateTime(moment, PARIS), "13 octobre 2026 à 14:30");
  assert.equal(fr.formatDateTime(moment, { ...PARIS, style: "short" }), "13/10/2026 à 14:30");
  assert.equal(fr.formatDate(Date.UTC(2026, 9, 1, 12), PARIS), "1er octobre 2026", "first of the month reads 1er whatever ICU says");
  assert.equal(fr.formatDate(Date.UTC(2026, 9, 2, 12), PARIS), "2 octobre 2026");
  assert.equal(fr.formatDate(Date.UTC(2026, 1, 28, 12), PARIS), "28 février 2026");
  assert.equal(fr.formatDate(Date.UTC(2026, 11, 31, 12), PARIS), "31 décembre 2026");
  assert.equal(fr.formatDate(Date.UTC(2026, 7, 5, 12), { ...PARIS, style: "short" }), "05/08/2026");
  assert.equal(fr.formatTime(Date.UTC(2026, 11, 1, 0, 5), { timeZone: "UTC" }), "00:05", "no 24:05 with hourCycle h23");
  assert.equal(fr.formatTime(Date.UTC(2026, 11, 1, 23, 59), { timeZone: "UTC" }), "23:59");
  assert.equal(fr.formatDate(Date.UTC(2026, 9, 12, 23, 30), PARIS), "13 octobre 2026", "time zone changes the day");
  assert.equal(fr.formatDate(Date.UTC(2026, 9, 12, 23, 30), { timeZone: "UTC" }), "12 octobre 2026");
  throwsCode(() => fr.formatDate(new Date(NaN)), "invalid-date");
  throwsCode(() => fr.formatDate("2026-10-13"), "invalid-date");
  throwsCode(() => fr.formatTime(Infinity), "invalid-date");
});

test("format: lengths from points and twips, with unit-specific rounding", () => {
  assert.equal(fr.formatLength(72, { to: "cm" }), `2,54${NN}cm`);
  assert.equal(fr.formatLength(72, { to: "mm" }), `25,4${NN}mm`);
  assert.equal(fr.formatLength(72, { to: "in" }), `1${NN}po`);
  assert.equal(fr.formatLength(36, { to: "cm" }), `1,27${NN}cm`);
  assert.equal(fr.formatLength(36, { to: "mm" }), `12,7${NN}mm`);
  assert.equal(fr.formatLength(11, { to: "pt" }), `11${NN}pt`);
  assert.equal(fr.formatLength(10.55, { to: "pt" }), `10,6${NN}pt`);
  assert.equal(fr.formatLength(1440, { from: "twip", to: "in" }), `1${NN}po`);
  assert.equal(fr.formatLength(567, { from: "twip", to: "cm" }), `1${NN}cm`);
  assert.equal(fr.formatLength(11906, { from: "twip", to: "mm" }), `210${NN}mm`, "A4 width");
  assert.equal(fr.formatLength(16838, { from: "twip", to: "mm" }), `297${NN}mm`, "A4 height");
  assert.equal(fr.formatLength(595.3, { to: "mm" }), `210${NN}mm`);
  assert.equal(fr.formatLength(25.4, { from: "mm", to: "in" }), `1${NN}po`);
  assert.equal(fr.formatLength(1, { from: "in", to: "cm" }), `2,54${NN}cm`);
  assert.equal(fr.formatLength(72, { to: "in", decimals: 2 }), `1,00${NN}po`);
  assert.equal(fr.formatLength(0, { to: "cm" }), `0${NN}cm`);
  assert.equal(fr.formatLength(-72, { to: "cm" }), `-2,54${NN}cm`);
  assert.equal(fr.formatLength(1700, { from: "twip", to: "cm", maxDecimals: 1 }), `3${NN}cm`);
  assert.equal(fr.formatLength(1700, { from: "twip", to: "cm" }), `3${NN}cm`);
  assert.equal(fr.formatLength(1701, { from: "twip", to: "cm" }), `3${NN}cm`);
  assert.equal(fr.formatLength(1800, { from: "twip", to: "cm" }), `3,18${NN}cm`);
});

test("format: unit conversion is exact enough and rejects unknown units", () => {
  assert.equal(convertLength(1440, "twip", "pt"), 72);
  assert.equal(convertLength(72, "pt", "in"), 1);
  assert.equal(convertLength(2.54, "cm", "in"), 1);
  assert.equal(convertLength(10, "mm", "cm"), 1);
  throwsCode(() => convertLength(1, "px", "cm"), "unit");
  throwsCode(() => convertLength(1, "pt", "furlong"), "unit");
  throwsCode(() => convertLength(1, "toString", "cm"), "unit");
});

test("format: file sizes (decimal Ko/Mo and binary Kio/Mio)", () => {
  const size = (bytes, opts) => fr.formatFileSize(bytes, opts);
  assert.equal(size(0), `0${NN}octet`);
  assert.equal(size(1), `1${NN}octet`);
  assert.equal(size(2), `2${NN}octets`);
  assert.equal(size(999), `999${NN}octets`);
  assert.equal(size(1000), `1${NN}Ko`);
  assert.equal(size(1500), `1,5${NN}Ko`);
  assert.equal(size(9990), `10${NN}Ko`);
  assert.equal(size(12345), `12${NN}Ko`);
  assert.equal(size(999499), `999${NN}Ko`);
  assert.equal(size(999999), `1${NN}Mo`, "rounding never shows 1000 Ko");
  assert.equal(size(1500000), `1,5${NN}Mo`);
  assert.equal(size(2.5e9), `2,5${NN}Go`);
  assert.equal(size(1e12), `1${NN}To`);
  assert.equal(size(1024, { binary: true }), `1${NN}Kio`);
  assert.equal(size(1023, { binary: true }), `1${NN}023${NN}octets`, "below the base the byte count is shown, grouped");
  assert.equal(size(131072, { binary: true }), `128${NN}Kio`);
  assert.equal(size(8 * 1048576, { binary: true }), `8${NN}Mio`);
  assert.equal(size(1048575, { binary: true }), `1${NN}Mio`);
  throwsCode(() => size(-1), "invalid-size");
  throwsCode(() => size(NaN), "invalid-size");
  throwsCode(() => size(Infinity), "invalid-size");
});

test("format: lists", () => {
  assert.equal(fr.formatList([]), "");
  assert.equal(fr.formatList(["a"]), "a");
  assert.equal(fr.formatList(["a", "b"]), "a et b");
  assert.equal(fr.formatList(["a", "b", "c"]), "a, b et c");
  assert.equal(fr.formatList(["a", "b", "c"], { type: "or" }), "a, b ou c");
  assert.equal(fr.formatList(["a", "b"], { type: "or" }), "a ou b");
});

test("locale is a parameter: other locales work through Intl and registerLocale", () => {
  const en = createI18n({ locale: "en-US", catalog: "a={n, plural, one {# page} other {# pages}}" });
  assert.equal(en.formatNumber(1234.5), "1,234.5");
  assert.equal(en.t("a", { n: 1 }), "1 page");
  assert.equal(en.t("a", { n: 2000 }), "2,000 pages");
  assert.equal(en.formatFileSize(1500), "1.5 KB");
  assert.equal(en.formatLength(72, { to: "in" }), "1 in");
  assert.equal(pluralCategory(1000000, "en-US"), "other");
  registerLocale("de-DE", { ...getLocaleData("fr-FR"), group: ".", decimal: ",", unitSpace: " ", plural: n => (n === 1 ? "one" : "other"), pluralCategories: ["one", "other"] });
  const de = createI18n({ locale: "de-DE" });
  assert.equal(de.formatNumber(1234.5), "1.234,5");
  assert.equal(de.formatPercent(0.5), "50 %");
  const unknown = createI18n({ locale: "zz-ZZ", onIssue() {} });
  assert.equal(unknown.locale, "zz-ZZ");
  assert.equal(Object.isFrozen(fr), true);
});

test("fix: a non-numeric plural parameter never shows an invented number", () => {
  const seen = [];
  const i18n = make("p={n, plural, one {# page} other {# pages}}", { onIssue: issue => seen.push(issue.code) });
  for (const bad of [NaN, "abc", true, Infinity, {}, ""]) assert.equal(i18n.t("p", { n: bad }), "{n}", String(bad));
  assert.equal(seen.every(code => code === "invalid-param"), true);
  assert.ok(seen.length >= 1);
  assert.equal(i18n.t("p", { n: "3" }), "3 pages", "a valid numeric string is accepted");
  throwsCode(() => make("p={n, plural, other {#}}", { dev: true, onIssue() {} }).t("p", { n: NaN }), "invalid-param");
});

test("fix: dates without timeZone follow the machine, an invalid zone is an I18nError or a documented fallback", () => {
  const moment = Date.UTC(2026, 9, 1, 23, 30);
  assert.equal(fr.formatDate(moment, { timeZone: "UTC" }), "1er octobre 2026");
  assert.equal(fr.formatDate(moment, PARIS), "2 octobre 2026");
  const local = new Date(moment);
  assert.equal(fr.formatTime(moment), String(local.getHours()).padStart(2, "0") + ":" + String(local.getMinutes()).padStart(2, "0"), "machine zone");
  const seen = [];
  const lenient = createI18n({ onIssue: issue => seen.push(issue.code) });
  assert.equal(lenient.formatDateTime(moment, { timeZone: "Mars/Olympus" }), lenient.formatDateTime(moment));
  assert.deepEqual(seen, ["invalid-timezone"]);
  throwsCode(() => createI18n({ dev: true, onIssue() {} }).formatDate(moment, { timeZone: "Mars/Olympus" }), "invalid-timezone");
});

test("fix: file sizes round half up and agree in number, fractions are rounded", () => {
  assert.equal(fr.formatFileSize(9950), `10${NN}Ko`);
  assert.equal(fr.formatFileSize(9949), `9,9${NN}Ko`);
  assert.equal(fr.formatFileSize(99950), `100${NN}Ko`);
  assert.equal(fr.formatFileSize(1.5), `2${NN}octets`);
  assert.equal(fr.formatFileSize(1.4), `1${NN}octet`);
  assert.equal(fr.formatFileSize(0.4), `0${NN}octet`);
});

test("fix: plural rules stay French for huge values (Intl disagrees from 1e20)", () => {
  for (const [value, category] of [[1e12, "many"], [1e15, "many"], [1e20, "many"], [1e21, "many"], [2 ** 53, "other"], [-1e20, "many"]]) {
    assert.equal(pluralCategory(value, "fr-FR"), category, String(value));
  }
  assert.equal(verifyPluralRules("fr-FR").ok, true);
});

test("fix: a backslash outside the known escapes makes the message invalid", () => {
  assert.equal(make(String.raw`a=C:\temp`).t("a"), "⟦a⟧");
  assert.equal(make(String.raw`a=50\%`).t("a"), "⟦a⟧");
  assert.equal(make(String.raw`a=C:\\temp`).t("a"), String.raw`C:\temp`);
});

test("fix: regex after a condition parenthesis, division after an expression", () => {
  const strings = source => tokenize(source).tokens.filter(token => token.type === "string").map(token => token.value);
  assert.deepEqual(strings('if (x) /ab"c/.test(z); const w = "Après…"'), ["Après…"]);
  assert.deepEqual(strings('while (x) /"/.exec(y); k("a b")'), ["a b"]);
  assert.deepEqual(strings('for (;;) /"/g.test(y); k("a b")'), ["a b"]);
  assert.deepEqual(tokenize("(a + b) / 2 / c").tokens.map(token => token.type), ["punct", "ident", "punct", "ident", "punct", "punct", "number", "punct", "ident"]);
  assert.deepEqual(tokenize("f(x) / 2").tokens.map(token => token.type + ":" + token.value), ["ident:f", "punct:(", "ident:x", "punct:)", "punct:/", "number:2"]);
});

// ---------------------------------------------------------------------------------------------
// Outil de contrôle : analyseur de jetons
// ---------------------------------------------------------------------------------------------

const kinds = source => tokenize(source).tokens.map(token => `${token.type}:${token.value}`);

test("tokenizer: comments hide strings and quotes, strings hide comment markers", () => {
  const { tokens, comments } = tokenize(`const a = "x // y"; // il y a "une chaîne" ici\n/* 'multi\nligne' */ const b = 'z';`);
  assert.deepEqual(tokens.filter(token => token.type === "string").map(token => token.value), ["x // y", "z"]);
  assert.equal(comments.length, 2);
  assert.equal(comments[1].line, 2);
  assert.equal(comments[1].endLine, 3);
  assert.equal(tokens.at(-1).line, 3);
});

test("tokenizer: regular expressions vs division", () => {
  assert.deepEqual(kinds('x = /"un deux"/g.test(y)').slice(0, 3), ["ident:x", "punct:=", 'regex:/"un deux"/g']);
  assert.deepEqual(kinds("a / b / c"), ["ident:a", "punct:/", "ident:b", "punct:/", "ident:c"]);
  assert.deepEqual(kinds("(a) / 2 / 3").filter(item => item.startsWith("regex")), []);
  assert.deepEqual(kinds("return /a b/"), ["ident:return", "regex:/a b/"]);
  assert.deepEqual(kinds("x = /[/]/"), ["ident:x", "punct:=", "regex:/[/]/"], "a slash inside a class does not end the regex");
  assert.deepEqual(kinds("x = /\\//u"), ["ident:x", "punct:=", "regex:/\\//u"]);
  assert.deepEqual(kinds("n++ / 2"), ["ident:n", "punct:++", "punct:/", "number:2"]);
  assert.deepEqual(kinds('f(/"/); "ok"'), ["ident:f", "punct:(", 'regex:/"/', "punct:)", "punct:;", "string:ok"]);
});

test("tokenizer: templates, nesting, escapes and braces", () => {
  const { tokens } = tokenize("const a = `x ${ f({ k: \"v\" }) + `in ${ \"deep\" } ner` } fin\\n`; const b = 1;");
  const outer = tokens.find(token => token.type === "template");
  assert.deepEqual(outer.quasis, ["x ", " fin\n"]);
  assert.equal(outer.exprCount, 1);
  assert.deepEqual(tokens.filter(token => token.type === "template").map(token => token.quasis), [["x ", " fin\n"], ["in ", " ner"]]);
  assert.deepEqual(tokens.filter(token => token.type === "string").map(token => token.value), ["v", "deep"]);
  assert.equal(tokens.at(-1).value, ";");
  assert.equal(tokens.filter(token => token.value === "b").length, 1, "tokens after the template are still read");
  assert.deepEqual(tokenize("`a${1}b${2}c`").tokens.find(token => token.type === "template").quasis, ["a", "b", "c"]);
});

test("tokenizer: string escapes, numbers, private names, line numbers", () => {
  const { tokens } = tokenize('"a\\u00e9\\x41\\n\\"" 0x1F 1_000.5e3 .5 #priv\n\n"x"');
  assert.deepEqual(kinds('"a\\u00e9\\x41\\n\\""')[0], 'string:aéA\n"');
  assert.deepEqual(tokens.map(token => token.type), ["string", "number", "number", "number", "ident", "string"]);
  assert.equal(tokens.at(-1).line, 3);
});

// ---------------------------------------------------------------------------------------------
// Outil de contrôle : heuristique, analyse d'un module, HTML
// ---------------------------------------------------------------------------------------------

test("heuristic: human text vs identifiers, selectors, URLs and class lists", () => {
  for (const human of ["Enregistrer sous", "Pages du document", "Échec", "Le fichier est prêt.", "Où ?", "Atelier · aperçu"]) {
    assert.equal(looksHuman(human), true, human);
  }
  for (const technical of ["", "  ", "keydown", "status-ok", "files.error.too_large", ".ribbon-tab", "#page-title", "[role=\"tab\"]", "button:disabled",
    "a > b", "li:not(.x)", "https://example.org/a b", "/app.mjs", "./x y", "ui-button ui-toggle", "12px", "1 2 3", "us112-csp|a|b", "Atelier", "—", "x", "ab"]) {
    assert.equal(looksHuman(technical), false, technical);
  }
  assert.equal(looksHuman("Fermer", false), false);
  assert.equal(looksHuman("Fermer", true), true, "a display slot accepts one word");
  assert.equal(looksHuman(`px${String.fromCharCode(1)} ${String.fromCharCode(1)}px`), false, "words glued to expressions do not count");
});

const scan = (source, file = "web/x.mjs") => analyzeScript(source, file);
const hardcoded = source => scan(source).findings.filter(finding => finding.category === "hardcoded").map(finding => finding.detail);

test("hardcoded: display slots are flagged", () => {
  assert.deepEqual(hardcoded('node.textContent = "Bonjour le monde";'), ['"Bonjour le monde"']);
  assert.deepEqual(hardcoded('node.textContent = ok ? "Fermer" : "Ouvrir";'), ['"Fermer"', '"Ouvrir"']);
  assert.deepEqual(hardcoded('node.setAttribute("aria-label", "Fermer la fenêtre");'), ['"Fermer la fenêtre"']);
  assert.deepEqual(hardcoded('node.setAttribute("title", "Aide");'), ['"Aide"']);
  assert.deepEqual(hardcoded("node.title = `Salut ${name}, bienvenue`;"), ['"Salut ${…}, bienvenue"']);
  assert.deepEqual(hardcoded('const labels = { label: "Police", id: "font" };'), ['"Police"']);
  assert.deepEqual(hardcoded('node.append("Terminé");'), ['"Terminé"']);
  assert.deepEqual(hardcoded('show("Une phrase de deux mots");'), ['"Une phrase de deux mots"']);
  assert.deepEqual(hardcoded('const m = new Map([["a", "Écho confirmé"]]);'), ['"Écho confirmé"'], "texts in tables are still flagged");
  assert.deepEqual(hardcoded('const FALLBACK = { "status-ok": "Le moteur a répondu." };'), ['"Le moteur a répondu."']);
});

test("hardcoded: false positives avoided (comments, selectors, regex, templates, protocol)", () => {
  const source = [
    '// "Un commentaire avec une chaîne" et des mots',
    '/* Un autre commentaire "entre guillemets" */',
    'import { a } from "./un module.mjs";',
    'const pattern = /un deux trois "quatre"/g;',
    'const ratio = total / count / 2;',
    'document.querySelector(".ribbon-tab .label > span");',
    'document.querySelectorAll("[role=\\"tab\\"] button:not(:disabled)");',
    'el.addEventListener("pointer down", f);',
    'el.classList.add("ui-button primary");',
    'el.className = "ui-button ui-toggle";',
    'el.style.cssText = "display: none";',
    'const px = `${width}px ${height}px`;',
    'const joined = `${a}${b}`;',
    'const css = `calc(100% - ${gap}px)`;',
    'const key = "status-ok", url = "https://example.org/une page";',
    'const table = { "ui-popup": 1, "autre clé": 2 };',
    'if (kind === "une valeur de test") run();',
    'if ("une valeur de test" !== kind) run();',
    'switch (kind) { case "deux mots": break; }',
    'throw new Error("Quelque chose a échoué.");',
    'throw new TypeError(`Valeur invalide ${v}`);',
    'console.warn("Message de diagnostic");',
    'assert(ok, "Message de diagnostic");',
    'assertUniqueKeyTips(tabs, "ribbon tabs");',
    'fail("INVALID", "Truncated payload here.");',
    'worker.postMessage("us112-csp|a b|c");',
    'const wire = obj["deux mots"];',
    'el.setAttribute("data-state", "deux mots");',
    'el.setAttribute("class", "deux mots");',
    'const brand = "Atelier";',
    'const bytes = "ok", empty = "", number = "123 456";',
  ].join("\n");
  assert.deepEqual(hardcoded(source), []);
  assert.deepEqual(scan(source).findings, []);
});

test("hardcoded: exemptions need a reason, on the line or on the comment line above", () => {
  const found = source => scan(source).findings.map(finding => `${finding.category}:${finding.line}`);
  assert.deepEqual(found('x.textContent = "Bonjour le monde"; // i18n-ignore: texte de test'), []);
  assert.deepEqual(found('// i18n-ignore: texte de test\nx.textContent = "Bonjour le monde";'), []);
  assert.deepEqual(found('/* i18n-ignore: texte de test */ x.textContent = "Bonjour le monde";'), []);
  assert.deepEqual(found('// i18n-ignore: texte de test\n\nx.textContent = "Bonjour le monde";'), ["hardcoded:3"], "only the next line is exempt");
  assert.deepEqual(found('x.textContent = "Bonjour le monde";\n// i18n-ignore: trop tard'), ["hardcoded:1"]);
  assert.deepEqual(found('x.textContent = "Bonjour le monde"; // i18n-ignore'), ["invalid-exemption:1", "hardcoded:1"]);
  assert.deepEqual(found('x.textContent = "Bonjour le monde"; // i18n-ignore:   '), ["invalid-exemption:1", "hardcoded:1"]);
  assert.deepEqual(found('const a = `ligne un\nligne deux ${x}`; // i18n-ignore: gabarit multi-ligne'), []);
});

test("keys: every real form of use is recognised", () => {
  const result = scan([
    't("one"); i18n.t("two", { a: 1 });',
    'messages.get("three"); messages.get(ok ? "four" : "five");',
    'messages.get(a === "pas-une-clé" ? "six" : other);',
    'messages.get(`seven`); messages.get(`theme-${value}`);',
    'document.querySelector(\'[data-i18n="eight"]\');',
    'command("id", "nine", "N"); setAttribute("aria-label", t("ten"));',
    'foo(t("eleven"), bar("not-a-key"));',
    'obj.get("not-a-key-either"); const r = messages.get(result.key);',
  ].join("\n"));
  const named = kind => result.uses.filter(use => use.kind === kind).map(use => use.key);
  assert.deepEqual(named("call"), ["one", "two", "three", "four", "five", "six", "seven", "ten", "eleven"]);
  assert.deepEqual(named("selector"), ["eight"]);
  assert.deepEqual(result.prefixes, ["theme-"]);
  assert.ok(named("literal").includes("nine"), "a literal that is a key is an indirect use");
  assert.ok(!named("call").includes("pas-une-clé"), "comparison operands are not keys");
});

test("html: text, attributes, data-i18n fallbacks, comments, scripts", () => {
  const html = [
    "<!doctype html>",
    '<html lang="fr"><head><title>Un titre en dur</title><style>.a::before { content: "Un texte css" }</style></head>',
    "<body>",
    '<!-- Un commentaire avec du texte -->',
    '<p data-i18n="k1">Texte de repli</p>',
    '<button aria-label="Fermer la fenêtre" data-i18n-aria-label="k2">x</button>',
    '<button aria-label="Fermer la fenêtre">Fermer</button>',
    '<input placeholder="Rechercher ici" title="Rechercher" alt="">',
    '<img src="a.png" alt="Logo de la société">',
    '<span class="a b" id="x y" data-x="deux mots">12</span>',
    '<p data-i18n="k3">Texte <b>imbriqué</b> de repli</p>',
    '<p>Un texte <em>avec</em> du balisage</p>',
    '<!-- i18n-ignore: nom de marque -->',
    "<p>Marque déposée</p>",
    "<p>Texte exempté</p> <!-- i18n-ignore: démonstration -->",
    "<p>&amp; &nbsp;</p>",
    '<script>document.title = "Un titre dynamique";</script>',
    '<script type="module" src="/app.mjs"></script>',
    "</body></html>",
  ].join("\n");
  const result = analyzeHtml(html, "web/index.html");
  assert.deepEqual(result.findings.map(finding => `${finding.line}:${finding.category}:${finding.detail}`), [
    '2:hardcoded:"Un titre en dur"', '7:hardcoded:"Fermer la fenêtre"', '7:hardcoded:"Fermer"', '8:hardcoded:"Rechercher ici"',
    '8:hardcoded:"Rechercher"', '9:hardcoded:"Logo de la société"', '12:hardcoded:"Un texte"', '12:hardcoded:"avec"', '12:hardcoded:"du balisage"',
    '17:hardcoded:"Un titre dynamique"',
  ].sort((x, y) => parseInt(x) - parseInt(y)), "expected findings");
  assert.deepEqual(result.uses.filter(use => use.kind === "attribute").map(use => use.key), ["k1", "k2", "k3"]);
});

// ---------------------------------------------------------------------------------------------
// Outil de contrôle : dépôt complet dans un dossier temporaire
// ---------------------------------------------------------------------------------------------

const work = mkdtempSync(join(tmpdir(), "atelier-i18n-"));
let counter = 0;
function repo(files) {
  counter += 1;
  const base = join(work, `r${counter}`);
  for (const [path, content] of Object.entries(files)) {
    mkdirSync(dirname(join(base, path)), { recursive: true });
    writeFileSync(join(base, path), content, "utf8");
  }
  return base;
}
const summarize = result => result.findings.map(finding => `${finding.category}|${finding.file}:${finding.line}|${finding.detail}`).sort();

try {
  test("repository: missing and unused keys, with file:line", () => {
    const base = repo({
      "web/i18n/fr-FR.txt": "# SPDX\nused=Utilisée\nviaHtml=Depuis HTML\nunused=Jamais\ndyn-a=Dynamique\ndyn-b=Dynamique\n",
      "web/app.mjs": 'import { t } from "/x.mjs";\nt("used");\nt("absent");\nt(`dyn-${x}`);\nmessages.get(ok ? "viaHtml" : "gone");\n',
      "web/index.html": '<p data-i18n="viaHtml">x</p>\n<p data-i18n="nowhere">x</p>\n<a data-i18n-aria-label="viaHtml"></a>',
    });
    assert.deepEqual(summarize(checkRepository({ root: base })), [
      "missing-key|web/app.mjs:3|absent", "missing-key|web/app.mjs:5|gone", "missing-key|web/index.html:2|nowhere", "unused-key|web/i18n/fr-FR.txt:4|unused",
    ]);
  });

  test("repository: hardcoded strings in modules and in index.html", () => {
    const base = repo({
      "web/i18n/fr-FR.txt": "",
      "web/m.mjs": 'node.textContent = "Bonjour le monde";\nnode.title = "Écho";\n// "commentaire en dur ici"\n',
      "web/sub/n.js": 'label.textContent = "Fermer";',
      "web/index.html": '<h1>Titre en dur</h1>\n<button aria-label="Fermer" data-i18n-aria-label="x"></button>',
    });
    assert.deepEqual(summarize(checkRepository({ root: base })).filter(item => item.startsWith("hardcoded")), [
      'hardcoded|web/index.html:1|"Titre en dur"', 'hardcoded|web/m.mjs:1|"Bonjour le monde"', 'hardcoded|web/m.mjs:2|"Écho"', 'hardcoded|web/sub/n.js:1|"Fermer"',
    ]);
  });

  test("repository: parameters missing, extra, undecidable and plural parameters", () => {
    const base = repo({
      "web/i18n/fr-FR.txt": "page=Page {page} sur {total}\nwords={n, plural, one {# mot} other {# mots}}\nplain=Rien\n",
      "web/app.mjs": [
        't("page", { page: 1 });',
        't("page", { page: 1, total: 2 });',
        't("page", { page: 1, total: 2, extra: 3 });',
        't("page");',
        't("words", { n: 3 });',
        't("words", {});',
        't("plain", { x: 1 });',
        't("page", params);',
        't("page", { ...base, page: 1 });',
        't("page", { page, total });',
        't("page", { ["page"]: 1 });',
        't("plain");',
        'messages.get("page");',
        'i18n.t("page", { total: 1 });',
        'other.format("page");',
      ].join("\n"),
    });
    assert.deepEqual(summarize(checkRepository({ root: base })), [
      "param-extra|web/app.mjs:3|page:extra", "param-extra|web/app.mjs:7|plain:x",
      "param-missing|web/app.mjs:14|page:page", "param-missing|web/app.mjs:1|page:total", "param-missing|web/app.mjs:4|page:page", "param-missing|web/app.mjs:4|page:total",
      "param-missing|web/app.mjs:6|words:n",
    ].sort());
  });

  test("repository: invalid catalogue (lines, keys, duplicates, plurals) and locale parity", () => {
    const base = repo({
      "web/i18n/fr-FR.txt": "a=1\na=2\nbroken\nbad key=1\np={n, plural, one {# x}}\nq={n, plural, few {# x} other {# y}}\nr=ok\n",
      "web/i18n/en-US.txt": "a=1\nextra=2\n",
      "web/app.mjs": 't("a"); t("p", {n: 1}); t("q", {n: 1}); t("r"); t("extra");',
    });
    assert.deepEqual(summarize(checkRepository({ root: base })), [
      "duplicate-key|web/i18n/fr-FR.txt:2|a", "invalid-key|web/i18n/fr-FR.txt:4|bad key", "invalid-line|web/i18n/fr-FR.txt:3|ligne 3",
      "locale-extra|web/i18n/en-US.txt:2|extra", "locale-missing|web/i18n/en-US.txt:1|p", "locale-missing|web/i18n/en-US.txt:1|q", "locale-missing|web/i18n/en-US.txt:1|r",
      "message-syntax|web/i18n/fr-FR.txt:5|p", "missing-key|web/app.mjs:1|extra", "plural-category|web/i18n/fr-FR.txt:6|q",
    ].sort());
  });

  const cli = (base, ...args) => {
    try {
      return { code: 0, out: execFileSync(process.execPath, [join(root, "tests/check-i18n.mjs"), `--root=${base}`, ...args], { encoding: "utf8" }) };
    } catch (error) { return { code: error.status, out: String(error.stdout) }; }
  };

  test("fix: concatenated keys register a prefix, undecidable keys are informative only", () => {
    const base = repo({
      "web/i18n/fr-FR.txt": "status-open=Ouvert\nstatus-closed=Fermé\nother=Autre\n",
      "web/app.mjs": 't("status-" + etat);\nt("other");\nt(name);\nmessages.get(compute());\nt(ok ? "other" : name);\n',
    });
    const result = checkRepository({ root: base });
    assert.deepEqual(result.findings, []);
    assert.deepEqual(result.info.map(item => item.category + ":" + item.line), ["dynamic-key:3", "dynamic-key:4"]);
    const run = cli(base);
    assert.equal(run.code, 0);
    assert.match(run.out, /^Clés dynamiques, invérifiables \(informatif/m);
    assert.match(run.out, /^  web\/app\.mjs:3: \[dynamic-key\] clé dynamique, invérifiable$/m);
    assert.equal(cli(base, "--strict").code, 0, "informative even in strict mode");
  });

  test("fix: an unknown backslash in the catalogue is reported as an invalid message with its line", () => {
    const BS = String.fromCharCode(92);
    const base = repo({ "web/i18n/fr-FR.txt": ["ok=1", "chemin=C:" + BS + "temp", "rate=50" + BS + "%", ""].join("\n"), "web/app.mjs": 't("ok"); t("chemin"); t("rate");' });
    assert.deepEqual(summarize(checkRepository({ root: base })), ["message-syntax|web/i18n/fr-FR.txt:2|chemin", "message-syntax|web/i18n/fr-FR.txt:3|rate"]);
  });

  test("repository: no catalogue at all is an error, not a crash", () => {
    const result = checkRepository({ root: repo({ "web/app.mjs": 'x.textContent = "Bonjour le monde";' }) });
    assert.deepEqual(result.findings.map(finding => finding.category).sort(), ["hardcoded", "no-catalog"]);
  });

  test("baseline: format, multiset counting, line shifts, new and resolved findings", () => {
    const finding = (line, detail = '"a b"', category = "hardcoded", file = "web/a.mjs") => ({ category, file, line, detail, message: detail });
    const baselineText = formatBaseline([finding(1), finding(2), finding(9, "k", "missing-key")]);
    assert.match(baselineText, /^# SPDX-License-Identifier: AGPL-3\.0-or-later\n/);
    const baseline = parseBaseline(baselineText);
    assert.equal(baseline.get('hardcoded\tweb/a.mjs\t"a b"'), 2);
    const same = compareToBaseline([finding(40), finding(41), finding(3, "k", "missing-key")], baseline);
    assert.deepEqual([same.fresh.length, same.known.length, same.resolved.length], [0, 3, 0], "line numbers do not matter");
    const third = compareToBaseline([finding(1), finding(2), finding(3), finding(9, "k", "missing-key")], baseline);
    assert.deepEqual(third.fresh.map(item => item.line), [3], "a third occurrence is new");
    const fixed = compareToBaseline([finding(1)], baseline);
    assert.equal(fixed.fresh.length, 0);
    assert.deepEqual([...fixed.resolved].sort(), ['1\thardcoded\tweb/a.mjs\t"a b"', "1\tmissing-key\tweb/a.mjs\tk"]);
    assert.equal(compareToBaseline([finding(1)], null).fresh.length, 1);
    assert.equal(compareToBaseline([finding(1, "autre", "hardcoded")], baseline).fresh.length, 1);
  });


  test("cli: strict failure, baseline adoption, new findings fail, fixed findings are listed", () => {
    const base = repo({
      "web/i18n/fr-FR.txt": "ok=Bonjour\n",
      "web/app.mjs": 't("ok");\nnode.textContent = "Ancien texte en dur";\nt("manquante");\n',
    });
    const strict = cli(base, "--strict");
    assert.equal(strict.code, 1);
    assert.match(strict.out, /^web\/app\.mjs:2: \[hardcoded\] chaîne en dur "Ancien texte en dur"$/m);
    assert.match(strict.out, /^web\/app\.mjs:3: \[missing-key\] clé manquante absente du catalogue$/m);
    assert.equal(cli(base).code, 1, "no baseline file: strict");
    const written = cli(base, "--update-baseline");
    assert.equal(written.code, 0);
    assert.match(readFileSync(join(base, "tests/i18n-baseline.txt"), "utf8"), /hardcoded\tweb\/app\.mjs\t"Ancien texte en dur"/);
    const adopted = cli(base);
    assert.equal(adopted.code, 0);
    assert.match(adopted.out, /Écarts connus de la référence \(2\)/);
    assert.match(adopted.out, /OK : aucun nouvel écart/);
    assert.equal(cli(base, "--strict").code, 1, "strict ignores the baseline");
    writeFileSync(join(base, "web/app.mjs"), 't("ok");\n\nnode.textContent = "Ancien texte en dur";\nt("manquante");\nother.textContent = "Nouveau texte en dur";\n');
    const regress = cli(base);
    assert.equal(regress.code, 1);
    assert.match(regress.out, /NOUVEAUX écarts \(1\) :\nweb\/app\.mjs:5: \[hardcoded\] chaîne en dur "Nouveau texte en dur"/);
    writeFileSync(join(base, "web/app.mjs"), 't("ok");\n');
    const improved = cli(base);
    assert.equal(improved.code, 0);
    assert.match(improved.out, /corrigés depuis \(2\)/);
    assert.equal(cli(base, "--bogus").code, 2);
  });

  test("real repository: the committed baseline matches, nothing new, the i18n module itself is clean", () => {
    const result = checkRepository({ root });
    const baseline = parseBaseline(readFileSync(join(root, "tests/i18n-baseline.txt"), "utf8"));
    const { fresh, resolved } = compareToBaseline(result.findings, baseline);
    assert.deepEqual(fresh, []);
    assert.deepEqual(resolved, [], "baseline is not stale");
    assert.deepEqual(result.findings.filter(finding => finding.file === "web/i18n.mjs"), []);
    assert.equal(cli(root).code, 0);
    assert.equal(cli(root, "--strict").code, 1);
    const counts = {};
    for (const finding of result.findings) counts[finding.category] = (counts[finding.category] ?? 0) + 1;
    assert.deepEqual(counts, { "duplicate-key": 4, hardcoded: 52, "missing-key": 8, "unused-key": 7 });
  });
} finally {
  rmSync(work, { recursive: true, force: true });
}

console.log(JSON.stringify({ status: "passed", count: checks.length, checks }));
