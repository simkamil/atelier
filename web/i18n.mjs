// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Message catalogue, named parameters, plurals and locale formatting (US-102).
 *
 * The module renders plain text only. Callers must assign the result with `textContent` or
 * `setAttribute`, never `innerHTML`: parameter values are inserted verbatim and never re-parsed.
 *
 * Catalogue format (`web/i18n/fr-FR.txt`): one `key=value` per line. The first `=` separates the
 * key from the value. Empty lines and lines starting with `#` are ignored. A value is a message:
 *   - `{name}`                                  named parameter (numbers are formatted by the locale);
 *   - `{n, plural, one {# page} other {# pages}}`  plural, a subset of ICU MessageFormat;
 *   - `#` inside a plural branch is the number, formatted by the locale;
 *   - escapes: `\n` newline, `\\` backslash, `\{` `\}` literal braces, `\#` literal hash.
 * Plural selectors: the CLDR categories `zero one two few many other` and exact values `=0`, `=1`.
 * `other` is mandatory. A missing `many` branch falls back to `other`.
 *
 * This file contains no user-facing sentence: locale data lives in {@link registerLocale} tables.
 */

/** Error raised for programming mistakes (bad syntax, bad arguments, missing parameter in dev mode). */
export class I18nError extends Error {
  /** @param {string} code @param {string} message */
  constructor(code, message) {
    super(message);
    this.name = "I18nError";
    this.code = code;
  }
}

const KEY_PATTERN = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;
const NAME_PATTERN = /^[A-Za-z_][A-Za-z0-9_]*$/;
const CATEGORIES = new Set(["zero", "one", "two", "few", "many", "other"]);
const MISSING_OPEN = "⟦";
const MISSING_CLOSE = "⟧";
const NNBSP = String.fromCharCode(0x202f); // narrow no-break space

/**
 * @typedef {object} LocaleData
 * @property {string} group thousands separator
 * @property {string} decimal decimal separator
 * @property {string} unitSpace separator between a number and its unit or percent sign
 * @property {string} dateTimeJoiner text placed between a date and a time
 * @property {string} firstDaySuffix suffix of day 1 in long dates ("" for none)
 * @property {Record<string, string>} lengthUnits symbols for pt, mm, cm, in
 * @property {{ one: string, other: string }} byte word for the byte
 * @property {{ decimal: string[], binary: string[] }} byteUnits
 * @property {string[]} pluralCategories categories the plural rules can produce
 * @property {(n: number) => string} plural reference plural rules, used to verify Intl
 */

/** @type {Map<string, LocaleData>} */
const LOCALES = new Map();

/**
 * French cardinal rules (CLDR): one for the integer part 0 and 1 (so 0, 1 and 1.5), many for exact
 * multiples of one million, other otherwise. Negative numbers use their absolute value.
 * @param {number} n
 */
function frenchPlural(n) {
  const a = Math.abs(n);
  if (!Number.isFinite(a)) return "other";
  const integerPart = Math.trunc(a);
  if (integerPart === 0 || integerPart === 1) return "one";
  if (Number.isInteger(a) && a % 1000000 === 0) return "many";
  return "other";
}

/** Adds or replaces the data of a locale. Only `fr-FR` is provided. @param {string} tag @param {LocaleData} data */
export function registerLocale(tag, data) {
  LOCALES.set(tag, Object.freeze({ ...data }));
}

registerLocale("fr-FR", {
  group: NNBSP,
  decimal: ",",
  unitSpace: NNBSP,
  dateTimeJoiner: " à ", // i18n-ignore: locale data, not an interface message
  firstDaySuffix: "er",
  lengthUnits: { pt: "pt", mm: "mm", cm: "cm", in: "po" },
  byte: { one: "octet", other: "octets" },
  byteUnits: { decimal: ["Ko", "Mo", "Go", "To"], binary: ["Kio", "Mio", "Gio", "Tio"] },
  pluralCategories: ["one", "many", "other"],
  plural: frenchPlural,
});

/** @param {string} tag @returns {LocaleData | undefined} */
export function getLocaleData(tag) {
  return LOCALES.get(tag);
}

const PROBES = [1000000000000, 1000000000000000, 0, 0.5, 1, 1.5, 1.99, 2, 2.5, 3, 10, 21, 100, 999999, 1000000, 1000001, 1500000, 2000000, 3000000, 10000000, 1000000000];

/**
 * Compares `Intl.PluralRules` with the reference rules of the locale on probe values (and their
 * negatives). Returns the mismatches so a caller or a test can show them.
 * @param {string} tag
 * @returns {{ ok: boolean, available: boolean, mismatches: { value: number, intl: string, expected: string }[] }}
 */
export function verifyPluralRules(tag) {
  const data = LOCALES.get(tag);
  let rules;
  try { rules = new Intl.PluralRules(tag); } catch { return { ok: false, available: false, mismatches: [] }; }
  const mismatches = [];
  if (data) {
    for (const probe of PROBES) {
      for (const value of [probe, -probe]) {
        const intl = rules.select(value);
        const expected = data.plural(value);
        if (intl !== expected) mismatches.push({ value, intl, expected });
      }
    }
  }
  return { ok: mismatches.length === 0, available: true, mismatches };
}

/** @type {Map<string, (n: number) => string>} */
const pluralCache = new Map();

/**
 * Returns the plural selector of a locale: `Intl.PluralRules` when it agrees with the reference
 * rules, the reference rules when it does not (older ICU lacks `many`), Intl alone for locales
 * without reference data.
 * @param {string} tag
 */
export function pluralSelector(tag) {
  let select = pluralCache.get(tag);
  if (!select) {
    const data = LOCALES.get(tag);
    const check = verifyPluralRules(tag);
    if (data && (!check.available || !check.ok)) select = data.plural;
    else if (check.available) {
      const rules = new Intl.PluralRules(tag);
      // Intl disagrees with CLDR for huge values (1e20 and above): the reference rules are authoritative there.
      select = n => (data && Math.abs(n) >= 2 ** 53 ? data.plural(n) : rules.select(n));
    }
    else select = n => (n === 1 ? "one" : "other");
    pluralCache.set(tag, select);
  }
  return select;
}

/** Plural category of a number in a locale. @param {number} n @param {string} [tag] */
export function pluralCategory(n, tag = "fr-FR") {
  return pluralSelector(tag)(n);
}

// ---------------------------------------------------------------------------------------------
// Catalogue parsing
// ---------------------------------------------------------------------------------------------

/**
 * @typedef {object} CatalogProblem
 * @property {string} code invalid-line | invalid-key | duplicate-key | invalid-message
 * @property {number} line 1-based line
 * @property {string} [key]
 * @property {string} message
 */

/**
 * @typedef {object} ParsedCatalog
 * @property {Map<string, { value: string, line: number }>} entries raw (still escaped) values; the last duplicate wins
 * @property {CatalogProblem[]} problems nothing is dropped silently
 */

const stripBom = text => (text.charCodeAt(0) === 0xfeff ? text.slice(1) : text);

/**
 * Parses `key=value` text. Compatible with the historic loader of `app.mjs` for valid files.
 * @param {string} text
 * @returns {ParsedCatalog}
 */
export function parseCatalog(text) {
  /** @type {ParsedCatalog} */
  const result = { entries: new Map(), problems: [] };
  const lines = stripBom(String(text)).split(/\r?\n/);
  lines.forEach((line, index) => {
    const number = index + 1;
    if (line.trim() === "" || line.startsWith("#")) return;
    const separator = line.indexOf("=");
    if (separator < 0) {
      result.problems.push({ code: "invalid-line", line: number, message: "line has no '=' separator" });
      return;
    }
    const key = line.slice(0, separator);
    if (!KEY_PATTERN.test(key)) {
      result.problems.push({ code: "invalid-key", line: number, key, message: `invalid key ${JSON.stringify(key)}` });
      return;
    }
    const value = line.slice(separator + 1);
    const previous = result.entries.get(key);
    if (previous) {
      result.problems.push({ code: "duplicate-key", line: number, key, message: `key already defined on line ${previous.line}` });
    }
    const analysis = analyzeMessage(value);
    if (analysis.error) result.problems.push({ code: "invalid-message", line: number, key, message: analysis.error });
    result.entries.set(key, { value, line: number });
  });
  return result;
}

// ---------------------------------------------------------------------------------------------
// Message compiler
// ---------------------------------------------------------------------------------------------

/**
 * @typedef {string | { type: "param", name: string } | { type: "hash" }
 *   | { type: "plural", name: string, cases: Map<string, Node[]> }} Node
 */

/**
 * @param {string} source
 * @returns {{ nodes: Node[], params: Set<string> }}
 * @throws {I18nError} code `syntax`
 */
export function compileMessage(source) {
  const params = new Set();
  let position = 0;
  const fail = reason => { throw new I18nError("syntax", `${reason} at offset ${position}`); };
  const skipSpaces = () => { while (position < source.length && /\s/.test(source[position])) position += 1; };

  /** @param {boolean} inPlural @param {boolean} nested */
  function parseNodes(inPlural, nested) {
    /** @type {Node[]} */
    const nodes = [];
    let text = "";
    const flush = () => { if (text) { nodes.push(text); text = ""; } };
    while (position < source.length) {
      const char = source[position];
      if (char === "\\") {
        const next = source[position + 1];
        if (next === "n") text += "\n";
        else if (next === "\\" || next === "{" || next === "}" || next === "#") text += next;
        else fail(next === undefined ? "dangling backslash" : `unknown escape \\${next}`);
        position += 2;
      } else if (char === "{") {
        flush();
        position += 1;
        nodes.push(parseArgument());
      } else if (char === "}") {
        if (!nested) fail("unbalanced '}'");
        flush();
        position += 1;
        return nodes;
      } else if (char === "#" && inPlural) {
        flush();
        nodes.push({ type: "hash" });
        position += 1;
      } else {
        text += char;
        position += 1;
      }
    }
    if (nested) fail("missing '}'");
    flush();
    return nodes;
  }

  function readWord() {
    const start = position;
    while (position < source.length && /[A-Za-z0-9_=]/.test(source[position])) position += 1;
    return source.slice(start, position);
  }

  function parseArgument() {
    skipSpaces();
    const name = readWord();
    if (!NAME_PATTERN.test(name)) fail(`invalid parameter name ${JSON.stringify(name)}`);
    params.add(name);
    skipSpaces();
    if (source[position] === "}") { position += 1; return { type: "param", name }; }
    if (source[position] !== ",") fail("expected '}' or ','");
    position += 1;
    skipSpaces();
    const kind = readWord();
    if (kind !== "plural") fail(`unsupported argument type ${JSON.stringify(kind)}`);
    skipSpaces();
    if (source[position] !== ",") fail("expected ',' after 'plural'");
    position += 1;
    const cases = new Map();
    for (;;) {
      skipSpaces();
      if (position >= source.length) fail("missing '}'");
      if (source[position] === "}") { position += 1; break; }
      const selector = readWord();
      if (!CATEGORIES.has(selector) && !/^=\d+$/.test(selector)) fail(`invalid plural selector ${JSON.stringify(selector)}`);
      if (cases.has(selector)) fail(`duplicate plural selector ${selector}`);
      skipSpaces();
      if (source[position] !== "{") fail(`expected '{' after ${selector}`);
      position += 1;
      cases.set(selector, parseNodes(true, true));
    }
    if (!cases.has("other")) fail("plural needs an 'other' branch");
    return { type: "plural", name, cases };
  }

  const nodes = parseNodes(false, false);
  return { nodes, params };
}

/**
 * Non-throwing variant for tools.
 * @param {string} source
 * @param {{ locale?: string }} [options]
 * @returns {{ params: Set<string>, error?: string, warnings: string[] }}
 */
export function analyzeMessage(source, options = {}) {
  const warnings = [];
  try {
    const { nodes, params } = compileMessage(source);
    const data = LOCALES.get(options.locale ?? "fr-FR");
    if (data) {
      const visit = list => {
        for (const node of list) {
          if (typeof node === "object" && node.type === "plural") {
            for (const [selector, branch] of node.cases) {
              if (CATEGORIES.has(selector) && selector !== "other" && !data.pluralCategories.includes(selector)) {
                warnings.push(`plural branch '${selector}' is never selected by the rules of ${options.locale ?? "fr-FR"}`);
              }
              visit(branch);
            }
          }
        }
      };
      visit(nodes);
    }
    return { params, warnings };
  } catch (error) {
    return { params: new Set(), error: error instanceof Error ? error.message : String(error), warnings };
  }
}

// ---------------------------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------------------------

const POINTS_PER = Object.freeze({ pt: 1, twip: 1 / 20, in: 72, cm: 72 / 2.54, mm: 72 / 25.4 });
const LENGTH_DIGITS = Object.freeze({ pt: 1, mm: 1, cm: 2, in: 2 });

/**
 * Converts a length between pt, twip (1/20 pt), in, cm and mm.
 * @param {number} value
 * @param {"pt" | "twip" | "in" | "cm" | "mm"} from
 * @param {"pt" | "twip" | "in" | "cm" | "mm"} to
 */
export function convertLength(value, from, to) {
  if (!Object.hasOwn(POINTS_PER, from) || !Object.hasOwn(POINTS_PER, to)) throw new I18nError("unit", `unknown length unit ${from} -> ${to}`);
  return Number(((value * POINTS_PER[from]) / POINTS_PER[to]).toPrecision(12));
}

function toDate(value) {
  const date = value instanceof Date ? value : typeof value === "number" ? new Date(value) : null;
  if (!date || Number.isNaN(date.getTime())) throw new I18nError("invalid-date", "expected a valid Date or a finite timestamp");
  return date;
}

/**
 * @typedef {object} IssueInfo
 * @property {string} code missing-key | missing-param | invalid-param | invalid-message | locale
 * @property {string} message
 * @property {string} [key]
 */

/**
 * @typedef {object} I18nOptions
 * @property {string} [locale] BCP 47 tag, default `fr-FR`
 * @property {string | ParsedCatalog | Map<string, string | { value: string }> | Record<string, string>} [catalog]
 * @property {boolean} [dev] development mode: a missing parameter throws, issues go to the console
 * @property {(issue: IssueInfo) => void} [onIssue] receives every issue (replaces the console)
 */

/**
 * Creates the translator and the formatters of one locale.
 * @param {I18nOptions} [options]
 */
export function createI18n(options = {}) {
  const locale = options.locale ?? "fr-FR";
  const dev = options.dev === true;
  const data = LOCALES.get(locale);
  /** @type {IssueInfo[]} */
  const issues = [];
  const reported = new Set();
  function report(issue) {
    const id = `${issue.code}|${issue.key ?? ""}|${issue.message}`;
    if (reported.has(id)) return;
    reported.add(id);
    if (issues.length < 200) issues.push(issue);
    if (options.onIssue) { try { options.onIssue(issue); } catch { /* a reporter must not break rendering */ } }
    else if (dev) console.warn(`[i18n] ${issue.code}${issue.key ? ` ${issue.key}` : ""}: ${issue.message}`);
  }

  let supported = true;
  try { supported = Intl.NumberFormat.supportedLocalesOf([locale]).length > 0; } catch { supported = false; }
  if (!supported) report({ code: "locale", message: `Intl has no data for ${locale}` });

  const rawEntries = new Map();
  const source = options.catalog;
  if (typeof source === "string") for (const [key, entry] of parseCatalog(source).entries) rawEntries.set(key, entry.value);
  else if (source && source.entries instanceof Map) for (const [key, entry] of source.entries) rawEntries.set(key, entry.value);
  else if (source instanceof Map) for (const [key, entry] of source) rawEntries.set(key, typeof entry === "string" ? entry : entry.value);
  else if (source) for (const [key, value] of Object.entries(source)) rawEntries.set(key, String(value));

  /** @type {Map<string, { nodes: Node[], params: Set<string> } | { error: string }>} */
  const compiled = new Map();
  function lookup(key) {
    let entry = compiled.get(key);
    if (entry === undefined && rawEntries.has(key)) {
      try { entry = compileMessage(rawEntries.get(key)); } catch (error) { entry = { error: error.message }; }
      compiled.set(key, entry);
    }
    return entry;
  }

  const formatters = new Map();
  function numberFormat(extra) {
    const id = JSON.stringify(extra);
    let format = formatters.get(id);
    if (!format) { format = new Intl.NumberFormat(locale, extra); formatters.set(id, format); }
    return format;
  }

  /** Maps ICU parts to the locale data so the output does not depend on the ICU version. */
  function joinParts(parts, percent) {
    const digitsAreZero = parts.every(part => (part.type !== "integer" && part.type !== "fraction") || /^0+$/.test(part.value));
    let text = "";
    for (const part of parts) {
      if (part.type === "group" && data) text += data.group;
      else if (part.type === "decimal" && data) text += data.decimal;
      else if (part.type === "minusSign" && digitsAreZero) continue;
      else if (part.type === "literal" && percent && data) continue;
      else if (part.type === "percentSign" && data) text += data.unitSpace + part.value;
      else text += part.value;
    }
    return text;
  }

  function fractionOptions(opts = {}) {
    const result = {};
    if (opts.decimals !== undefined) { result.minimumFractionDigits = opts.decimals; result.maximumFractionDigits = opts.decimals; }
    else if (opts.maxDecimals !== undefined) { result.minimumFractionDigits = 0; result.maximumFractionDigits = opts.maxDecimals; }
    if (opts.grouping === false) result.useGrouping = false;
    return result;
  }

  /**
   * @param {number | bigint} value
   * @param {{ decimals?: number, maxDecimals?: number, grouping?: boolean }} [opts] `decimals` is exact, `maxDecimals` trims zeros
   */
  function formatNumber(value, opts) {
    return joinParts(numberFormat(fractionOptions(opts)).formatToParts(value), false);
  }

  /** @param {number} fraction 0.256 gives "25,6 %" @param {{ decimals?: number, maxDecimals?: number }} [opts] */
  function formatPercent(fraction, opts) {
    return joinParts(numberFormat({ style: "percent", ...fractionOptions(opts) }).formatToParts(fraction), true);
  }

  /**
   * Without `timeZone` the machine time zone is used (right for a local application; pass `timeZone` for a
   * reproducible result). An invalid zone throws I18nError("invalid-timezone") in development; in production
   * it is reported and the machine time zone is used.
   */
  const dateParts = (date, extra) => {
    try {
      return new Intl.DateTimeFormat(locale, { timeZone: extra.timeZone, ...extra.fields }).formatToParts(date);
    } catch (error) {
      if (!(error instanceof RangeError) || extra.timeZone === undefined) throw error;
      const message = `invalid time zone ${String(extra.timeZone)}`;
      report({ code: "invalid-timezone", message });
      if (dev) throw new I18nError("invalid-timezone", message);
      return new Intl.DateTimeFormat(locale, extra.fields).formatToParts(date);
    }
  };

  /** Without `timeZone` the machine time zone is used; an invalid zone is an I18nError (dev) or falls back to the machine zone (production). @param {Date | number} value @param {{ style?: "long" | "short", timeZone?: string }} [opts] */
  function formatDate(value, opts = {}) {
    const date = toDate(value);
    if ((opts.style ?? "long") === "short") {
      return dateParts(date, { timeZone: opts.timeZone, fields: { day: "2-digit", month: "2-digit", year: "numeric" } })
        .map(part => part.value).join("");
    }
    const parts = dateParts(date, { timeZone: opts.timeZone, fields: { day: "numeric", month: "long", year: "numeric" } });
    const pick = type => parts.find(part => part.type === type)?.value ?? "";
    const day = pick("day") === "1" && data ? `1${data.firstDaySuffix}` : pick("day");
    return `${day} ${pick("month")} ${pick("year")}`;
  }

  /** Without `timeZone` the machine time zone is used; an invalid zone is an I18nError (dev) or falls back to the machine zone (production). 24-hour clock "14:30". @param {Date | number} value @param {{ timeZone?: string }} [opts] */
  function formatTime(value, opts = {}) {
    return dateParts(toDate(value), { timeZone: opts.timeZone, fields: { hour: "2-digit", minute: "2-digit", hourCycle: "h23" } })
      .map(part => part.value).join("");
  }

  /** Without `timeZone` the machine time zone is used; an invalid zone is an I18nError (dev) or falls back to the machine zone (production). @param {Date | number} value @param {{ style?: "long" | "short", timeZone?: string }} [opts] */
  function formatDateTime(value, opts = {}) {
    return formatDate(value, opts) + (data?.dateTimeJoiner ?? " ") + formatTime(value, opts);
  }

  /**
   * Converts then formats a length, e.g. `formatLength(72, { to: "cm" })` gives "2,54 cm".
   * Default rounding: mm 1, cm 2, pt 1, in 2 decimals at most.
   * @param {number} value
   * @param {{ from?: "pt" | "twip" | "in" | "cm" | "mm", to?: "pt" | "in" | "cm" | "mm", decimals?: number, maxDecimals?: number }} [opts]
   */
  function formatLength(value, opts = {}) {
    const to = opts.to ?? "cm";
    const symbol = data?.lengthUnits[to] ?? to;
    const converted = convertLength(value, opts.from ?? "pt", to);
    const rounding = opts.decimals !== undefined || opts.maxDecimals !== undefined ? opts : { maxDecimals: LENGTH_DIGITS[to] };
    return formatNumber(converted, rounding) + (data?.unitSpace ?? " ") + symbol;
  }

  const select = pluralSelector(locale);

  /**
   * File size: "512 octets", "1,5 Ko", "12 Mo". Decimal units (1 Ko = 1000 octets) by default,
   * `binary: true` for Kio/Mio/Gio (1024).
   * @param {number} bytes @param {{ binary?: boolean }} [opts]
   */
  function formatFileSize(bytes, opts = {}) {
    if (!Number.isFinite(bytes) || bytes < 0) throw new I18nError("invalid-size", "expected a finite non-negative byte count");
    const base = opts.binary ? 1024 : 1000;
    const names = data ? (opts.binary ? data.byteUnits.binary : data.byteUnits.decimal) : ["KB", "MB", "GB", "TB"];
    if (bytes < base) {
      const whole = Math.round(bytes); // fractional byte counts are rounded half up
      const word = data ? data.byte[select(whole) === "one" ? "one" : "other"] : "B";
      return `${formatNumber(whole, { maxDecimals: 0 })}${data?.unitSpace ?? " "}${word}`;
    }
    const round = value => (value < 10 ? Math.round(Number((value * 10).toPrecision(12))) / 10 : Math.round(Number(value.toPrecision(12))));
    let index = 0;
    let scaled = bytes / base;
    // Pick the unit after rounding: 999 999 bytes must read "1 Mo", not "1000 Ko".
    while (index < names.length - 1 && round(scaled) >= base) { scaled /= base; index += 1; }
    return `${formatNumber(round(scaled), { maxDecimals: 1 })}${data?.unitSpace ?? " "}${names[index]}`;
  }

  /** "a, b et c" (conjunction) or "a, b ou c" (disjunction). @param {string[]} items @param {{ type?: "and" | "or" }} [opts] */
  function formatList(items, opts = {}) {
    return new Intl.ListFormat(locale, { type: opts.type === "or" ? "disjunction" : "conjunction", style: "long" }).format(items);
  }

  function insertable(name, value) {
    if (typeof value === "number" || typeof value === "bigint") return formatNumber(value);
    if (typeof value === "string") return value;
    report({ code: "invalid-param", message: `parameter ${name} should be a string or a number` });
    if (dev) throw new I18nError("invalid-param", `parameter ${name} should be a string or a number`);
    return String(value);
  }

  function render(nodes, params, hash, key) {
    let out = "";
    for (const node of nodes) {
      if (typeof node === "string") { out += node; continue; }
      if (node.type === "hash") { out += hash === undefined ? "#" : formatNumber(hash); continue; }
      const present = params !== null && typeof params === "object" && Object.hasOwn(params, node.name) && params[node.name] != null;
      if (!present) {
        const message = `missing parameter ${node.name}`;
        report({ code: "missing-param", key, message });
        if (dev) throw new I18nError("missing-param", `${message} for message ${key}`);
        out += `{${node.name}}`;
        continue;
      }
      const value = params[node.name];
      if (node.type === "param") { out += insertable(node.name, value); continue; }
      const count = typeof value === "bigint" ? Number(value) : typeof value === "string" && value.trim() !== "" ? Number(value) : value;
      if (typeof count !== "number" || !Number.isFinite(count)) {
        report({ code: "invalid-param", key, message: `plural parameter ${node.name} should be a finite number` });
        if (dev) throw new I18nError("invalid-param", `plural parameter ${node.name} should be a finite number (message ${key})`);
        out += `{${node.name}}`; // never display an invented number
        continue;
      }
      const branch = node.cases.get(`=${count}`) ?? node.cases.get(select(count)) ?? node.cases.get("other");
      out += render(branch, params, count, key);
    }
    return out;
  }

  /**
   * Translates a key. An unknown key never throws: it renders as a visible marker and is reported.
   * @param {string} key
   * @param {Record<string, string | number | bigint>} [params]
   */
  function t(key, params = {}) {
    const entry = typeof key === "string" ? lookup(key) : undefined;
    if (!entry) {
      report({ code: "missing-key", key: String(key), message: "key not found in the catalogue" });
      return `${MISSING_OPEN}${String(key)}${MISSING_CLOSE}`;
    }
    if ("error" in entry) {
      report({ code: "invalid-message", key, message: entry.error });
      return `${MISSING_OPEN}${key}${MISSING_CLOSE}`;
    }
    return render(entry.nodes, params, undefined, key);
  }

  return Object.freeze({
    locale, t, has: key => rawEntries.has(key), issues,
    formatNumber, formatPercent, formatDate, formatTime, formatDateTime, formatLength, formatFileSize, formatList,
    convertLength, pluralCategory: n => select(n),
  });
}
