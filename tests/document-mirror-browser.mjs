// SPDX-License-Identifier: AGPL-3.0-or-later
// US-209 : miroir DOM accessible dans Edge/Chromium (structure, rôles, alt, sélection DOM, focus, contraste forcé).
import { createDocumentMirror } from "/document-mirror.mjs";
import { attachTextInput } from "/text-input.mjs";
import { createI18n } from "/i18n.mjs";

const checks = [];
const assert = (value, name) => { if (!value) throw new Error(name); checks.push(name); };
const forced = new URLSearchParams(location.search).get("forced") === "1";
const enc = new TextEncoder();
const item = (start, text, extra = {}) => ({ paraStart: BigInt(start), paraEnd: BigInt(start + enc.encode(text).length), role: 0, level: 0, label: "", text, objects: [], ...extra });
// Paragraph starts follow the flat text: one newline byte between paragraphs.
const specs = [
  ["Titre un", { role: 1, level: 1 }], ["Sous-titre", { role: 1, level: 3 }], ["Très profond", { role: 1, level: 8 }],
  ["Premier", { role: 2, level: 0, label: "1." }], ["Second niveau", { role: 2, level: 1, label: "•" }],
  ["Voir ￼ et ￼.", { objects: [{ offset: 5n, alt: "Un chat noir" }, { offset: 12n, alt: "" }] }], ["Café crème", {}],
];
const starts = [];
for (let at = 0, i = 0; i < specs.length; i += 1) { starts.push(at); at += enc.encode(specs[i][0]).length + 1; }
const baseDoc = (replace = {}) => specs.map(([text, extra], i) => item(starts[i], replace[i] ?? text, extra));
const [S_SECOND, S_FIRST, S_LAST] = [starts[4], starts[3], starts[6]];

async function main() {
  const catalog = await (await fetch("/i18n/fr-FR.txt")).text();
  const i18n = createI18n({ locale: "fr-FR", catalog });
  const t = (key, values) => i18n.t(key, values);
  const host = document.getElementById("host");
  let selection = { anchor: 0n, focus: 0n };
  const input = attachTextInput({ container: host, getSelection: () => selection, getCaret: () => ({ x: 10, y: 10, width: 1, height: 12 }), onCommand: () => {} });
  input.focus();
  assert(document.activeElement === input.element, "the input has the focus before the mirror exists");

  let revision = 1n;
  let current = baseDoc();
  const calls = [];
  const mirror = createDocumentMirror({
    container: host, t, activeDescendantTarget: input.element,
    getAccessibleWindow: async (start, end, { signal }) => {
      calls.push([start, end]);
      await new Promise(resolve => setTimeout(resolve, 5));
      if (signal.aborted) throw new DOMException("aborted", "AbortError");
      return { revision, items: current };
    },
  });
  const root = mirror.element;
  const push = () => mirror.update({ revision, docLen: 200n, selection, visible: { start: 0n, end: 120n } });
  assert((await push()).status === "applied", "first window applied");

  // Structure, roles, levels, alt.
  assert(root.getAttribute("role") === "document" && root.getAttribute("aria-label") === "Contenu du document", "root is a labelled document");
  assert(root.querySelector("h1")?.textContent === "Titre un", "level 1 heading is h1");
  assert(root.querySelector("h3")?.textContent === "Sous-titre", "level 3 heading is h3");
  const deep = root.querySelector("[role=heading]");
  assert(deep?.getAttribute("aria-level") === "8" && deep.tagName === "DIV", "level 8 heading uses role=heading and aria-level");
  const list = root.querySelector("[role=list]");
  assert(list && list.children.length === 2, "consecutive list items share one list");
  assert([...list.children].map(n => n.getAttribute("role") + n.getAttribute("aria-level") + n.textContent).join("|") === "listitem11. Premier|listitem2• Second niveau", "list items carry label and level");
  const images = [...root.querySelectorAll("[role=img]")];
  assert(images.length === 2 && images[0].getAttribute("aria-label") === "Un chat noir", "image alt text is exposed");
  assert(images[1].getAttribute("aria-label") === "Image sans texte alternatif", "image without alt is announced as such");
  assert(root.querySelectorAll("script, img").length === 0, "no script or img element");

  // Out of sight but readable by assistive technology.
  const rect = root.getBoundingClientRect();
  const style = getComputedStyle(root);
  assert(rect.width <= 1 && rect.height <= 1, "mirror occupies at most 1x1 px");
  assert(style.display !== "none" && style.visibility !== "hidden" && root.getAttribute("aria-hidden") === null && !root.hidden, "mirror is not hidden from the accessibility tree");
  assert(style.clipPath.startsWith("inset"), "mirror is clipped out of sight");
  assert(!root.querySelector("[tabindex]") && !root.hasAttribute("tabindex"), "mirror takes no tab stop");
  assert(forced === matchMedia("(forced-colors: active)").matches, forced ? "forced-colors is active in this run" : "forced-colors is inactive in the normal run");
  assert(style.borderTopWidth === "0px" && style.outlineWidth === "0px" || style.outlineStyle === "none", "no system border or outline on the mirror");
  assert(getComputedStyle(root.querySelector("p")).borderTopWidth === "0px", "no system border on mirror paragraphs");

  // Selection follows the model; focus stays in the input.
  selection = { anchor: BigInt(S_SECOND), focus: BigInt(S_SECOND + 7) };
  await push();
  const dom = getSelection();
  assert(dom.toString().trim() === "Second", "DOM selection selects the model range (UTF-8 offsets)");
  assert(document.activeElement === input.element, "setting the DOM selection does not steal the focus");
  selection = { anchor: BigInt(S_SECOND), focus: BigInt(S_FIRST) };
  await push();
  assert(dom.focusNode.parentNode.closest("[role=listitem]").textContent.startsWith("1."), "reversed selection: the focus end lands in the first list item");
  assert(dom.anchorNode.parentNode.closest("[role=listitem]").textContent.startsWith("•"), "reversed selection: the anchor end stays in the second item");
  assert(document.activeElement === input.element, "focus kept after a reversed selection");
  const accent = BigInt(S_LAST) + BigInt(enc.encode("Caf").length);
  selection = { anchor: accent, focus: accent + 2n };
  await push();
  assert(dom.toString() === "é", "multi-byte character selected by its UTF-8 range");
  selection = { anchor: BigInt(S_LAST), focus: BigInt(S_LAST) };
  await push();
  assert(dom.isCollapsed && dom.anchorOffset === 0, "caret is a collapsed DOM selection");
  const last = [...root.querySelectorAll("p")].at(-1);
  assert(input.element.getAttribute("aria-activedescendant") === last.id, "aria-activedescendant names the current paragraph");
  const live = root.querySelector("[role=status]");
  assert(live.getAttribute("aria-live") === "polite" && live.textContent === "C", "live region announces the character after the caret within a paragraph");
  selection = { anchor: 0n, focus: 0n };
  await push();
  assert(live.textContent === "Titre de niveau 1 : Titre un", "live region announces headings with their level");
  selection = { anchor: 0n, focus: 5n };
  await push();
  assert(live.textContent === "Sélection : Titre", "live region announces the selected text");

  // The user can still type: the input keeps the focus and its content.
  input.element.value = "x";
  assert(document.activeElement === input.element && input.element.value === "x", "the input still holds focus and text");
  input.element.value = "";

  // Incremental update: only the changed paragraph is replaced, unchanged nodes are kept.
  const before = [...root.querySelectorAll("p")];
  const callsBefore = calls.length;
  revision = 2n;
  current = baseDoc({ 6: "Café crème brûlée" });
  assert((await push()).status === "applied" && calls.length === callsBefore + 1, "a new revision triggers one request");
  const after = [...root.querySelectorAll("p")];
  assert(after.length === 2 && after[0] === before[0] && after[1] !== before[1] && after.at(-1).textContent === "Café crème brûlée", "only the edited paragraph is rebuilt");
  assert(document.activeElement === input.element, "focus kept after a content update");

  // A superseded request is cancelled and never applied.
  let release;
  const gate = new Promise(resolve => { release = resolve; });
  const slow = createDocumentMirror({ container: host, t, getAccessibleWindow: async (start, end, { signal }) => { await gate; if (signal.aborted) throw new DOMException("aborted", "AbortError"); return { revision: 5n, items: [item(0, "périmé")] }; } });
  const old = slow.update({ revision: 5n, docLen: 10n, selection: { anchor: 0n, focus: 0n } });
  const newer = slow.update({ revision: 6n, docLen: 10n, selection: { anchor: 0n, focus: 0n } });
  release();
  assert((await old).status === "stale" && slow.element.querySelector("p") === null && (await newer).status === "stale", "stale revision never reaches the DOM");
  slow.dispose();

  mirror.dispose();
  assert(!root.isConnected && !input.element.hasAttribute("aria-activedescendant"), "dispose removes the mirror and the reference");
  input.dispose();
}

let report;
try { await main(); report = { status: "passed", forced, checks }; } catch (error) { report = { status: "failed", forced, checks, message: String(error?.message ?? error), stack: String(error?.stack ?? "") }; }
document.getElementById("result").textContent = JSON.stringify(report);
await fetch("/__mirror_result", { method: "POST", body: JSON.stringify(report) });
