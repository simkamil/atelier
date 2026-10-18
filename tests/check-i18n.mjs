// SPDX-License-Identifier: AGPL-3.0-or-later
// Contrôle d'internationalisation (US-102) : clés manquantes, clés inutilisées, chaînes en dur,
// paramètres incohérents, catalogue invalide. Sans dépendance ; voir docs/us-102-i18n.md.
//
//   node tests/check-i18n.mjs                       compare au fichier de référence (code 0 si aucun nouvel écart)
//   node tests/check-i18n.mjs --strict              ignore la référence : tout écart fait échouer
//   node tests/check-i18n.mjs --update-baseline     réécrit tests/i18n-baseline.txt avec l'état actuel
//   options : --root=<dossier> --baseline=<fichier> --quiet (résumé seul des écarts connus)
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { compileMessage, analyzeMessage, parseCatalog } from "../web/i18n.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));

// ---------------------------------------------------------------------------------------------
// Configuration (exemptions et heuristiques). Toute exemption est expliquée ici ou dans le code
// source par `// i18n-ignore: raison` (JS) ou `<!-- i18n-ignore: raison -->` (HTML), sur la ligne
// concernée ou sur la ligne de commentaire seule qui la précède.
// ---------------------------------------------------------------------------------------------

/** Textes qui ne se traduisent pas (marques). Comparaison sans ponctuation ni casse. */
const ALLOWED_TEXT = new Set([
  "atelier", // nom du produit
]);

/**
 * Modules du moteur, des protocoles et de la bibliothèque i18n : ils ne produisent aucun texte d'interface,
 * seulement des diagnostics de développeur en anglais (tables d'états, messages d'erreur). Leurs chaînes sans
 * lettre accentuée sont donc exemptées ; une chaîne accentuée (donc probablement française) reste signalée.
 */
const DIAGNOSTIC_MODULES = new Set([
  "web/wasm-loader.mjs", "web/wasm-protocol.mjs", "web/editor-protocol.mjs", "web/display-list.mjs",
  "web/engine-client.mjs", "web/engine-worker.mjs", "web/i18n.mjs",
]);
/** Fonctions dont les arguments texte ne sont jamais affichés (sélecteurs, événements, API DOM, diagnostics). */
const NON_DISPLAY_CALLEES = new Set([
  "addEventListener", "removeEventListener", "dispatchEvent", "querySelector", "querySelectorAll", "getElementById",
  "closest", "matches", "getAttribute", "removeAttribute", "hasAttribute", "toggleAttribute", "createElement",
  "createElementNS", "matchMedia", "postMessage", "fetch", "import", "require", "add", "remove", "toggle", "contains",
  "replace", "setProperty", "getPropertyValue", "removeProperty", "has", "get", "delete", "hasOwn", "split", "join",
  "startsWith", "endsWith", "includes", "indexOf", "lastIndexOf", "padStart", "padEnd", "test", "match", "matchAll",
  "normalize", "localeCompare", "toLocaleString", "toLocaleUpperCase", "toLocaleLowerCase", "getItem", "setItem",
  "removeItem", "parse", "stringify", "for", "assert", "ok", "equal", "deepEqual", "strictEqual", "throws", "rejects",
  "warn", "log", "error", "info", "debug", "trace", "time", "timeEnd", "NumberFormat", "DateTimeFormat", "PluralRules",
  "ListFormat", "defineProperty", "setRequestHeader", "getComputedStyle", "from", "of", "super",
]);
/** Aides de validation et d'échec : leurs messages sont des diagnostics de développeur (assertX, failX, invalidX, rejectX). */
const NON_DISPLAY_CALLEE_PATTERN = /^(?:assert|fail|invalid|reject|throw)/i;
/** `new X(...)` dont X se termine par ces motifs : jamais affiché. */
const NON_DISPLAY_NEW = /(?:Error|DOMException|Event|URL|Worker|RegExp|Map|Set|Headers|Request|Response|Blob|AbortController)$/;
/** Attributs et propriétés qui portent du texte lisible. */
const DISPLAY_ATTRS = new Set(["aria-label", "title", "placeholder", "alt", "aria-description", "aria-valuetext", "aria-roledescription"]);
const DISPLAY_PROPS = new Set(["textContent", "innerText", "outerText", "title", "placeholder", "alt", "ariaLabel", "ariaDescription", "ariaValueText", "ariaRoleDescription", "innerHTML", "outerHTML", "label"]);
const DISPLAY_KEYS = new Set(["label", "title", "text", "placeholder", "description", "tooltip", "caption", "ariaLabel", "alt", "message", "heading", "summary"]);
const DISPLAY_METHODS = new Set(["append", "prepend", "createTextNode", "before", "after", "replaceChildren"]);
/** Propriétés dont la valeur est technique. */
const DATA_PROPS = new Set(["className", "id", "type", "name", "href", "src", "value", "htmlFor", "role", "cssText", "tabIndex", "dir", "lang", "rel", "target"]);
/** Appels `callee(...)` qui reçoivent une clé de catalogue en premier argument. */
const KEY_CALLEES = [["t"], ["i18n", "t"], ["messages", "get"], ["catalog", "get"], ["FALLBACK", "get"]];

const REFERENCE_LOCALE = "fr-FR";
const DEFAULT_BASELINE = "tests/i18n-baseline.txt";
const MAX_DETAIL = 90;

// ---------------------------------------------------------------------------------------------
// Analyseur de jetons JavaScript
// ---------------------------------------------------------------------------------------------

const KEYWORDS_BEFORE_REGEX = new Set(["return", "typeof", "case", "do", "else", "in", "of", "new", "delete", "void", "throw", "instanceof", "yield", "await"]);
const CONDITION_KEYWORDS = new Set(["if", "while", "for", "with"]);
const PUNCTUATORS = [">>>=", "...", "===", "!==", "**=", "<<=", ">>=", ">>>", "&&=", "||=", "??=", "=>", "==", "!=", "<=", ">=", "&&", "||", "??", "?.", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<", ">>", "**"];
const NUMBER = /^(?:0[xX][\da-fA-F_]+|0[bB][01_]+|0[oO][0-7_]+|(?:\d[\d_]*\.?[\d_]*|\.\d[\d_]*)(?:[eE][+-]?\d+)?)n?/;
const IDENT_START = /[\p{ID_Start}$_]/u;
const IDENT_PART = /[\p{ID_Continue}$\u200c\u200d]/u;

/**
 * @typedef {object} Token
 * @property {"ident" | "number" | "string" | "template" | "regex" | "punct"} type
 * @property {string} value texte cuit pour une chaîne, source brute sinon
 * @property {number} start @property {number} end @property {number} line @property {number} endLine
 * @property {string[]} [quasis] morceaux statiques d'un gabarit
 * @property {number} [exprCount] nombre d'expressions `${}` d'un gabarit
 */

/**
 * Découpe du JavaScript en jetons. Distingue commentaires, chaînes, gabarits (imbriqués) et
 * expressions régulières ; les commentaires sont rendus à part.
 * @param {string} source
 * @returns {{ tokens: Token[], comments: { text: string, line: number, endLine: number }[] }}
 */
export function tokenize(source) {
  const tokens = [];
  const comments = [];
  const lineStarts = [0];
  for (let index = 0; index < source.length; index += 1) if (source[index] === "\n") lineStarts.push(index + 1);
  const lineOf = position => {
    let low = 0;
    let high = lineStarts.length - 1;
    while (low < high) {
      const middle = (low + high + 1) >> 1;
      if (lineStarts[middle] <= position) low = middle; else high = middle - 1;
    }
    return low + 1;
  };
  /** Pile des accolades ouvertes et des gabarits dont une expression `${` est en cours. */
  const stack = [];
  const parens = []; // vrai quand la parenthèse ouverte suit if / while / for / with
  let previous = null;
  let position = 0;

  const make = (type, value, start, end, extra = {}) => ({ type, value, start, end, line: lineOf(start), endLine: lineOf(Math.max(start, end - 1)), ...extra });
  const emit = token => { tokens.push(token); previous = token; return token; };

  function cook(at) {
    const next = source[at + 1];
    switch (next) {
      case "n": return ["\n", at + 2];
      case "t": return ["\t", at + 2];
      case "r": return ["\r", at + 2];
      case "b": return ["\b", at + 2];
      case "f": return ["\f", at + 2];
      case "v": return ["\v", at + 2];
      case "0": return ["\0", at + 2];
      case "\n": return ["", at + 2];
      case "x": return [String.fromCharCode(parseInt(source.slice(at + 2, at + 4), 16) || 0), at + 4];
      case "u": {
        if (source[at + 2] === "{") {
          const close = source.indexOf("}", at);
          return [String.fromCodePoint(parseInt(source.slice(at + 3, close), 16) || 0), close + 1];
        }
        return [String.fromCharCode(parseInt(source.slice(at + 2, at + 6), 16) || 0), at + 6];
      }
      default: return [next ?? "", at + 2];
    }
  }

  function scanTemplate(from, template) {
    let at = from;
    let current = "";
    while (at < source.length) {
      const char = source[at];
      if (char === "\\") { const [text, next] = cook(at); current += text; at = next; }
      else if (char === "`") {
        template.quasis.push(current);
        template.end = at + 1;
        template.endLine = lineOf(at);
        previous = template;
        return at + 1;
      } else if (char === "$" && source[at + 1] === "{") {
        template.quasis.push(current);
        template.exprCount += 1;
        stack.push(template);
        previous = { type: "punct", value: "${" };
        return at + 2;
      } else { current += char; at += 1; }
    }
    template.quasis.push(current);
    template.end = source.length;
    return source.length;
  }

  const regexAllowed = () => {
    if (!previous) return true;
    if (previous.type === "number" || previous.type === "string" || previous.type === "template" || previous.type === "regex") return false;
    if (previous.type === "ident") return KEYWORDS_BEFORE_REGEX.has(previous.value);
    if (previous.value === ")") return previous.condition === true; // « if (x) /re/ » : la parenthèse ferme une condition
    return !(previous.value === "]" || previous.value === "++" || previous.value === "--");
  };

  while (position < source.length) {
    const char = source[position];
    if (/\s/.test(char)) { position += 1; continue; }
    if (char === "/" && source[position + 1] === "/") {
      let end = source.indexOf("\n", position);
      if (end < 0) end = source.length;
      comments.push({ text: source.slice(position + 2, end), line: lineOf(position), endLine: lineOf(position) });
      position = end;
      continue;
    }
    if (char === "/" && source[position + 1] === "*") {
      let end = source.indexOf("*/", position + 2);
      end = end < 0 ? source.length : end + 2;
      comments.push({ text: source.slice(position + 2, end - 2), line: lineOf(position), endLine: lineOf(end - 1) });
      position = end;
      continue;
    }
    if (char === '"' || char === "'") {
      let at = position + 1;
      let text = "";
      while (at < source.length && source[at] !== char && source[at] !== "\n") {
        if (source[at] === "\\") { const [piece, next] = cook(at); text += piece; at = next; } else { text += source[at]; at += 1; }
      }
      emit(make("string", text, position, at + 1));
      position = at + 1;
      continue;
    }
    if (char === "`") {
      const template = emit(make("template", "", position, position + 1, { quasis: [], exprCount: 0 }));
      position = scanTemplate(position + 1, template);
      continue;
    }
    if (char === "}") {
      const top = stack.pop();
      if (top && top.type === "template") { position = scanTemplate(position + 1, top); continue; }
      emit(make("punct", "}", position, position + 1));
      position += 1;
      continue;
    }
    if (char === "{") { stack.push({ type: "brace" }); emit(make("punct", "{", position, position + 1)); position += 1; continue; }
    if (char === "/" && regexAllowed()) {
      let at = position + 1;
      let inClass = false;
      let closed = false;
      while (at < source.length && source[at] !== "\n") {
        if (source[at] === "\\") at += 2;
        else if (source[at] === "[") { inClass = true; at += 1; }
        else if (source[at] === "]") { inClass = false; at += 1; }
        else if (source[at] === "/" && !inClass) { closed = true; at += 1; break; }
        else at += 1;
      }
      if (closed) {
        while (at < source.length && /[a-z]/i.test(source[at])) at += 1;
        emit(make("regex", source.slice(position, at), position, at));
        position = at;
        continue;
      }
    }
    if (/[0-9]/.test(char) || (char === "." && /[0-9]/.test(source[position + 1] ?? ""))) {
      const match = NUMBER.exec(source.slice(position, position + 64));
      emit(make("number", match[0], position, position + match[0].length));
      position += match[0].length;
      continue;
    }
    if (IDENT_START.test(char) || (char === "#" && IDENT_START.test(source[position + 1] ?? ""))) {
      let at = position + 1;
      while (at < source.length && IDENT_PART.test(source[at])) at += 1;
      emit(make("ident", source.slice(position, at), position, at));
      position = at;
      continue;
    }
    const punctuator = PUNCTUATORS.find(candidate => source.startsWith(candidate, position)) ?? char;
    if (punctuator === "(") parens.push(previous?.type === "ident" && CONDITION_KEYWORDS.has(previous.value));
    const closed = punctuator === ")" ? parens.pop() : undefined;
    emit(make("punct", punctuator, position, position + punctuator.length, closed ? { condition: true } : {}));
    position += punctuator.length;
  }
  return { tokens, comments };
}

// ---------------------------------------------------------------------------------------------
// Heuristique « texte lisible »
// ---------------------------------------------------------------------------------------------

const ACCENTED = /[À-ÖØ-öø-ÿŒœ]/u;
const WORD = /^[(«"“]?\p{L}[\p{L}'’-]*\p{L}[.,;:!?…)»"”]*$/u;
const SELECTOR = /^[.#[:*@]|^--|\[[^\]]*=|\s[>+~]\s|::|:not\(|:is\(/;

/**
 * @param {string} text texte statique ; `\u0001` marque une expression insérée
 * @param {boolean} relaxed vrai pour un emplacement d'affichage certain : un mot de 3 lettres suffit
 */
export function looksHuman(text, relaxed = false) {
  const trimmed = text.trim();
  if (!/\p{L}/u.test(trimmed)) return false;
  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(trimmed) || /^\.{0,2}\//.test(trimmed) || SELECTOR.test(trimmed)) return false;
  const bare = trimmed.replace(/^[^\p{L}]+|[^\p{L}]+$/gu, "").toLowerCase();
  if (ALLOWED_TEXT.has(bare)) return false;
  const words = trimmed.split(/\s+/).filter(word => !word.includes("\u0001"));
  if (words.length && words.every(word => /^\w+(?:[-_.:]\w+)+$/.test(word))) return false; // classes CSS, identifiants composés
  const human = words.filter(word => WORD.test(word));
  if (human.some(word => ACCENTED.test(word))) return true;
  if (human.length >= 2) return true;
  return relaxed && human.length === 1 && /\p{L}{3,}/u.test(human[0]);
}

// ---------------------------------------------------------------------------------------------
// Analyse d'un module JavaScript
// ---------------------------------------------------------------------------------------------

const NOT_A_CALL = new Set(["if", "for", "while", "switch", "catch", "with", "return", "typeof", "void", "delete", "await", "yield", "in", "of", "case", "throw", "function", "else", "do", "new", "instanceof"]);
const COMPARISONS = new Set(["===", "!==", "==", "!="]);
const KEY_POSITION = new Set(["(", "?", ":", "??", "||", "&&"]);

/**
 * @typedef {object} Finding
 * @property {string} category @property {string} file @property {number} line @property {string} detail @property {string} message
 */

const clip = text => (text.length > MAX_DETAIL ? `${text.slice(0, MAX_DETAIL - 1)}…` : text);
const staticText = token => (token.type === "template" ? token.quasis.join("\u0001") : token.value);

/**
 * Lignes exemptées par `i18n-ignore: raison` (la ligne et la suivante) ou par une région
 * `i18n-ignore-start: raison` ... `i18n-ignore-end`, plus les exemptions sans raison.
 */
function exemptions(comments, tokenLines, file, findings, marker = /\bi18n-ignore(?!-)\b(?::\s*(\S[^]*))?/) {
  const lines = new Set();
  const finding = (line, detail, message) => findings.push({ category: "invalid-exemption", file, line, detail, message });
  const reasonless = reason => !reason || !reason.replace(/-->|\*\/$/g, "").trim();
  let region = null;
  for (const comment of comments) {
    const edge = /\bi18n-ignore-(start|end)\b(?::\s*(\S[^]*))?/.exec(comment.text);
    if (edge) {
      if (edge[1] === "end") {
        if (region === null) finding(comment.line, "i18n-ignore-end", "i18n-ignore-end sans i18n-ignore-start");
        else for (let line = region; line <= comment.endLine; line += 1) lines.add(line);
        region = null;
      } else if (reasonless(edge[2])) finding(comment.line, "i18n-ignore-start", "exemption sans raison : écrire « i18n-ignore-start: raison »");
      else if (region === null) region = comment.line;
      else finding(comment.line, "i18n-ignore-start", "i18n-ignore-start imbriqué : fermer la région ouverte ligne " + region + " d’abord");
      continue;
    }
    const match = marker.exec(comment.text);
    if (!match) continue;
    if (reasonless(match[1])) { finding(comment.line, "i18n-ignore", "exemption sans raison : écrire « i18n-ignore: raison »"); continue; }
    for (let line = comment.line; line <= comment.endLine; line += 1) lines.add(line);
    if (!tokenLines.has(comment.endLine)) lines.add(comment.endLine + 1);
  }
  if (region !== null) finding(region, "i18n-ignore-start", "i18n-ignore-start sans i18n-ignore-end");
  return lines;
}

/**
 * @param {string} source @param {string} file chemin relatif affiché
 * @param {{ lineOffset?: number }} [options]
 */
export function analyzeScript(source, file, options = {}) {
  const { tokens, comments } = tokenize(source);
  const offset = options.lineOffset ?? 0;
  /** @type {Finding[]} */
  const findings = [];
  const uses = []; // { key, line, kind: "call" | "selector" | "literal" }
  const prefixes = []; // préfixes dynamiques `theme-${…}` ou "theme-" + x
  const dynamics = []; // clés indécidables : t(variable)
  const calls = []; // appels t(...) vérifiables
  const tokenLines = new Set(tokens.map(token => token.line));
  const ignored = exemptions(comments, tokenLines, file, findings);
  const isIgnored = token => {
    for (let line = token.line; line <= token.endLine; line += 1) if (ignored.has(line)) return true;
    return false;
  };
  const at = index => tokens[index];
  const isPunct = (token, value) => token?.type === "punct" && token.value === value;

  /** Chemin d'un appelant : `a.b.c(` donne ["a","b","c"]. */
  function calleeOf(openIndex) {
    const nameToken = at(openIndex - 1);
    if (!nameToken || nameToken.type !== "ident" || NOT_A_CALL.has(nameToken.value)) return null;
    const path = [nameToken.value];
    let index = openIndex - 1;
    while ((isPunct(at(index - 1), ".") || isPunct(at(index - 1), "?.")) && at(index - 2)?.type === "ident") { path.unshift(at(index - 2).value); index -= 2; }
    return { path, isNew: at(index - 1)?.type === "ident" && at(index - 1).value === "new", name: nameToken.value };
  }

  /** Découpe les arguments d'un appel (indices de jetons). */
  function argumentsOf(openIndex, closeIndex) {
    const result = [];
    let depth = 0;
    let start = openIndex + 1;
    for (let index = openIndex + 1; index < closeIndex; index += 1) {
      const token = at(index);
      if (token.type !== "punct") continue;
      if ("([{".includes(token.value)) depth += 1;
      else if (")]}".includes(token.value)) depth -= 1;
      else if (token.value === "," && depth === 0) { result.push([start, index]); start = index + 1; }
    }
    if (start < closeIndex) result.push([start, closeIndex]);
    return result;
  }

  /** Propriétés d'un littéral d'objet `{ a, b: 1, "c": 2 }` ; `null` si indécidable (spread, calculé). */
  function objectKeys(from, to) {
    if (!isPunct(at(from), "{") || !isPunct(at(to - 1), "}")) return null;
    const keys = [];
    let depth = 0;
    let expectKey = true;
    for (let index = from + 1; index < to - 1; index += 1) {
      const token = at(index);
      if (depth === 0 && expectKey) {
        if (isPunct(token, "...") || isPunct(token, "[")) return null;
        if (token.type === "ident" || token.type === "string") {
          const next = at(index + 1);
          if (isPunct(next, ":") || isPunct(next, ",") || index + 1 === to - 1 || isPunct(next, "(")) keys.push(token.value);
          else return null;
        }
        expectKey = false;
      }
      if (token.type === "punct") {
        if ("([{".includes(token.value)) depth += 1;
        else if (")]}".includes(token.value)) depth -= 1;
        else if (token.value === "," && depth === 0) expectKey = true;
      }
    }
    return keys;
  }

  /** Cible d'une affectation qui précède le jeton (`x.textContent = … "texte"`), ou null. */
  function assignmentTarget(index) {
    let depth = 0;
    for (let cursor = index - 1; cursor >= 0; cursor -= 1) {
      const token = at(cursor);
      if (token.type !== "punct") continue;
      if (")]}".includes(token.value)) depth += 1;
      else if ("([{".includes(token.value)) { if (depth === 0) return null; depth -= 1; }
      else if (depth === 0 && (token.value === ";" || token.value === "," || token.value === "=>")) return null;
      else if (depth === 0 && ["=", "+=", "||=", "??="].includes(token.value)) return at(cursor - 1)?.type === "ident" ? at(cursor - 1).value : null;
    }
    return null;
  }

  const isKeyCallee = calleePath => KEY_CALLEES.some(path => calleePath.length >= path.length && path.every((part, offsetIndex) => calleePath.at(-path.length + offsetIndex) === part));
  const frames = [];
  const nearestCall = () => {
    for (let index = frames.length - 1; index >= 0; index -= 1) {
      if (frames[index].kind === "call") return frames[index];
      if (frames[index].kind !== "group") return null;
    }
    return null;
  };

  tokens.forEach((token, index) => {
    if (token.type === "punct") {
      if (token.value === "(") {
        const callee = calleeOf(index) ?? (isPunct(at(index - 1), ")") || isPunct(at(index - 1), "]") ? { path: [], name: "", isNew: false } : null);
        frames.push(callee ? { kind: "call", callee, argIndex: 0, open: index } : { kind: "group", open: index });
      } else if (token.value === "[" || token.value === "{") frames.push({ kind: "bracket", open: index });
      else if (token.value === ")" || token.value === "]" || token.value === "}") {
        const frame = frames.pop();
        if (frame?.kind === "call" && token.value === ")") { collectCall(frame, index); checkDynamic(frame, index); }
      } else if (token.value === "," && frames.at(-1)?.kind === "call") frames.at(-1).argIndex += 1;
      return;
    }
    if (token.type !== "string" && token.type !== "template") return;
    const previous = at(index - 1);
    const next = at(index + 1);
    const call = nearestCall();

    // 1. Utilisations de clés.
    if (call && call.argIndex === 0 && isKeyCallee(call.callee.path)
      && previous?.type === "punct" && KEY_POSITION.has(previous.value)) {
      if (isPunct(next, "+") && token.type === "string") prefixes.push(token.value); // "status-" + state
      else if (token.type === "string" || token.exprCount === 0) uses.push({ key: staticText(token), line: token.line + offset, kind: "call" });
      else if (token.quasis[0]) prefixes.push(token.quasis[0]);
    }
    for (const match of staticText(token).matchAll(/\[data-i18n(?:-[a-z-]+)?=["']?([^"'\]]+)["']?\]/g)) uses.push({ key: match[1], line: token.line + offset, kind: "selector" });
    if (token.type === "string" && !((isPunct(previous, "{") || isPunct(previous, ",")) && isPunct(next, ":"))) uses.push({ key: token.value, line: token.line + offset, kind: "literal" });

    // 2. Chaînes en dur.
    if (isIgnored(token)) return;
    if (previous?.type === "ident" && (previous.value === "from" || previous.value === "import" || previous.value === "case")) return;
    if (isPunct(previous, "[") && isPunct(next, "]")) return;
    if ((isPunct(previous, "{") || isPunct(previous, ",")) && isPunct(next, ":")) return;
    if (COMPARISONS.has(previous?.value) || COMPARISONS.has(next?.value)) return;
    let relaxed = false;
    if (call) {
      const { callee } = call;
      const attribute = call.argIndex === 0 ? null : at(call.open + 1);
      if (callee.isNew ? NON_DISPLAY_NEW.test(callee.name) : NON_DISPLAY_CALLEES.has(callee.name) || NON_DISPLAY_CALLEE_PATTERN.test(callee.name)) return;
      if (callee.name === "setAttribute") {
        if (call.argIndex === 0 || attribute?.type !== "string" || !DISPLAY_ATTRS.has(attribute.value)) return;
        relaxed = true;
      } else if (DISPLAY_METHODS.has(callee.name)) relaxed = true;
    }
    const target = assignmentTarget(index);
    if (target && DATA_PROPS.has(target)) return;
    if (target && DISPLAY_PROPS.has(target)) relaxed = true;
    if (isPunct(previous, ":") && at(index - 2)?.type !== "punct" && DISPLAY_KEYS.has(at(index - 2)?.value) && frames.at(-1)?.kind === "bracket") relaxed = true;
    if (token.type === "string" && frames.at(-1)?.kind === "bracket" && isPunct(next, ":")) return;
    const text = staticText(token);
    if (DIAGNOSTIC_MODULES.has(file) && !ACCENTED.test(text)) return;
    if (looksHuman(text, relaxed)) findings.push({ category: "hardcoded", file, line: token.line + offset, detail: clip(JSON.stringify(text.replaceAll("\u0001", "${…}"))), message: `chaîne en dur ${clip(JSON.stringify(text.replaceAll("\u0001", "${…}")))}` });
  });

  /** Une clé sans aucun littéral (variable, appel) ne peut pas être vérifiée statiquement. */
  function checkDynamic(frame, closeIndex) {
    if (!isKeyCallee(frame.callee.path)) return;
    const first = argumentsOf(frame.open, closeIndex)[0];
    if (!first) return;
    let literal = false;
    for (let index = first[0]; index < first[1]; index += 1) if (at(index).type === "string" || at(index).type === "template") literal = true;
    if (!literal) dynamics.push({ line: at(frame.open).line + offset });
  }

  /** Paramètres de t("clé", { … }) quand la forme est statique. */
  function collectCall(frame, closeIndex) {
    const path = frame.callee.path;
    if (!(path.length >= 1 && path.at(-1) === "t" && (path.length === 1 || path.at(-2) === "i18n" || path.at(-2) === "this"))) return;
    const args = argumentsOf(frame.open, closeIndex);
    if (!args.length) return;
    const [from, to] = args[0];
    const keyToken = at(from);
    if (to - from !== 1 || !(keyToken.type === "string" || (keyToken.type === "template" && keyToken.exprCount === 0))) return;
    let given = [];
    if (args[1]) {
      given = objectKeys(args[1][0], args[1][1]);
      if (given === null || args.length > 2) return;
    }
    calls.push({ key: staticText(keyToken), line: keyToken.line + offset, given });
  }

  return { findings, uses, prefixes, calls, dynamics };
}

// ---------------------------------------------------------------------------------------------
// Analyse de HTML
// ---------------------------------------------------------------------------------------------

const VOID_ELEMENTS = new Set(["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"]);
const ENTITIES = { "&amp;": "&", "&lt;": "<", "&gt;": ">", "&quot;": '"', "&apos;": "'", "&nbsp;": "\u00a0", "&#39;": "'" };
const decodeEntities = text => text.replace(/&(?:amp|lt|gt|quot|apos|nbsp|#39);/g, entity => ENTITIES[entity]);

/** @param {string} source @param {string} file */
export function analyzeHtml(source, file) {
  /** @type {Finding[]} */
  const findings = [];
  const uses = [];
  const lineStarts = [0];
  for (let index = 0; index < source.length; index += 1) if (source[index] === "\n") lineStarts.push(index + 1);
  const lineOf = position => {
    let low = 0;
    let high = lineStarts.length - 1;
    while (low < high) {
      const middle = (low + high + 1) >> 1;
      if (lineStarts[middle] <= position) low = middle; else high = middle - 1;
    }
    return low + 1;
  };
  const comments = [];
  const textLines = new Set();
  const pending = []; // texte et attributs à juger après lecture des exemptions
  const stack = [];
  const scripts = [];
  let position = 0;

  while (position < source.length) {
    if (source.startsWith("<!--", position)) {
      let end = source.indexOf("-->", position);
      end = end < 0 ? source.length : end + 3;
      comments.push({ text: source.slice(position + 4, end), line: lineOf(position), endLine: lineOf(end - 1) });
      position = end;
      continue;
    }
    if (source[position] === "<" && /[A-Za-z]/.test(source[position + 1] ?? "")) {
      const close = (() => {
        let quote = null;
        for (let at = position + 1; at < source.length; at += 1) {
          const char = source[at];
          if (quote) { if (char === quote) quote = null; } else if (char === '"' || char === "'") quote = char; else if (char === ">") return at;
        }
        return source.length;
      })();
      const tag = source.slice(position + 1, close);
      const name = /^[A-Za-z][A-Za-z0-9-]*/.exec(tag)[0].toLowerCase();
      const attributes = new Map();
      for (const match of tag.slice(name.length).matchAll(/([^\s"'<>/=]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+)))?/g)) {
        attributes.set(match[1].toLowerCase(), decodeEntities(match[2] ?? match[3] ?? match[4] ?? ""));
      }
      const line = lineOf(position);
      for (const [attribute, value] of attributes) {
        if (attribute === "data-i18n" || attribute.startsWith("data-i18n-")) uses.push({ key: value, line, kind: "attribute" });
        if (DISPLAY_ATTRS.has(attribute) && value.trim()) pending.push({ kind: "attribute", attribute, value, line, owner: attributes });
      }
      position = close + 1;
      if (tag.trimEnd().endsWith("/") || VOID_ELEMENTS.has(name)) continue;
      if (name === "script" || name === "style") {
        const end = source.toLowerCase().indexOf(`</${name}`, position);
        const stop = end < 0 ? source.length : end;
        if (name === "script" && !attributes.has("src") && source.slice(position, stop).trim()) scripts.push({ code: source.slice(position, stop), line: lineOf(position) - 1 });
        position = stop;
        continue;
      }
      stack.push({ name, i18n: attributes.has("data-i18n") });
      continue;
    }
    if (source.startsWith("</", position)) {
      const close = source.indexOf(">", position);
      const name = source.slice(position + 2, close < 0 ? source.length : close).trim().toLowerCase();
      while (stack.length && stack.pop().name !== name) { /* ferme les éléments laissés ouverts */ }
      position = close < 0 ? source.length : close + 1;
      continue;
    }
    let end = source.indexOf("<", position + 1);
    if (end < 0) end = source.length;
    const raw = source.slice(position, end);
    const text = decodeEntities(raw);
    if (text.trim() && !stack.some(element => element.i18n)) pending.push({ kind: "text", value: text, line: lineOf(position + raw.search(/\S/)) });
    position = end;
  }

  const lineHasTag = new Set(pending.map(item => item.line));
  const ignored = exemptions(comments.map(comment => ({ ...comment, text: comment.text })), lineHasTag, file, findings);
  for (const item of pending) {
    if (ignored.has(item.line)) continue;
    if (item.kind === "attribute" && (item.owner.has(`data-i18n-${item.attribute}`))) continue;
    if (looksHuman(item.value, true)) {
      findings.push({ category: "hardcoded", file, line: item.line, detail: clip(JSON.stringify(item.value.trim())), message: `${item.kind === "attribute" ? `attribut ${item.attribute}` : "texte"} en dur ${clip(JSON.stringify(item.value.trim()))}` });
    }
  }
  const scriptResults = scripts.map(script => analyzeScript(script.code, file, { lineOffset: script.line }));
  return {
    findings: findings.concat(scriptResults.flatMap(result => result.findings)),
    uses: uses.concat(scriptResults.flatMap(result => result.uses)),
    prefixes: scriptResults.flatMap(result => result.prefixes),
    dynamics: scriptResults.flatMap(result => result.dynamics),
    calls: scriptResults.flatMap(result => result.calls),
  };
}

// ---------------------------------------------------------------------------------------------
// Contrôle d'un dépôt
// ---------------------------------------------------------------------------------------------

function listFiles(directory, accept) {
  const result = [];
  for (const name of readdirSync(directory).sort()) {
    const path = join(directory, name);
    if (statSync(path).isDirectory()) { if (name !== "i18n") result.push(...listFiles(path, accept)); } else if (accept(name)) result.push(path);
  }
  return result;
}

/**
 * Remplace par des lignes vides les éléments `#[cfg(test)]` (module, fonction ou `use`) : une clé qui
 * n'apparaît que dans un test reste « inutilisée ». Les accolades sont comptées hors littéraux.
 * @param {string[]} lines
 */
function blankTestBlocks(lines) {
  const result = [...lines];
  for (let index = 0; index < lines.length; index += 1) {
    if (!lines[index].trimStart().startsWith("#[cfg(test)]")) continue;
    let depth = 0;
    let opened = false;
    let end = index;
    for (; end < lines.length; end += 1) {
      const bare = lines[end].replace(/"(?:\\.|[^"\\])*"/g, '""').replace(/'(?:\\.|[^'\\])'/g, "''").replace(/\/\/.*$/, "");
      for (const char of bare) {
        if (char === "{") { depth += 1; opened = true; } else if (char === "}") depth -= 1;
      }
      if (opened ? depth <= 0 : end > index && bare.includes(";")) break;
    }
    for (let at = index; at <= Math.min(end, lines.length - 1); at += 1) result[at] = "";
    index = end;
  }
  return result;
}

/**
 * Clés du shell natif : littéraux `"native.…"` des sources Rust (hors `i18n.rs`, dont les littéraux
 * sont des cas de test) et paramètres littéraux des appels `t("clé", &[("nom", …)])`.
 * @param {string} root
 * @param {(path: string) => string} rel
 */
export function analyzeRustSources(root, rel) {
  const uses = [];
  const calls = [];
  const srcDir = join(root, "crates", "desktop-win", "src");
  if (!existsSync(srcDir)) return { uses, calls };
  for (const path of listFiles(srcDir, name => name.endsWith(".rs") && name !== "i18n.rs")) {
    const file = rel(path);
    const lines = readFileSync(path, "utf8").split(/\r?\n/);
    const code = blankTestBlocks(lines).map(line => (line.trimStart().startsWith("//") ? "" : line));
    code.forEach((line, index) => {
      for (const match of line.matchAll(/"(native\.[A-Za-z0-9._-]+)"/g)) uses.push({ key: match[1], line: index + 1, file, kind: "rust" });
    });
    const text = code.join("\n");
    for (const match of text.matchAll(/\bt\(\s*"(native\.[A-Za-z0-9._-]+)"\s*,\s*&\[/g)) {
      // Crochets équilibrés : une valeur peut contenir un appel imbriqué (`&t(clé, &[])`).
      let depth = 1;
      let end = match.index + match[0].length;
      while (end < text.length && depth > 0) {
        if (text[end] === "[") depth += 1;
        else if (text[end] === "]") depth -= 1;
        end += 1;
      }
      // Seuls les couples `("nom", …)` du premier niveau de la liste donnent des paramètres.
      const body = text.slice(match.index + match[0].length, end - 1);
      const given = [];
      let level = 0;
      for (let at = 0; at < body.length; at += 1) {
        if (body[at] === "(" || body[at] === "[" || body[at] === "{") {
          if (level === 0 && body[at] === "(") {
            const name = /^\(\s*"([A-Za-z_][A-Za-z0-9_]*)"\s*,/.exec(body.slice(at));
            if (name) given.push(name[1]);
          }
          level += 1;
        } else if (body[at] === ")" || body[at] === "]" || body[at] === "}") level -= 1;
      }
      calls.push({ key: match[1], line: text.slice(0, match.index).split("\n").length, file, given });
    }
  }
  return { uses, calls };
}

/**
 * Lance tous les contrôles sans lire ni écrire la référence.
 * @param {{ root: string }} options racine contenant `web/` et `web/i18n/*.txt`
 * @returns {{ findings: Finding[], scanned: string[], catalogKeys: number, usedKeys: number }}
 */
export function checkRepository({ root }) {
  const webDir = join(root, "web");
  const rel = path => relative(root, path).split("\\").join("/");
  /** @type {Finding[]} */
  const findings = [];
  const uses = [];
  const prefixes = [];
  const calls = [];
  const info = [];
  const scanned = [];

  const catalogDir = join(webDir, "i18n");
  const catalogs = existsSync(catalogDir) ? readdirSync(catalogDir).filter(name => name.endsWith(".txt")).sort() : [];
  const referenceFile = `${REFERENCE_LOCALE}.txt`;
  const parsed = new Map();
  for (const name of catalogs) {
    const file = rel(join(catalogDir, name));
    const catalog = parseCatalog(readFileSync(join(catalogDir, name), "utf8"));
    parsed.set(name, catalog);
    for (const problem of catalog.problems) {
      const category = problem.code === "invalid-message" ? "message-syntax" : problem.code;
      findings.push({ category, file, line: problem.line, detail: problem.key ?? `ligne ${problem.line}`, message: problem.message });
    }
    for (const [key, entry] of catalog.entries) {
      for (const warning of analyzeMessage(entry.value, { locale: name.replace(/\.txt$/, "") }).warnings) {
        findings.push({ category: "plural-category", file, line: entry.line, detail: key, message: `${key} : ${warning}` });
      }
    }
  }
  const reference = parsed.get(referenceFile);
  if (!reference) findings.push({ category: "no-catalog", file: rel(join(catalogDir, referenceFile)), line: 1, detail: referenceFile, message: "catalogue de référence introuvable" });
  for (const [name, catalog] of parsed) {
    if (!reference || name === referenceFile) continue;
    const file = rel(join(catalogDir, name));
    for (const key of reference.entries.keys()) if (!catalog.entries.has(key)) findings.push({ category: "locale-missing", file, line: 1, detail: key, message: `clé ${key} absente de ${name}` });
    for (const [key, entry] of catalog.entries) if (!reference.entries.has(key)) findings.push({ category: "locale-extra", file, line: entry.line, detail: key, message: `clé ${key} absente du catalogue de référence` });
  }

  const sources = listFiles(webDir, name => /\.(?:mjs|js|html)$/.test(name));
  for (const path of sources) {
    const file = rel(path);
    scanned.push(file);
    const text = readFileSync(path, "utf8");
    const result = path.endsWith(".html") ? analyzeHtml(text, file) : analyzeScript(text, file);
    for (const finding of result.findings) findings.push(finding);
    for (const use of result.uses) uses.push({ ...use, file });
    for (const prefix of result.prefixes) prefixes.push(prefix);
    for (const dynamic of result.dynamics) info.push({ category: "dynamic-key", file, line: dynamic.line, detail: "dynamic-key", message: "clé dynamique, invérifiable" });
    for (const call of result.calls) calls.push({ ...call, file });
  }

  const rust = analyzeRustSources(root, rel);
  uses.push(...rust.uses);
  calls.push(...rust.calls);

  if (reference) {
    const keys = reference.entries;
    const used = new Set();
    for (const use of uses) {
      if (use.kind === "literal") { if (keys.has(use.key)) used.add(use.key); continue; }
      if (keys.has(use.key)) used.add(use.key);
      else findings.push({ category: "missing-key", file: use.file, line: use.line, detail: use.key, message: `clé ${use.key} absente du catalogue` });
    }
    for (const key of keys.keys()) if (prefixes.some(prefix => key.startsWith(prefix))) used.add(key);
    const catalogFile = rel(join(catalogDir, referenceFile));
    for (const [key, entry] of keys) {
      if (!used.has(key)) findings.push({ category: "unused-key", file: catalogFile, line: entry.line, detail: key, message: `clé ${key} inutilisée` });
    }
    for (const call of calls) {
      const entry = keys.get(call.key);
      if (!entry) continue;
      let declared;
      try { declared = compileMessage(entry.value).params; } catch { continue; }
      for (const name of declared) {
        if (!call.given.includes(name)) findings.push({ category: "param-missing", file: call.file, line: call.line, detail: `${call.key}:${name}`, message: `${call.key} déclare {${name}} mais l'appel ne le fournit pas` });
      }
      for (const name of call.given) {
        if (!declared.has(name)) findings.push({ category: "param-extra", file: call.file, line: call.line, detail: `${call.key}:${name}`, message: `l'appel fournit ${name} que ${call.key} ne déclare pas` });
      }
    }
    return { findings, info, scanned, catalogKeys: keys.size, usedKeys: used.size };
  }
  return { findings, info, scanned, catalogKeys: 0, usedKeys: 0 };
}

// ---------------------------------------------------------------------------------------------
// Fichier de référence
// ---------------------------------------------------------------------------------------------

const fingerprint = finding => `${finding.category}\t${finding.file}\t${finding.detail}`;

/** @param {Finding[]} findings */
export function formatBaseline(findings) {
  const counts = new Map();
  for (const finding of findings) counts.set(fingerprint(finding), (counts.get(fingerprint(finding)) ?? 0) + 1);
  const lines = [...counts].map(([key, count]) => `${count}\t${key}`).sort();
  return [
    "# SPDX-License-Identifier: AGPL-3.0-or-later",
    "# Écarts i18n connus, comparés par tests/check-i18n.mjs : tout écart absent de ce fichier fait échouer le contrôle.",
    "# Format : nombre<TAB>catégorie<TAB>fichier<TAB>détail. Les numéros de ligne ne comptent pas.",
    "# Régénérer avec : node tests/check-i18n.mjs --update-baseline (après avoir corrigé des écarts, jamais pour en masquer).",
    ...lines, "",
  ].join("\n");
}

/** @param {string} text */
export function parseBaseline(text) {
  const counts = new Map();
  for (const line of text.split(/\r?\n/)) {
    if (!line || line.startsWith("#")) continue;
    const separator = line.indexOf("\t");
    counts.set(line.slice(separator + 1), (counts.get(line.slice(separator + 1)) ?? 0) + Number(line.slice(0, separator)));
  }
  return counts;
}

/**
 * Sépare les écarts nouveaux des écarts connus.
 * @param {Finding[]} findings @param {Map<string, number> | null} baseline
 */
export function compareToBaseline(findings, baseline) {
  if (!baseline) return { fresh: findings, known: [], resolved: [] };
  const remaining = new Map(baseline);
  const fresh = [];
  const known = [];
  for (const finding of findings) {
    const key = fingerprint(finding);
    if ((remaining.get(key) ?? 0) > 0) { remaining.set(key, remaining.get(key) - 1); known.push(finding); } else fresh.push(finding);
  }
  const resolved = [...remaining].filter(([, count]) => count > 0).map(([key, count]) => `${count}\t${key}`);
  return { fresh, known, resolved };
}

// ---------------------------------------------------------------------------------------------
// Ligne de commande
// ---------------------------------------------------------------------------------------------

const order = list => [...list].sort((a, b) => a.file.localeCompare(b.file) || a.line - b.line || a.category.localeCompare(b.category));
const line = finding => `${finding.file}:${finding.line}: [${finding.category}] ${finding.message}`;

function summary(findings) {
  const byCategory = new Map();
  for (const finding of findings) {
    const entry = byCategory.get(finding.category) ?? new Map();
    entry.set(finding.file, (entry.get(finding.file) ?? 0) + 1);
    byCategory.set(finding.category, entry);
  }
  return [...byCategory].sort().map(([category, files]) => `  ${category} : ${[...files.values()].reduce((a, b) => a + b, 0)} (${[...files].map(([file, count]) => `${file} ${count}`).join(", ")})`);
}

/** @param {string[]} argv @param {{ write?: (text: string) => void, error?: (text: string) => void }} [io] */
export function main(argv, io = {}) {
  const write = io.write ?? (text => process.stdout.write(`${text}\n`));
  const options = { root: resolve(HERE, ".."), strict: false, update: false, quiet: false, baseline: null };
  for (const argument of argv) {
    if (argument === "--strict") options.strict = true;
    else if (argument === "--update-baseline") options.update = true;
    else if (argument === "--quiet") options.quiet = true;
    else if (argument.startsWith("--root=")) options.root = resolve(argument.slice(7));
    else if (argument.startsWith("--baseline=")) options.baseline = resolve(argument.slice(11));
    else { write(`argument inconnu : ${argument}`); return 2; }
  }
  const baselinePath = options.baseline ?? join(options.root, DEFAULT_BASELINE);
  const result = checkRepository({ root: options.root });
  const findings = order(result.findings);
  write(`i18n : ${result.scanned.length} fichiers, ${result.catalogKeys} clés au catalogue, ${result.usedKeys} utilisées.`);

  if (options.update) {
    mkdirSync(dirname(baselinePath), { recursive: true });
    writeFileSync(baselinePath, formatBaseline(findings), "utf8");
    write(`Référence écrite : ${relative(options.root, baselinePath).split("\\").join("/")} (${findings.length} écarts).`);
    return 0;
  }
  const baseline = options.strict || !existsSync(baselinePath) ? null : parseBaseline(readFileSync(baselinePath, "utf8"));
  if (!options.strict && !baseline) write("Aucun fichier de référence : mode strict.");
  const { fresh, known, resolved } = compareToBaseline(findings, baseline);
  if (fresh.length) {
    write(`\n${baseline ? "NOUVEAUX écarts" : "Écarts"} (${fresh.length}) :`);
    for (const finding of fresh) write(line(finding));
  }
  if (known.length) {
    write(`\nÉcarts connus de la référence (${known.length}) :`);
    if (options.quiet) for (const text of summary(known)) write(text);
    else for (const finding of known) write(`  ${line(finding)}`);
  }
  if (resolved.length) {
    write(`\nÉcarts de la référence corrigés depuis (${resolved.length}) ; régénérer la référence pour les retirer :`);
    for (const entry of resolved) write(`  ${entry.replaceAll("\t", " | ")}`);
  }
  if (result.info.length) {
    write(`
Clés dynamiques, invérifiables (informatif, sans effet sur le code de sortie) (${result.info.length}) :`);
    for (const finding of order(result.info)) write(`  ${line(finding)}`);
  }
  write("\nRésumé par catégorie :");
  for (const text of summary(findings)) write(text);
  write(fresh.length ? `\nÉCHEC : ${fresh.length} écart(s) ${baseline ? "nouveau(x)" : ""}`.trimEnd() : "\nOK : aucun nouvel écart.");
  return fresh.length ? 1 : 0;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) process.exitCode = main(process.argv.slice(2));
